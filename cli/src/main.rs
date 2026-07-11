//! Manual dogfooding CLI: `listen` prints a contact link; `connect` scans one
//! and pairs. Both land in the same chat REPL. A THIN shell over
//! `engine::ChatEngine` (architecture.md step 2) — every protocol behavior
//! (pairing, dedup, epoch-conflict retry, reconnect) lives in engine/, so
//! this binary contains no orchestration to drift out of sync with the app.
//! The automated proof of the stack lives in engine/tests/.

use clap::{Parser, Subcommand};
use engine::{ChatEngine, Event};
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
        /// The relay's TLS fingerprint (printed by the relay on startup). Baked
        /// into the contact link so the peer pins the same relay.
        #[arg(long)]
        relay_fingerprint: String,
        #[arg(long, default_value = "alice")]
        name: String,
        #[arg(long, default_value = "127.0.0.1:7443")]
        relay: String,
    },
    /// Scan a contact link printed by `listen`, pair, and chat. The relay
    /// address and fingerprint both come from the link.
    Connect {
        link: String,
        #[arg(long, default_value = "bob")]
        name: String,
    },
}

/// Parse a 64-char hex fingerprint into raw bytes.
fn parse_fingerprint(hex: &str) -> anyhow::Result<[u8; 32]> {
    let hex = hex.trim();
    if hex.len() != 64 {
        anyhow::bail!(
            "relay fingerprint must be 64 hex characters, got {}",
            hex.len()
        );
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let engine = match cli.command {
        Command::Listen {
            relay_fingerprint,
            name,
            relay,
        } => {
            let fingerprint = parse_fingerprint(&relay_fingerprint)?;
            let mut engine = ChatEngine::connect(&name, &relay, fingerprint).await?;
            println!("Share this link: {}", engine.contact_link()?);
            println!("Waiting to be paired...");
            engine.await_pairing().await?;
            engine
        }
        Command::Connect { link, name } => ChatEngine::pair_with_link(&name, &link).await?,
    };
    println!("Paired. Epoch {}.", engine.epoch()?);
    chat_repl(engine).await
}

async fn chat_repl(mut engine: ChatEngine) -> anyhow::Result<()> {
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
    loop {
        tokio::select! {
            line = line_rx.recv() => {
                let Some(line) = line else { break };
                engine.send_message(line.as_bytes()).await?;
            }
            event = engine.next_event() => {
                match event? {
                    Event::Message(bytes) => println!("< {}", String::from_utf8_lossy(&bytes)),
                    Event::EpochAdvanced(epoch) => println!("(group state updated, epoch {epoch})"),
                    Event::ConnectionChanged(false) => println!("(connection lost — reconnecting…)"),
                    Event::ConnectionChanged(true) => println!("(reconnected)"),
                }
            }
        }
    }
    Ok(())
}
