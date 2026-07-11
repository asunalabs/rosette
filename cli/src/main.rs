//! Manual dogfooding CLI: `listen` prints a contact link; `connect` scans one
//! and pairs. Both land in the same chat REPL. The automated proof of the
//! protocol lives in `tests/three_client_convergence.rs` — this binary is
//! for a human to actually watch two terminals talk to each other.
//!
//! v0.1 scope cut (disclosed, tracked alongside T4 in
//! tasks-eng-review-*.jsonl): the bootstrap payload's ratchet tree and group
//! inbox credentials ride unencrypted past the Welcome's own MLS encryption
//! — fine for a same-machine demo, not yet the hardened pairing spec.

use base64::Engine;
use chatcore::{message_id_for, ChatSession, Incoming};
use cli::RelayClient;
use clap::{Parser, Subcommand};
use proto::{ContactLink, DeliveryMode, Endpoint, Envelope, GroupSendKind, QueueId};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print a contact link, then wait to be paired and chat.
    Listen {
        #[arg(long, default_value = "alice")]
        name: String,
        #[arg(long, default_value = "127.0.0.1:7443")]
        relay: String,
    },
    /// Scan a contact link printed by `listen`, pair, and chat.
    Connect {
        link: String,
        #[arg(long, default_value = "bob")]
        name: String,
        #[arg(long, default_value = "127.0.0.1:7443")]
        relay: String,
    },
}

/// What travels through the bootstrap mailbox: the Welcome (self-encrypted
/// by MLS to the invitee's KeyPackage — safe for the relay to forward
/// blind) plus the ratchet tree and the fresh group inbox's credentials.
#[derive(Serialize, Deserialize)]
struct BootstrapPayload {
    welcome_wire: Vec<u8>,
    tree_wire: Vec<u8>,
    inbox_qid: QueueId,
    inbox_key: [u8; 32],
}

fn wrap(wire_bytes: Vec<u8>) -> Envelope {
    let padded = proto::pad(&wire_bytes).expect("demo messages fit the largest padding bucket");
    Envelope::new(message_id_for(&wire_bytes), DeliveryMode::RelayFanout, padded)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Listen { name, relay } => listen(&name, &relay).await,
        Command::Connect { link, name, relay } => connect(&link, &name, &relay).await,
    }
}

async fn listen(name: &str, relay_addr: &str) -> anyhow::Result<()> {
    let mut session = ChatSession::new(name);
    let relay = RelayClient::connect(relay_addr).await?;
    let (mailbox_qid, mailbox_key) = relay.create_mailbox().await?;
    relay.subscribe(vec![mailbox_qid]).await?;

    let key_package = session.generate_key_package()?;
    let link = chatcore::pairing::build_contact_link(key_package.key_package(), relay_addr, mailbox_qid, mailbox_key)?;
    let link_b64 = base64::engine::general_purpose::STANDARD.encode(link.to_bytes());
    println!("Share this link: {link_b64}");
    println!("Waiting to be paired...");

    let mut relay = relay;
    let (inbox_qid, inbox_key) = loop {
        let (_qid, envelope) = relay.push_rx.recv().await.ok_or_else(|| anyhow::anyhow!("relay connection closed"))?;
        let payload: BootstrapPayload = bincode::deserialize(&envelope.padded_ciphertext)?;
        session.join_from_welcome(&payload.welcome_wire, &payload.tree_wire)?;
        println!("Paired. Epoch {}.", session.epoch()?);
        break (payload.inbox_qid, payload.inbox_key);
    };

    chat_repl(session, relay, inbox_qid, inbox_key).await
}

async fn connect(link_b64: &str, name: &str, relay_addr: &str) -> anyhow::Result<()> {
    let link_bytes = base64::engine::general_purpose::STANDARD.decode(link_b64.trim())?;
    let link = ContactLink::from_bytes(&link_bytes)?;
    let Endpoint { queue_id: peer_mailbox, send_key: peer_send_key, .. } = link.primary_endpoint().clone();

    let mut session = ChatSession::new(name);
    let relay = RelayClient::connect(relay_addr).await?;
    let (own_mailbox, _own_key) = relay.create_mailbox().await?;
    relay.subscribe(vec![own_mailbox]).await?;

    let peer_kp = chatcore::pairing::key_package_from_link(&link, session.provider())?;
    session.create_group()?;
    let welcome_wire = session.add_members(&[peer_kp])?;
    let tree_wire = session.export_ratchet_tree()?;

    let (inbox_qid, inbox_key) = relay.create_group_inbox(1, vec![own_mailbox, peer_mailbox]).await?;
    let payload = BootstrapPayload {
        welcome_wire,
        tree_wire,
        inbox_qid,
        inbox_key,
    };
    let envelope = wrap(bincode::serialize(&payload)?);
    relay.send_to_mailbox(peer_mailbox, &peer_send_key, envelope).await?;
    println!("Paired. Epoch {}.", session.epoch()?);

    chat_repl(session, relay, inbox_qid, inbox_key).await
}

async fn chat_repl(mut session: ChatSession, mut relay: RelayClient, inbox_qid: QueueId, inbox_key: [u8; 32]) -> anyhow::Result<()> {
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    println!("Type a message and press enter. Ctrl-D to quit.");
    // Amendment A3: the relay fans a group-inbox send out to every member's
    // mailbox, including the sender's own — track what this session just
    // authored so its own echo is skipped instead of fed back into MLS
    // (which correctly refuses to "process" your own already-applied
    // message; this dedup responsibility is the caller's, not core/'s).
    let mut authored = std::collections::HashSet::new();
    loop {
        tokio::select! {
            line = line_rx.recv() => {
                let Some(line) = line else { break };
                let wire = session.encrypt_application(line.as_bytes())?;
                let envelope = wrap(wire);
                authored.insert(envelope.message_id);
                relay
                    .send_to_group_inbox(inbox_qid, &inbox_key, GroupSendKind::Application, envelope)
                    .await??;
            }
            push = relay.push_rx.recv() => {
                let Some((_qid, envelope)) = push else { break };
                if authored.remove(&envelope.message_id) {
                    continue;
                }
                match session.process_incoming(&envelope.padded_ciphertext)? {
                    Incoming::Application(bytes) => println!("< {}", String::from_utf8_lossy(&bytes)),
                    Incoming::CommitApplied => println!("(group state updated, epoch {})", session.epoch()?),
                }
            }
        }
    }
    Ok(())
}
