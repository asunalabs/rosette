//! `ChatEngine` — the one object a client holds. Owns the MLS session, the
//! relay connection (including reconnect + resubscribe), own-echo and
//! foreign-duplicate suppression (OV5), and the epoch-conflict auto-retry
//! loop (OV4). Extracted from cli/ (architecture.md step 2): the CLI's REPL
//! and the FFI surface are both thin shells over this type, so the tests
//! here cover exactly what the app ships.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use anyhow::{anyhow, bail};
use base64::Engine as _;
use chatcore::{message_id_for, ChatSession, Incoming, Store};
use proto::{
    ContactLink, DeliveryMode, Endpoint, Envelope, GroupSendKind, MessageId, QueueId, RejectionCode,
};

use crate::pairing::BootstrapPayload;
use crate::relay_client::{ConnectionClosed, RelayClient};

/// What the engine surfaces to its caller. Transport details (queues,
/// epochs' conflict retries, duplicates, reconnects) never appear here —
/// they are the engine's job to absorb.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A decrypted application message from the peer.
    Message(Vec<u8>),
    /// A commit was applied; the group now stands at this epoch. UI-wise
    /// this maps to the quiet "security code changed" system line.
    EpochAdvanced(u64),
    /// The relay connection dropped (false) or came back (true). Drives the
    /// calm reconnect banner. v0.1 limitation (disclosed): both events
    /// surface only once the reconnect loop finishes, because `next_event`
    /// blocks inside it — live "offline" signaling needs the engine to pump
    /// on its own task, which arrives with the FFI dispatch layer's caller.
    ConnectionChanged(bool),
}

pub struct ChatEngine {
    display_name: String,
    session: ChatSession,
    relay: RelayClient,
    relay_addr: String,
    relay_fingerprint: [u8; 32],
    mailbox_qid: QueueId,
    mailbox_key: [u8; 32],
    /// The group inbox this conversation sends through; None until paired.
    inbox: Option<(QueueId, [u8; 32])>,
    /// Every message id this engine has applied OR authored. At-least-once
    /// delivery (T4) makes duplicates normal, and `process_incoming`
    /// hard-errors on MLS replay — so everything is checked against this
    /// set first. In-memory for v0.1; persisted with the rest of the state
    /// at the SQLCipher milestone (T5/T8).
    seen: HashSet<MessageId>,
    /// Handled pushes become events HERE, synchronously, before any await —
    /// so a `next_event` future dropped mid-ack (it races user input in a
    /// select!) can never lose an already-applied message. `next_event`
    /// pops; `commit_self_update`'s conflict drain also parks displaced
    /// events here.
    pending_events: VecDeque<Event>,
    /// SQLCipher write-through (T5/T8): every state-changing operation
    /// persists BEFORE its ack, so a crash between the two redelivers into
    /// an already-updated seen set. None = in-memory engine (tests, and any
    /// caller that hasn't attached a store yet).
    store: Option<Store>,
}

/// The non-MLS half of what `resume` needs — queue credentials and relay
/// identity. The MLS half lives in the session snapshot, the seen set in
/// its own table.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedEngine {
    display_name: String,
    relay_addr: String,
    relay_fingerprint: [u8; 32],
    mailbox_qid: QueueId,
    mailbox_key: [u8; 32],
    inbox: Option<(QueueId, [u8; 32])>,
}

fn wrap(wire_bytes: &[u8]) -> anyhow::Result<Envelope> {
    let padded = proto::pad(wire_bytes)
        .ok_or_else(|| anyhow!("message exceeds the largest padding bucket"))?;
    Ok(Envelope::new(
        message_id_for(wire_bytes),
        DeliveryMode::RelayFanout,
        padded,
    ))
}

fn is_connection_closed(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ConnectionClosed>().is_some()
}

impl ChatEngine {
    /// Connect to a relay and mint this client's mailbox — the "listen"
    /// side's starting point. Pair by handing out `contact_link()` and then
    /// `await_pairing()`.
    pub async fn connect(
        display_name: &str,
        relay_addr: &str,
        relay_fingerprint: [u8; 32],
    ) -> anyhow::Result<Self> {
        Self::connect_with_session(
            display_name,
            relay_addr,
            relay_fingerprint,
            ChatSession::new(display_name),
        )
        .await
    }

    /// `connect`, but with a caller-supplied session — the account-recovery
    /// path (issue #3): a restored device keeps its original identity
    /// (signer + credential) so peers see no identity change.
    pub async fn connect_with_session(
        display_name: &str,
        relay_addr: &str,
        relay_fingerprint: [u8; 32],
        session: ChatSession,
    ) -> anyhow::Result<Self> {
        let relay = RelayClient::connect(relay_addr, relay_fingerprint).await?;
        let (mailbox_qid, mailbox_key) = relay.create_mailbox().await?;
        relay.subscribe(vec![mailbox_qid]).await?;
        Ok(ChatEngine {
            display_name: display_name.to_string(),
            session,
            relay,
            relay_addr: relay_addr.to_string(),
            relay_fingerprint,
            mailbox_qid,
            mailbox_key,
            inbox: None,
            seen: HashSet::new(),
            pending_events: VecDeque::new(),
            store: None,
        })
    }

    /// Make this engine durable: every state change from here on writes
    /// through to the encrypted store, and `resume` can rebuild it after a
    /// process death. Attach BEFORE `await_pairing` so the pairing itself is
    /// crash-safe; after `pair_with_link`, attach immediately on return.
    // ponytail: the scan side has a millisecond window between pairing and
    // attach where a crash loses the pairing (peer keeps a dead group) —
    // recovery is "re-scan the QR". Constructor-injected stores if that
    // window ever matters.
    pub fn attach_store(&mut self, store: Store) -> anyhow::Result<()> {
        self.store = Some(store);
        let all_seen: Vec<MessageId> = self.seen.iter().copied().collect();
        self.persist(&all_seen)
    }

    /// Rebuild a durable engine from its store and reconnect. The relay
    /// replays the unacked backlog on subscribe (T4); the persisted seen
    /// set suppresses everything already applied before the crash.
    pub async fn resume(store: Store) -> anyhow::Result<Self> {
        let rec: PersistedEngine = bincode::deserialize(
            &store
                .get("engine")?
                .ok_or_else(|| anyhow!("store holds no engine state — nothing to resume"))?,
        )?;
        let session = ChatSession::restore(
            &store
                .get("session")?
                .ok_or_else(|| anyhow!("store holds no session state — nothing to resume"))?,
        )?;
        let seen = store
            .seen_ids()?
            .into_iter()
            .map(|v| MessageId::try_from(v.as_slice()).map_err(|_| anyhow!("corrupt seen-id row")))
            .collect::<anyhow::Result<HashSet<_>>>()?;
        let relay = RelayClient::connect(&rec.relay_addr, rec.relay_fingerprint).await?;
        relay.subscribe(vec![rec.mailbox_qid]).await?;
        Ok(ChatEngine {
            display_name: rec.display_name,
            session,
            relay,
            relay_addr: rec.relay_addr,
            relay_fingerprint: rec.relay_fingerprint,
            mailbox_qid: rec.mailbox_qid,
            mailbox_key: rec.mailbox_key,
            inbox: rec.inbox,
            seen,
            pending_events: VecDeque::new(),
            store: Some(store),
        })
    }

    /// Write-through: engine record + full session snapshot + new seen ids,
    /// one transaction. No-op for in-memory engines.
    fn persist(&mut self, new_seen: &[MessageId]) -> anyhow::Result<()> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let rec = bincode::serialize(&PersistedEngine {
            display_name: self.display_name.clone(),
            relay_addr: self.relay_addr.clone(),
            relay_fingerprint: self.relay_fingerprint,
            mailbox_qid: self.mailbox_qid,
            mailbox_key: self.mailbox_key,
            inbox: self.inbox,
        })?;
        let session = self.session.snapshot()?;
        let seen: Vec<&[u8]> = new_seen.iter().map(|id| id.as_slice()).collect();
        store.commit(&[("engine", &rec), ("session", &session)], &seen)?;
        Ok(())
    }

    /// The base64 string a QR code encodes: a fresh KeyPackage plus this
    /// client's bootstrap mailbox endpoint.
    pub fn contact_link(&mut self) -> anyhow::Result<String> {
        let bundle = self.session.generate_key_package()?;
        let link = chatcore::pairing::build_contact_link(
            bundle.key_package(),
            &self.relay_addr,
            self.relay_fingerprint,
            self.mailbox_qid,
            self.mailbox_key,
        )?;
        Ok(base64::engine::general_purpose::STANDARD.encode(link.to_bytes()))
    }

    /// The "scan" side: consume a contact link, connect to its relay, found
    /// the 2-member group, create the group inbox, and deliver the bootstrap
    /// payload to the peer's mailbox. Returns a fully paired engine.
    pub async fn pair_with_link(display_name: &str, link_b64: &str) -> anyhow::Result<Self> {
        Self::pair_with_link_using(display_name, link_b64, None).await
    }

    /// `pair_with_link` with an optional restored session (issue #3) — same
    /// reason as [`Self::connect_with_session`].
    pub async fn pair_with_link_using(
        display_name: &str,
        link_b64: &str,
        session: Option<ChatSession>,
    ) -> anyhow::Result<Self> {
        let link_bytes = base64::engine::general_purpose::STANDARD.decode(link_b64.trim())?;
        let link = ContactLink::from_bytes(&link_bytes)?;
        let Endpoint {
            relay_addr,
            relay_fingerprint,
            queue_id: peer_mailbox,
            send_key: peer_send_key,
        } = link.primary_endpoint().clone();

        let session = session.unwrap_or_else(|| ChatSession::new(display_name));
        let mut engine =
            Self::connect_with_session(display_name, &relay_addr, relay_fingerprint, session)
                .await?;
        let peer_kp = chatcore::pairing::key_package_from_link(&link, engine.session.provider())?;
        engine.session.create_group()?;
        let welcome_wire = engine.session.add_members(&[peer_kp])?;
        let tree_wire = engine.session.export_ratchet_tree()?;

        let (inbox_qid, inbox_key) = engine
            .relay
            .create_group_inbox(1, vec![engine.mailbox_qid, peer_mailbox])
            .await?;
        let payload = BootstrapPayload {
            welcome_wire,
            tree_wire,
            inbox_qid,
            inbox_key,
        };
        let envelope = wrap(&bincode::serialize(&payload)?)?;
        engine
            .relay
            .send_to_mailbox(peer_mailbox, &peer_send_key, envelope)
            .await?;
        engine.inbox = Some((inbox_qid, inbox_key));
        Ok(engine)
    }

    /// The "listen" side's second half: wait for the scanner's bootstrap
    /// payload, join the group, adopt the group inbox credentials.
    /// Cancellation-safe past the join: all pairing state is set
    /// synchronously after the push arrives, before the ack await — a
    /// dropped future at worst skips the ack, which redelivery + the
    /// seen-set absorb. Callers racing this in a select! should re-check
    /// `is_paired()`.
    pub async fn await_pairing(&mut self) -> anyhow::Result<()> {
        loop {
            let Some((qid, envelope)) = self.relay.push_rx.recv().await else {
                self.reconnect().await?;
                continue;
            };
            let payload: BootstrapPayload = bincode::deserialize(&envelope.padded_ciphertext)?;
            self.session
                .join_from_welcome(&payload.welcome_wire, &payload.tree_wire)?;
            self.seen.insert(envelope.message_id);
            self.inbox = Some((payload.inbox_qid, payload.inbox_key));
            self.persist(&[envelope.message_id])?;
            self.try_ack(qid, envelope.message_id).await?;
            return Ok(());
        }
    }

    /// True once this engine has a conversation to send into (either side
    /// of the pairing handshake has completed).
    pub fn is_paired(&self) -> bool {
        self.inbox.is_some()
    }

    /// The peer's display name, read from their MLS credential. Decoration
    /// only — never an identifier. None until paired.
    pub fn peer_name(&self) -> Option<String> {
        let names = self.session.member_names().ok()?;
        names.into_iter().find(|n| *n != self.display_name)
    }

    pub fn epoch(&self) -> anyhow::Result<u64> {
        Ok(self.session.epoch()?)
    }

    /// DT6: the safety number both peers compare in the verify ceremony. See
    /// `ChatSession::safety_number` — bound to the MLS signature keys, so a MITM
    /// diverges. Errors until paired (no group yet).
    pub fn safety_number(&self) -> anyhow::Result<String> {
        Ok(self.session.safety_number()?)
    }

    /// Send an application message to the conversation. Reconnects and
    /// retries on connection loss; the stable message id (a hash of the wire
    /// bytes) means an ambiguous resend at worst produces a duplicate the
    /// receivers' seen-sets absorb.
    pub async fn send_message(&mut self, plaintext: &[u8]) -> anyhow::Result<()> {
        let (inbox_qid, inbox_key) = self.inbox.ok_or_else(|| anyhow!("not paired yet"))?;
        let wire = self.session.encrypt_application(plaintext)?;
        let envelope = wrap(&wire)?;
        let message_id = envelope.message_id;
        // Persist BEFORE the send: `encrypt_application` already advanced
        // the sender ratchet, and the seen entry pre-suppresses the own
        // fan-out echo. A crash after persist but before send at worst
        // skips a ratchet generation, which receivers tolerate; the reverse
        // order would let a redelivered own echo hit the session as an
        // undecryptable foreign message after resume.
        self.seen.insert(message_id);
        self.persist(&[message_id])?;
        loop {
            match self
                .relay
                .send_to_group_inbox(
                    inbox_qid,
                    &inbox_key,
                    GroupSendKind::Application,
                    envelope.clone(),
                )
                .await
            {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(code)) => bail!("send rejected: {code:?}"),
                Err(e) if is_connection_closed(&e) => self.reconnect().await?,
                Err(e) => return Err(e),
            }
        }
    }

    /// Commit a self-update, auto-resolving epoch conflicts (OV4): on
    /// `EpochConflict` the locally built commit is discarded, pushes are
    /// drained until the WINNING commit arrives and merges, then the commit
    /// is rebuilt against the new epoch and resent. Application messages
    /// displaced by the drain are buffered for `next_event`, never lost.
    /// Returns the epoch the group stands at once this commit lands.
    pub async fn commit_self_update(&mut self) -> anyhow::Result<u64> {
        let (inbox_qid, inbox_key) = self.inbox.ok_or_else(|| anyhow!("not paired yet"))?;
        loop {
            let epoch = self.session.epoch()?;
            let wire = self.session.self_update()?;
            let envelope = wrap(&wire)?;
            let send_result = loop {
                match self
                    .relay
                    .send_to_group_inbox(
                        inbox_qid,
                        &inbox_key,
                        GroupSendKind::Commit { epoch },
                        envelope.clone(),
                    )
                    .await
                {
                    Ok(result) => break result,
                    Err(e) if is_connection_closed(&e) => self.reconnect().await?,
                    Err(e) => return Err(e),
                }
            };
            match send_result {
                Ok(()) => {
                    self.seen.insert(envelope.message_id);
                    self.session.merge_pending_commit()?;
                    // ponytail: a crash between the relay accepting this
                    // commit and this persist strands the group one epoch
                    // ahead of the stored state — an inflight-commit marker
                    // plus merge-own-echo-on-resume logic closes it; add
                    // when anything beyond tests actually rotates keys.
                    self.persist(&[envelope.message_id])?;
                    return Ok(self.session.epoch()?);
                }
                Err(RejectionCode::EpochConflict) => {
                    // Lost the epoch race. The WINNING commit is already in
                    // flight to this client's mailbox (the relay fanned it
                    // out before rejecting this one), so: discard the stale
                    // commit, pump pushes until the winner merges (the
                    // session epoch moving is the signal), rebuild, resend.
                    // Everything pumped — including the winner's
                    // EpochAdvanced — lands in pending_events for the
                    // caller; nothing is swallowed.
                    self.session.discard_pending_commit()?;
                    let stale_epoch = epoch;
                    while self.session.epoch()? == stale_epoch {
                        self.pump_one().await?;
                    }
                }
                Err(code) => bail!("commit rejected: {code:?}"),
            }
        }
    }

    /// The caller's event loop: the next decrypted message or epoch advance.
    /// Duplicates, own echoes, acks, and reconnects are handled internally
    /// and never surface. Cancellation-safe: dropping this future mid-poll
    /// never loses an event (see `pending_events`).
    pub async fn next_event(&mut self) -> anyhow::Result<Event> {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Ok(event);
            }
            self.pump_one().await?;
        }
    }

    /// Receive and handle exactly one push (reconnecting if the connection
    /// is gone). Any event it produces is queued on `pending_events`
    /// synchronously — before the ack await — so cancellation between the
    /// two can't lose it.
    async fn pump_one(&mut self) -> anyhow::Result<()> {
        match self.relay.push_rx.recv().await {
            None => {
                self.pending_events
                    .push_back(Event::ConnectionChanged(false));
                self.reconnect().await?;
                self.pending_events
                    .push_back(Event::ConnectionChanged(true));
                Ok(())
            }
            Some((qid, envelope)) => {
                let id = envelope.message_id;
                if self.seen.insert(id) {
                    // First sight: apply to the MLS session and queue the
                    // event. Duplicates and own fan-out echoes fail the
                    // insert and skip straight to the (re-)ack — they must
                    // never reach the session (replay hard-errors).
                    let event = match self.session.process_incoming(&envelope.padded_ciphertext)? {
                        Incoming::Application(bytes) => Event::Message(bytes),
                        Incoming::CommitApplied => Event::EpochAdvanced(self.session.epoch()?),
                    };
                    self.pending_events.push_back(event);
                    // Persist before the ack (A9): a crash between the two
                    // redelivers into the already-updated seen set instead
                    // of replaying into MLS.
                    self.persist(&[id])?;
                }
                self.try_ack(qid, id).await
            }
        }
    }

    /// Ack, tolerating a dead connection: the message is in `seen`, so the
    /// redelivery that follows the reconnect gets suppressed and re-acked
    /// there — the ack is not worth failing the caller's event over.
    async fn try_ack(&mut self, qid: QueueId, id: MessageId) -> anyhow::Result<()> {
        match self.relay.ack(qid, id).await {
            Ok(()) => Ok(()),
            Err(e) if is_connection_closed(&e) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Reconnect and re-subscribe the full queue set. The relay then replays
    /// the unacked backlog (T4); anything already applied is suppressed by
    /// the seen-set, so the caller observes no loss and no duplicates.
    async fn reconnect(&mut self) -> anyhow::Result<()> {
        const ATTEMPTS: u32 = 40;
        const DELAY: Duration = Duration::from_millis(250);
        for attempt in 1..=ATTEMPTS {
            match RelayClient::connect(&self.relay_addr, self.relay_fingerprint).await {
                Ok(relay) => {
                    relay.subscribe(vec![self.mailbox_qid]).await?;
                    self.relay = relay;
                    return Ok(());
                }
                Err(_) if attempt < ATTEMPTS => tokio::time::sleep(DELAY).await,
                Err(e) => return Err(e.context("reconnect: relay unreachable")),
            }
        }
        unreachable!("loop returns on success or final error")
    }
}
