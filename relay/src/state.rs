//! Transport-agnostic relay state: queue storage, DS epoch enforcement, and
//! resource limits. Kept separate from net.rs so the DS conflict-resolution
//! logic (the property amendment A1 exists to prove) is unit-testable
//! without a TCP stack.
//!
//! Persistence (T9, eng-review OV1): `RelayState::open` backs every durable
//! mutation (queue creation, stored envelopes, acks, epoch advances) with
//! write-through SQLite, so a killed relay restarts with its queues, unacked
//! backlogs, and DS epochs intact — contact links and mid-flight
//! conversations survive. Ephemeral state (subscribers, PoW challenges,
//! rate-limit windows) is per-connection/abuse bookkeeping and deliberately
//! resets. `RelayState::new` stays memory-only for tests. A persistence
//! failure panics: a relay that cannot durably store what it acknowledges
//! must crash loudly, not degrade silently.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::VerifyingKey;
use proto::attestation::AttestationToken;

use proto::{
    limits, AuthTag, ClientMessage, Envelope, GroupSendKind, MessageId, PowChallenge, PowSolution,
    QueueId, RejectionCode, ServerMessage,
};
use rand::RngCore;
use rusqlite::{params, Connection};
use tokio::sync::mpsc;

/// Just a sizing hint for pre-allocating subscriber channels — not a hard cap.
pub const RELAY_QUEUE_CAPACITY_HINT: usize = 64;

pub type PushSender = mpsc::UnboundedSender<(QueueId, MessageId, Envelope)>;

enum QueueKind {
    Mailbox {
        pending: VecDeque<(MessageId, Envelope)>,
    },
    /// v0.1 scope cut (disclosed): the send key is static from creation, not
    /// derived per-epoch from the MLS exporter secret and not updated on
    /// membership changes. Ordering correctness (the property under test)
    /// lives entirely in `epoch`, not in the key. Revisit when dynamic
    /// Add/Remove commits land (design doc Next Steps #5).
    GroupInbox {
        epoch: u64,
        fan_out_to: Vec<QueueId>,
    },
}

struct QueueEntry {
    send_key: [u8; 32],
    kind: QueueKind,
    sent_this_minute: u32,
    window_started: Instant,
}

impl QueueEntry {
    fn check_and_bump_rate_limit(&mut self) -> Result<(), RejectionCode> {
        if self.window_started.elapsed() >= Duration::from_secs(60) {
            self.window_started = Instant::now();
            self.sent_this_minute = 0;
        }
        if self.sent_this_minute >= limits::RATE_LIMIT_PER_QUEUE_PER_MINUTE {
            return Err(RejectionCode::RateLimited);
        }
        self.sent_this_minute += 1;
        Ok(())
    }
}

#[derive(Default)]
struct Inner {
    queues: HashMap<QueueId, QueueEntry>,
    subscribers: HashMap<QueueId, Vec<PushSender>>,
    outstanding_challenges: HashMap<[u8; 32], u8>,
    /// Insertion order of `outstanding_challenges` keys, for FIFO eviction at
    /// the cap. May hold keys already consumed/removed from the map; eviction
    /// tolerates that (the remove is a no-op). Bounded by the same cap.
    challenge_order: VecDeque<[u8; 32]>,
    storage_bytes_used: u64,
    /// Write-through persistence (T9). None = memory-only (tests).
    db: Option<Connection>,
    /// T27: the directory's public key, baked in at deploy time. `None` = the
    /// attestation gate is off (queue creation needs only PoW), so an
    /// un-configured relay behaves exactly as before T27.
    attestation_key: Option<VerifyingKey>,
    /// T27: spent token nonces → their expiry (unix secs). Replay protection.
    /// Pruned by EXPIRY, never by count: a count-evicted-but-unexpired nonce
    /// would be replayable, so eviction must key on the token's own lifetime.
    spent_tokens: HashMap<[u8; 16], i64>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct RelayState {
    inner: Mutex<Inner>,
}

impl Default for RelayState {
    fn default() -> Self {
        RelayState {
            inner: Mutex::new(Inner::default()),
        }
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS queues (
    queue_id BLOB PRIMARY KEY,
    send_key BLOB NOT NULL,
    -- NULL for a mailbox; the current DS epoch for a group inbox.
    epoch    INTEGER
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS roster (
    inbox_id     BLOB NOT NULL,
    ord          INTEGER NOT NULL,
    member_queue BLOB NOT NULL,
    PRIMARY KEY (inbox_id, ord)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS pending (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_id   BLOB NOT NULL,
    message_id BLOB NOT NULL,
    envelope   BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS pending_by_queue ON pending(queue_id);
";

fn blob32(bytes: Vec<u8>) -> rusqlite::Result<[u8; 32]> {
    bytes.try_into().map_err(|_| rusqlite::Error::InvalidQuery)
}

impl RelayState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open (or create) the persistent relay state at `path`. Every durable
    /// mutation from here on is written through before it takes effect in
    /// memory, so dropping the process at ANY point — kill -9 included —
    /// loses at most requests that were never acknowledged.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open(path)?;
        // WAL keeps single-writer inserts cheap; the relay is the only writer.
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(SCHEMA)?;

        let mut queues: HashMap<QueueId, QueueEntry> = HashMap::new();
        {
            let mut stmt = db.prepare("SELECT queue_id, send_key, epoch FROM queues")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    blob32(row.get::<_, Vec<u8>>(0)?)?,
                    blob32(row.get::<_, Vec<u8>>(1)?)?,
                    row.get::<_, Option<u64>>(2)?,
                ))
            })?;
            for row in rows {
                let (queue_id, send_key, epoch) = row?;
                let kind = match epoch {
                    None => QueueKind::Mailbox {
                        pending: VecDeque::new(),
                    },
                    Some(epoch) => QueueKind::GroupInbox {
                        epoch,
                        fan_out_to: Vec::new(),
                    },
                };
                queues.insert(
                    queue_id,
                    QueueEntry {
                        send_key,
                        kind,
                        sent_this_minute: 0,
                        window_started: Instant::now(),
                    },
                );
            }
        }
        {
            let mut stmt =
                db.prepare("SELECT inbox_id, member_queue FROM roster ORDER BY inbox_id, ord")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    blob32(row.get::<_, Vec<u8>>(0)?)?,
                    blob32(row.get::<_, Vec<u8>>(1)?)?,
                ))
            })?;
            for row in rows {
                let (inbox_id, member) = row?;
                if let Some(QueueEntry {
                    kind: QueueKind::GroupInbox { fan_out_to, .. },
                    ..
                }) = queues.get_mut(&inbox_id)
                {
                    fan_out_to.push(member);
                }
            }
        }
        let mut storage_bytes_used = 0u64;
        {
            let mut stmt = db.prepare("SELECT queue_id, envelope FROM pending ORDER BY seq")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    blob32(row.get::<_, Vec<u8>>(0)?)?,
                    row.get::<_, Vec<u8>>(1)?,
                ))
            })?;
            for row in rows {
                let (queue_id, envelope_bytes) = row?;
                let envelope: Envelope = proto::decode(&envelope_bytes)
                    .map_err(|e| anyhow::anyhow!("corrupt stored envelope: {e}"))?;
                storage_bytes_used += envelope.padded_ciphertext.len() as u64;
                if let Some(QueueEntry {
                    kind: QueueKind::Mailbox { pending },
                    ..
                }) = queues.get_mut(&queue_id)
                {
                    pending.push_back((envelope.message_id, envelope));
                }
            }
        }

        Ok(RelayState {
            inner: Mutex::new(Inner {
                queues,
                storage_bytes_used,
                db: Some(db),
                ..Inner::default()
            }),
        })
    }

    pub fn issue_pow_challenge(&self) -> PowChallenge {
        let challenge = PowChallenge::generate(limits::QUEUE_CREATION_POW_DIFFICULTY);
        let mut inner = self.inner.lock().unwrap();
        // Bounded: FIFO-evict the oldest unsolved challenge(s) so a client that
        // requests challenges without ever solving them can't OOM the relay.
        while inner.challenge_order.len() >= limits::MAX_OUTSTANDING_POW_CHALLENGES {
            match inner.challenge_order.pop_front() {
                Some(oldest) => {
                    inner.outstanding_challenges.remove(&oldest);
                }
                None => break,
            }
        }
        inner
            .outstanding_challenges
            .insert(challenge.challenge, challenge.difficulty);
        inner.challenge_order.push_back(challenge.challenge);
        challenge
    }

    /// T27: bake in the directory's public key (from relay config). Once set,
    /// queue creation requires a valid attestation token.
    pub fn set_attestation_key(&self, key: Option<VerifyingKey>) {
        self.inner.lock().unwrap().attestation_key = key;
    }

    /// T27: enforce the attestation gate. Off (Ok) when no key is configured.
    /// When on, the token must be present, correctly signed, unexpired, and
    /// unspent; a valid token's nonce is then recorded as spent (single-use).
    /// Purely local — no directory call — so T21's crash-isolation holds.
    fn consume_attestation(
        &self,
        inner: &mut Inner,
        token: Option<&AttestationToken>,
    ) -> Result<(), RejectionCode> {
        let Some(key) = inner.attestation_key else {
            return Ok(()); // gate off
        };
        let token = token.ok_or(RejectionCode::InvalidAttestation)?;
        let now = now_unix();
        if token.is_expired(now) || !token.signature_valid(&key) {
            return Err(RejectionCode::InvalidAttestation);
        }
        // Prune expired nonces before checking/inserting, so the set stays
        // bounded by the number of live (unexpired) tokens.
        inner.spent_tokens.retain(|_, &mut exp| exp > now);
        if inner.spent_tokens.contains_key(&token.nonce) {
            return Err(RejectionCode::InvalidAttestation); // replay
        }
        inner.spent_tokens.insert(token.nonce, token.expires_at);
        Ok(())
    }

    fn consume_pow(&self, inner: &mut Inner, solution: &PowSolution) -> Result<(), RejectionCode> {
        let difficulty = inner
            .outstanding_challenges
            .remove(&solution.challenge)
            .ok_or(RejectionCode::InvalidProofOfWork)?;
        let challenge = PowChallenge {
            challenge: solution.challenge,
            difficulty,
        };
        if challenge.verify(solution) {
            Ok(())
        } else {
            Err(RejectionCode::InvalidProofOfWork)
        }
    }

    fn fresh_queue_id(inner: &Inner) -> QueueId {
        loop {
            let mut id = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut id);
            if !inner.queues.contains_key(&id) {
                return id;
            }
        }
    }

    pub fn create_mailbox(
        &self,
        solution: PowSolution,
        attestation: Option<AttestationToken>,
    ) -> Result<(QueueId, [u8; 32]), RejectionCode> {
        let mut inner = self.inner.lock().unwrap();
        self.consume_attestation(&mut inner, attestation.as_ref())?;
        self.consume_pow(&mut inner, &solution)?;
        let queue_id = Self::fresh_queue_id(&inner);
        let mut send_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut send_key);
        if let Some(db) = &inner.db {
            db.execute(
                "INSERT INTO queues (queue_id, send_key, epoch) VALUES (?1, ?2, NULL)",
                params![queue_id.as_slice(), send_key.as_slice()],
            )
            .expect("relay must not hand out a queue it cannot persist");
        }
        inner.queues.insert(
            queue_id,
            QueueEntry {
                send_key,
                kind: QueueKind::Mailbox {
                    pending: VecDeque::new(),
                },
                sent_this_minute: 0,
                window_started: Instant::now(),
            },
        );
        Ok((queue_id, send_key))
    }

    pub fn create_group_inbox(
        &self,
        solution: PowSolution,
        initial_epoch: u64,
        fan_out_to: Vec<QueueId>,
        attestation: Option<AttestationToken>,
    ) -> Result<(QueueId, [u8; 32]), RejectionCode> {
        let mut inner = self.inner.lock().unwrap();
        self.consume_attestation(&mut inner, attestation.as_ref())?;
        self.consume_pow(&mut inner, &solution)?;
        let queue_id = Self::fresh_queue_id(&inner);
        let mut send_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut send_key);
        if let Some(db) = inner.db.as_mut() {
            // One transaction: an inbox must never exist without its roster.
            let tx = db
                .transaction()
                .expect("starting a persistence transaction never fails");
            tx.execute(
                "INSERT INTO queues (queue_id, send_key, epoch) VALUES (?1, ?2, ?3)",
                params![queue_id.as_slice(), send_key.as_slice(), initial_epoch],
            )
            .expect("relay must not hand out a queue it cannot persist");
            for (ord, member) in fan_out_to.iter().enumerate() {
                tx.execute(
                    "INSERT INTO roster (inbox_id, ord, member_queue) VALUES (?1, ?2, ?3)",
                    params![queue_id.as_slice(), ord as i64, member.as_slice()],
                )
                .expect("relay must not hand out a queue it cannot persist");
            }
            tx.commit()
                .expect("relay must not hand out a queue it cannot persist");
        }
        inner.queues.insert(
            queue_id,
            QueueEntry {
                send_key,
                kind: QueueKind::GroupInbox {
                    epoch: initial_epoch,
                    fan_out_to,
                },
                sent_this_minute: 0,
                window_started: Instant::now(),
            },
        );
        Ok((queue_id, send_key))
    }

    /// Registers `tx` to receive every future push for `queue_ids`. Idempotent
    /// per connection: a re-subscribe from the same connection (same channel)
    /// does NOT stack a second sender for a queue — that would double-deliver
    /// every push. Also prunes senders whose connection has since closed.
    ///
    /// T4 (eng-review OV3): after registering, the queue's unacked backlog is
    /// drained to the subscribing connection. Delivery is at-least-once — a
    /// message leaves `pending` only via `ack`, so a client that received a
    /// push but crashed before acking sees it again on reconnect. Duplicate
    /// suppression is the client's job (engine seen-set, OV5).
    pub fn subscribe(&self, queue_ids: &[QueueId], tx: PushSender) {
        let mut inner = self.inner.lock().unwrap();
        for qid in queue_ids {
            let subs = inner.subscribers.entry(*qid).or_default();
            subs.retain(|existing| !existing.is_closed() && !existing.same_channel(&tx));
            subs.push(tx.clone());
        }
        for qid in queue_ids {
            if let Some(QueueEntry {
                kind: QueueKind::Mailbox { pending },
                ..
            }) = inner.queues.get(qid)
            {
                for (message_id, envelope) in pending {
                    if tx.send((*qid, *message_id, envelope.clone())).is_err() {
                        return; // connection already gone; senders get pruned on the next send
                    }
                }
            }
        }
    }

    fn push_and_notify(
        inner: &mut Inner,
        queue_id: QueueId,
        message_id: MessageId,
        envelope: Envelope,
    ) {
        let mut stored = false;
        if let Some(entry) = inner.queues.get_mut(&queue_id) {
            if let QueueKind::Mailbox { pending } = &mut entry.kind {
                pending.push_back((message_id, envelope.clone()));
                stored = true;
            }
        }
        if stored {
            inner.storage_bytes_used += envelope.padded_ciphertext.len() as u64;
        }
        // Prune dead subscribers as we notify: a send that fails means the
        // receiving connection is gone, so drop its sender instead of leaking
        // it forever (every reconnect would otherwise add a permanent entry).
        if let Some(subs) = inner.subscribers.get_mut(&queue_id) {
            subs.retain(|tx| tx.send((queue_id, message_id, envelope.clone())).is_ok());
        }
    }

    /// v0.1 stores one copy per recipient mailbox (amendment A11's
    /// store-once + refcount is a disclosed later cut — see relay/src/lib.rs
    /// module doc), so a send that will land in `targets` mailboxes must be
    /// checked against the full multiplied cost before any of it is written.
    fn would_exceed_storage_bound(inner: &Inner, envelope_len: usize, targets: usize) -> bool {
        let incoming = envelope_len as u64 * targets as u64;
        inner.storage_bytes_used.saturating_add(incoming) > limits::MAX_STORAGE_BYTES
    }

    pub fn send_to_mailbox(
        &self,
        queue_id: QueueId,
        auth_tag: AuthTag,
        envelope: Envelope,
    ) -> Result<(), RejectionCode> {
        if envelope.padded_ciphertext.len() > limits::MAX_MESSAGE_SIZE {
            return Err(RejectionCode::MessageTooLarge);
        }
        let mut inner = self.inner.lock().unwrap();
        let message_id = envelope.message_id;
        {
            let entry = inner
                .queues
                .get_mut(&queue_id)
                .ok_or(RejectionCode::QueueNotFound)?;
            if !matches!(entry.kind, QueueKind::Mailbox { .. }) {
                return Err(RejectionCode::QueueNotFound);
            }
            if !proto::verify_tag(&entry.send_key, &queue_id, &envelope, &auth_tag) {
                return Err(RejectionCode::Unauthorized);
            }
            if let QueueKind::Mailbox { pending } = &entry.kind {
                if pending.len() >= limits::MAX_QUEUE_DEPTH {
                    return Err(RejectionCode::QueueFull);
                }
            }
            entry.check_and_bump_rate_limit()?;
        }
        if Self::would_exceed_storage_bound(&inner, envelope.padded_ciphertext.len(), 1) {
            return Err(RejectionCode::StorageBoundExceeded);
        }
        // Durable before acknowledged: the Ok reply must never outlive a
        // crash that loses the message.
        if let Some(db) = &inner.db {
            db.execute(
                "INSERT INTO pending (queue_id, message_id, envelope) VALUES (?1, ?2, ?3)",
                params![
                    queue_id.as_slice(),
                    message_id.as_slice(),
                    proto::encode(&envelope)
                ],
            )
            .expect("relay must not acknowledge a message it cannot persist");
        }
        Self::push_and_notify(&mut inner, queue_id, message_id, envelope);
        Ok(())
    }

    /// The DS ordering rule (amendment A1): a Commit is accepted only if its
    /// `epoch` matches the queue's current epoch, which then advances by one.
    /// Any other commit racing for the same epoch — including one that
    /// arrives a nanosecond later — necessarily sees the bumped epoch and is
    /// rejected with `EpochConflict`. This holds under real concurrency
    /// because the whole check-and-bump happens under one mutex acquisition;
    /// there is no window where two callers can both observe the pre-bump
    /// epoch.
    pub fn send_to_group_inbox(
        &self,
        queue_id: QueueId,
        kind: GroupSendKind,
        auth_tag: AuthTag,
        envelope: Envelope,
    ) -> Result<(), RejectionCode> {
        if envelope.padded_ciphertext.len() > limits::MAX_MESSAGE_SIZE {
            return Err(RejectionCode::MessageTooLarge);
        }
        let mut inner = self.inner.lock().unwrap();
        let message_id = envelope.message_id;

        // Peek-only pass: validate everything (auth, rate limit, epoch match,
        // storage bound) BEFORE mutating any state. This keeps the epoch bump
        // and the fan-out atomic together — a commit that fails the storage
        // check must not have already "won" the epoch with nothing delivered.
        let fan_out_to = {
            let entry = inner
                .queues
                .get_mut(&queue_id)
                .ok_or(RejectionCode::GroupInboxNotFound)?;
            if !proto::verify_tag(&entry.send_key, &queue_id, &envelope, &auth_tag) {
                return Err(RejectionCode::Unauthorized);
            }
            let (current_epoch, roster) = match &entry.kind {
                QueueKind::GroupInbox { epoch, fan_out_to } => (*epoch, fan_out_to.clone()),
                QueueKind::Mailbox { .. } => return Err(RejectionCode::GroupInboxNotFound),
            };
            if let GroupSendKind::Commit {
                epoch: target_epoch,
            } = kind
            {
                if target_epoch != current_epoch {
                    return Err(RejectionCode::EpochConflict);
                }
            }
            roster
        };
        if Self::would_exceed_storage_bound(
            &inner,
            envelope.padded_ciphertext.len(),
            fan_out_to.len(),
        ) {
            return Err(RejectionCode::StorageBoundExceeded);
        }
        {
            let entry = inner.queues.get_mut(&queue_id).expect("checked above");
            entry.check_and_bump_rate_limit()?;
        }
        // Durable before acknowledged, in ONE transaction: the epoch advance
        // and its fan-out land together or not at all. A crash in between
        // would otherwise burn the epoch with the winning commit lost — the
        // loser would then drain forever waiting for a winner that never
        // arrives.
        let stored_targets: Vec<QueueId> = fan_out_to
            .iter()
            .copied()
            .filter(|member| {
                matches!(
                    inner.queues.get(member),
                    Some(QueueEntry {
                        kind: QueueKind::Mailbox { .. },
                        ..
                    })
                )
            })
            .collect();
        let is_commit = matches!(kind, GroupSendKind::Commit { .. });
        if let Some(db) = inner.db.as_mut() {
            let tx = db
                .transaction()
                .expect("starting a persistence transaction never fails");
            if is_commit {
                tx.execute(
                    "UPDATE queues SET epoch = epoch + 1 WHERE queue_id = ?1",
                    params![queue_id.as_slice()],
                )
                .expect("relay must not acknowledge a commit it cannot persist");
            }
            let envelope_bytes = proto::encode(&envelope);
            for member in &stored_targets {
                tx.execute(
                    "INSERT INTO pending (queue_id, message_id, envelope) VALUES (?1, ?2, ?3)",
                    params![member.as_slice(), message_id.as_slice(), envelope_bytes],
                )
                .expect("relay must not acknowledge a message it cannot persist");
            }
            tx.commit()
                .expect("relay must not acknowledge a message it cannot persist");
        }
        {
            let entry = inner.queues.get_mut(&queue_id).expect("checked above");
            if let (GroupSendKind::Commit { .. }, QueueKind::GroupInbox { epoch, .. }) =
                (kind, &mut entry.kind)
            {
                *epoch += 1;
            }
        }
        for member_queue in fan_out_to {
            Self::push_and_notify(&mut inner, member_queue, message_id, envelope.clone());
        }
        Ok(())
    }

    pub fn ack(&self, queue_id: QueueId, message_id: MessageId) {
        // Delete-on-ack (amendment A3, T4): the ack removes the message from
        // the recipient's mailbox and frees its storage, ending its
        // redelivery-on-resubscribe lifetime. Per-recipient semantics fall
        // out of v0.1's store-one-copy-per-mailbox model (A11's store-once +
        // refcount is a disclosed later cut). TTL-based expiry for messages
        // never acked at all is still missing — that's relay persistence
        // milestone territory (architecture.md step 5).
        let mut inner = self.inner.lock().unwrap();
        if let Some(db) = &inner.db {
            db.execute(
                "DELETE FROM pending WHERE queue_id = ?1 AND message_id = ?2",
                params![queue_id.as_slice(), message_id.as_slice()],
            )
            .expect("relay must not forget an ack it cannot persist");
        }
        let mut freed = 0u64;
        if let Some(entry) = inner.queues.get_mut(&queue_id) {
            if let QueueKind::Mailbox { pending } = &mut entry.kind {
                let before = pending.len();
                pending.retain(|(mid, env)| {
                    let keep = *mid != message_id;
                    if !keep {
                        freed += env.padded_ciphertext.len() as u64;
                    }
                    keep
                });
                debug_assert!(pending.len() <= before);
            }
        }
        inner.storage_bytes_used = inner.storage_bytes_used.saturating_sub(freed);
    }

    /// Dispatch a single wire message. Kept here (not in net.rs) so the
    /// framing layer stays a thin adapter and every branch is reachable from
    /// unit tests without opening a socket.
    pub fn handle(&self, msg: ClientMessage, push_tx: Option<PushSender>) -> ServerMessage {
        match msg {
            ClientMessage::RequestPowChallenge => {
                ServerMessage::PowChallenge(self.issue_pow_challenge())
            }
            ClientMessage::CreateMailbox {
                solution,
                attestation,
            } => match self.create_mailbox(solution, attestation) {
                Ok((queue_id, send_key)) => ServerMessage::QueueCreated { queue_id, send_key },
                Err(e) => ServerMessage::Error(e),
            },
            ClientMessage::CreateGroupInbox {
                solution,
                initial_epoch,
                fan_out_to,
                attestation,
            } => match self.create_group_inbox(solution, initial_epoch, fan_out_to, attestation) {
                Ok((queue_id, send_key)) => ServerMessage::QueueCreated { queue_id, send_key },
                Err(e) => ServerMessage::Error(e),
            },
            ClientMessage::SendToMailbox {
                queue_id,
                auth_tag,
                envelope,
            } => match self.send_to_mailbox(queue_id, auth_tag, envelope) {
                Ok(()) => ServerMessage::Ok,
                Err(e) => ServerMessage::Error(e),
            },
            ClientMessage::SendToGroupInbox {
                queue_id,
                kind,
                auth_tag,
                envelope,
            } => match self.send_to_group_inbox(queue_id, kind, auth_tag, envelope) {
                Ok(()) => ServerMessage::Ok,
                Err(e) => ServerMessage::Error(e),
            },
            ClientMessage::Subscribe { queue_ids } => {
                if let Some(tx) = push_tx {
                    self.subscribe(&queue_ids, tx);
                }
                ServerMessage::Ok
            }
            ClientMessage::Ack {
                queue_id,
                message_id,
            } => {
                self.ack(queue_id, message_id);
                ServerMessage::Ok
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::DeliveryMode;

    fn solved_pow(state: &RelayState) -> PowSolution {
        state.issue_pow_challenge().solve()
    }

    #[test]
    fn no_attestation_key_means_the_gate_is_off() {
        // The default relay has no key, so a `None` token still creates a
        // mailbox — exactly the pre-T27 behavior. An un-configured deploy is
        // unaffected by the whole feature.
        let state = RelayState::new();
        assert!(state.create_mailbox(solved_pow(&state), None).is_ok());
    }

    #[test]
    fn attestation_gate_requires_a_valid_unspent_unexpired_token() {
        use proto::attestation::{signing_key_from_seed, AttestationToken};
        let state = RelayState::new();
        let sk = signing_key_from_seed(&[11u8; 32]);
        state.set_attestation_key(Some(sk.verifying_key()));
        let now = now_unix();

        // No token at all → rejected.
        assert_eq!(
            state.create_mailbox(solved_pow(&state), None).unwrap_err(),
            RejectionCode::InvalidAttestation
        );

        // A valid token → accepted, exactly once.
        let good = AttestationToken::sign(&sk, [1u8; 16], now + 3600);
        assert!(state
            .create_mailbox(solved_pow(&state), Some(good.clone()))
            .is_ok());

        // Replaying the same token → rejected (single-use).
        assert_eq!(
            state
                .create_mailbox(solved_pow(&state), Some(good))
                .unwrap_err(),
            RejectionCode::InvalidAttestation
        );

        // An expired token → rejected.
        let expired = AttestationToken::sign(&sk, [2u8; 16], now - 1);
        assert_eq!(
            state
                .create_mailbox(solved_pow(&state), Some(expired))
                .unwrap_err(),
            RejectionCode::InvalidAttestation
        );

        // A token signed by a DIFFERENT key (a forgery) → rejected. This is the
        // property that stops a custom client from minting its own tokens.
        let forged =
            AttestationToken::sign(&signing_key_from_seed(&[99u8; 32]), [3u8; 16], now + 3600);
        assert_eq!(
            state
                .create_mailbox(solved_pow(&state), Some(forged))
                .unwrap_err(),
            RejectionCode::InvalidAttestation
        );

        // A group inbox is gated the same way.
        assert_eq!(
            state
                .create_group_inbox(solved_pow(&state), 1, vec![], None)
                .unwrap_err(),
            RejectionCode::InvalidAttestation
        );
    }

    fn env(id: u8) -> Envelope {
        Envelope::new([id; 16], DeliveryMode::RelayFanout, vec![0u8; 8])
    }

    #[test]
    fn mailbox_create_and_send_roundtrip() {
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let e = env(1);
        let tag = proto::compute_tag(&key, &qid, &e);
        state.send_to_mailbox(qid, tag, e).unwrap();
    }

    #[test]
    fn mailbox_send_rejects_bad_auth() {
        let state = RelayState::new();
        let (qid, _key) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let e = env(1);
        let bad_tag = [0u8; 32];
        assert_eq!(
            state.send_to_mailbox(qid, bad_tag, e).unwrap_err(),
            RejectionCode::Unauthorized
        );
    }

    #[test]
    fn mailbox_send_rejects_unknown_queue() {
        let state = RelayState::new();
        assert_eq!(
            state
                .send_to_mailbox([9u8; 32], [0u8; 32], env(1))
                .unwrap_err(),
            RejectionCode::QueueNotFound
        );
    }

    #[test]
    fn reused_pow_solution_rejected() {
        let state = RelayState::new();
        let solution = solved_pow(&state);
        state.create_mailbox(solution, None).unwrap();
        assert_eq!(
            state.create_mailbox(solution, None).unwrap_err(),
            RejectionCode::InvalidProofOfWork
        );
    }

    #[test]
    fn group_inbox_concurrent_commit_conflict_resolves() {
        // The property amendment A1 exists to prove: two commits racing for
        // the same epoch never both win, and the winner deterministically
        // advances the epoch by exactly one.
        let state = RelayState::new();
        let member_a = state.create_mailbox(solved_pow(&state), None).unwrap().0;
        let member_b = state.create_mailbox(solved_pow(&state), None).unwrap().0;
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member_a, member_b], None)
            .unwrap();

        let commit_a = env(0xA);
        let tag_a = proto::compute_tag(&key, &inbox, &commit_a);
        let commit_b = env(0xB);
        let tag_b = proto::compute_tag(&key, &inbox, &commit_b);

        let result_a =
            state.send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 1 }, tag_a, commit_a);
        let result_b =
            state.send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 1 }, tag_b, commit_b);

        // Exactly one wins.
        assert_ne!(result_a.is_ok(), result_b.is_ok());
        let loser = if result_a.is_err() {
            result_a
        } else {
            result_b
        };
        assert_eq!(loser.unwrap_err(), RejectionCode::EpochConflict);

        // The loser can retry against the new epoch and succeeds.
        let retry = env(0xC);
        let retry_tag = proto::compute_tag(&key, &inbox, &retry);
        state
            .send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 2 }, retry_tag, retry)
            .expect("retry against the advanced epoch must succeed");
    }

    #[test]
    fn group_inbox_application_messages_never_conflict() {
        let state = RelayState::new();
        let member = state.create_mailbox(solved_pow(&state), None).unwrap().0;
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member], None)
            .unwrap();
        for i in 0..5u8 {
            let e = env(i);
            let tag = proto::compute_tag(&key, &inbox, &e);
            state
                .send_to_group_inbox(inbox, GroupSendKind::Application, tag, e)
                .expect("application messages within an epoch never conflict");
        }
    }

    #[test]
    fn group_inbox_fans_out_to_all_members() {
        let state = RelayState::new();
        let (member_a, _) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let (member_b, _) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member_a, member_b], None)
            .unwrap();

        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        state.subscribe(&[member_a], tx_a);
        state.subscribe(&[member_b], tx_b);

        let e = env(1);
        let tag = proto::compute_tag(&key, &inbox, &e);
        state
            .send_to_group_inbox(inbox, GroupSendKind::Application, tag, e.clone())
            .unwrap();

        let (qid_a, _, pushed_a) = rx_a
            .try_recv()
            .expect("member A must receive the fan-out push");
        assert_eq!(qid_a, member_a);
        assert_eq!(pushed_a, e);
        let (qid_b, _, pushed_b) = rx_b
            .try_recv()
            .expect("member B must receive the fan-out push");
        assert_eq!(qid_b, member_b);
        assert_eq!(pushed_b, e);
    }

    #[test]
    fn storage_bound_rejects_and_ack_frees_it_again() {
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state), None).unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            // Leave room for exactly one more max-size message.
            inner.storage_bytes_used = limits::MAX_STORAGE_BYTES - limits::MAX_MESSAGE_SIZE as u64;
        }
        let big = Envelope::new(
            [1u8; 16],
            DeliveryMode::RelayFanout,
            vec![0u8; limits::MAX_MESSAGE_SIZE],
        );
        let tag = proto::compute_tag(&key, &qid, &big);
        state
            .send_to_mailbox(qid, tag, big.clone())
            .expect("fits exactly at the bound");

        let over = Envelope::new([2u8; 16], DeliveryMode::RelayFanout, vec![0u8; 1]);
        let tag2 = proto::compute_tag(&key, &qid, &over);
        assert_eq!(
            state.send_to_mailbox(qid, tag2, over.clone()).unwrap_err(),
            RejectionCode::StorageBoundExceeded
        );

        // Ack the first message; its bytes are freed, so the second now fits.
        state.ack(qid, big.message_id);
        state
            .send_to_mailbox(qid, tag2, over)
            .expect("storage freed by ack");
    }

    #[test]
    fn resubscribe_same_connection_does_not_double_deliver() {
        // Bug fix (eng-review OV9): subscribe used to append, so a connection
        // that re-sent its Subscribe (the documented "full current set" flow)
        // got registered twice and received every push twice.
        let state = RelayState::new();
        let (member, _) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member], None)
            .unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        state.subscribe(&[member], tx.clone());
        state.subscribe(&[member], tx); // re-subscribe, same channel

        let e = env(1);
        let tag = proto::compute_tag(&key, &inbox, &e);
        state
            .send_to_group_inbox(inbox, GroupSendKind::Application, tag, e)
            .unwrap();

        assert!(rx.try_recv().is_ok(), "one delivery expected");
        assert!(
            rx.try_recv().is_err(),
            "must not double-deliver to a re-subscribed connection"
        );
    }

    #[test]
    fn dead_subscriber_is_pruned_on_send() {
        // Bug fix (eng-review OV9): a dropped connection's sender used to stay
        // registered forever. After the receiver is dropped, the next send must
        // remove it so the subscriber list doesn't grow without bound.
        let state = RelayState::new();
        let (member, _) = state.create_mailbox(solved_pow(&state), None).unwrap();
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member], None)
            .unwrap();

        let (tx, rx) = mpsc::unbounded_channel();
        state.subscribe(&[member], tx);
        drop(rx); // connection gone

        let e = env(1);
        let tag = proto::compute_tag(&key, &inbox, &e);
        state
            .send_to_group_inbox(inbox, GroupSendKind::Application, tag, e)
            .unwrap();

        let inner = state.inner.lock().unwrap();
        assert_eq!(
            inner.subscribers.get(&member).map(|v| v.len()).unwrap_or(0),
            0,
            "dead sender must be pruned after a failed push"
        );
    }

    #[test]
    fn subscribe_drains_unacked_backlog_and_ack_ends_redelivery() {
        // T4 (eng-review OV3): enqueue while unsubscribed, resubscribe,
        // receive the backlog, ack, storage freed — and a later subscriber
        // no longer sees the acked messages.
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state), None).unwrap();

        // Two messages land while nobody is subscribed.
        let first = env(1);
        let second = env(2);
        for e in [&first, &second] {
            let tag = proto::compute_tag(&key, &qid, e);
            state.send_to_mailbox(qid, tag, e.clone()).unwrap();
        }

        // Subscribing drains the backlog, oldest first.
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.subscribe(&[qid], tx);
        let (_, mid_1, env_1) = rx.try_recv().expect("backlog message 1 redelivered");
        let (_, mid_2, env_2) = rx.try_recv().expect("backlog message 2 redelivered");
        assert_eq!((mid_1, &env_1), (first.message_id, &first));
        assert_eq!((mid_2, &env_2), (second.message_id, &second));
        assert!(rx.try_recv().is_err(), "backlog has exactly two messages");

        // Acking both frees their storage entirely.
        state.ack(qid, mid_1);
        state.ack(qid, mid_2);
        assert_eq!(
            state.inner.lock().unwrap().storage_bytes_used,
            0,
            "delete-on-ack must free the stored bytes"
        );

        // A fresh connection subscribing now gets nothing: acked messages
        // are gone for good.
        let (tx2, mut rx2) = mpsc::unbounded_channel();
        state.subscribe(&[qid], tx2);
        assert!(
            rx2.try_recv().is_err(),
            "acked messages must not be redelivered"
        );
    }

    #[test]
    fn unacked_message_is_redelivered_on_resubscribe() {
        // At-least-once (T4): a message that was pushed live but never acked
        // (client crashed mid-processing) arrives again on the next
        // subscribe. The duplicate is the contract — engine dedup (OV5)
        // absorbs it client-side.
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state), None).unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        state.subscribe(&[qid], tx.clone());

        let e = env(1);
        let tag = proto::compute_tag(&key, &qid, &e);
        state.send_to_mailbox(qid, tag, e.clone()).unwrap();
        assert!(rx.try_recv().is_ok(), "live push delivered");

        // No ack. Re-subscribing (same connection, per the documented
        // full-set refresh flow) replays the unacked message.
        state.subscribe(&[qid], tx);
        let (_, mid, _) = rx.try_recv().expect("unacked message must be redelivered");
        assert_eq!(mid, e.message_id);
    }

    #[test]
    fn state_survives_restart_with_backlog_epochs_and_keys() {
        // T9 (eng-review OV1): dropping the state at ANY point stands in for
        // kill -9 — write-through persistence means there is no flush to
        // miss. Queues, send keys, the unacked backlog, and the DS epoch all
        // come back; acks are just as durable.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay_state.sqlite3");

        let (member, inbox, inbox_key, app_env, commit_env, used_before) = {
            let state = RelayState::open(&path).unwrap();
            let (member, _member_key) = state.create_mailbox(solved_pow(&state), None).unwrap();
            let (inbox, inbox_key) = state
                .create_group_inbox(solved_pow(&state), 1, vec![member], None)
                .unwrap();

            // One application message and one epoch-advancing commit land
            // while nobody is subscribed.
            let app_env = env(1);
            let tag = proto::compute_tag(&inbox_key, &inbox, &app_env);
            state
                .send_to_group_inbox(inbox, GroupSendKind::Application, tag, app_env.clone())
                .unwrap();
            let commit_env = env(2);
            let tag = proto::compute_tag(&inbox_key, &inbox, &commit_env);
            state
                .send_to_group_inbox(
                    inbox,
                    GroupSendKind::Commit { epoch: 1 },
                    tag,
                    commit_env.clone(),
                )
                .unwrap();

            let used = state.inner.lock().unwrap().storage_bytes_used;
            (member, inbox, inbox_key, app_env, commit_env, used)
        }; // ← the "crash"

        let state = RelayState::open(&path).unwrap();
        assert_eq!(
            state.inner.lock().unwrap().storage_bytes_used,
            used_before,
            "storage accounting must be rebuilt from the stored backlog"
        );

        // The unacked backlog replays on subscribe, in order.
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.subscribe(&[member], tx);
        let (_, mid_1, env_1) = rx.try_recv().expect("backlog message 1 survives restart");
        let (_, mid_2, env_2) = rx.try_recv().expect("backlog message 2 survives restart");
        assert_eq!((mid_1, env_1), (app_env.message_id, app_env.clone()));
        assert_eq!((mid_2, env_2), (commit_env.message_id, commit_env.clone()));
        assert!(rx.try_recv().is_err());

        // The DS epoch survived the restart: epoch 1 was burned before the
        // crash, so a stale commit still conflicts and the current one wins.
        // That the tags verify at all proves the send key survived too.
        let stale = env(3);
        let tag = proto::compute_tag(&inbox_key, &inbox, &stale);
        assert_eq!(
            state
                .send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 1 }, tag, stale)
                .unwrap_err(),
            RejectionCode::EpochConflict
        );
        let fresh = env(4);
        let tag = proto::compute_tag(&inbox_key, &inbox, &fresh);
        state
            .send_to_group_inbox(
                inbox,
                GroupSendKind::Commit { epoch: 2 },
                tag,
                fresh.clone(),
            )
            .unwrap();

        // Acks are durable too: retire everything, restart again, and the
        // backlog is gone for good.
        state.ack(member, app_env.message_id);
        state.ack(member, commit_env.message_id);
        state.ack(member, fresh.message_id);
        drop(state);

        let state = RelayState::open(&path).unwrap();
        assert_eq!(state.inner.lock().unwrap().storage_bytes_used, 0);
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.subscribe(&[member], tx);
        assert!(
            rx.try_recv().is_err(),
            "acked messages must not resurrect across a restart"
        );
    }

    #[test]
    fn outstanding_pow_challenges_are_bounded() {
        // Bug fix (eng-review OV9): requesting challenges without solving them
        // used to grow the map without limit (OOM). It must stay capped.
        let state = RelayState::new();
        for _ in 0..(limits::MAX_OUTSTANDING_POW_CHALLENGES + 500) {
            state.issue_pow_challenge();
        }
        let inner = state.inner.lock().unwrap();
        assert!(
            inner.outstanding_challenges.len() <= limits::MAX_OUTSTANDING_POW_CHALLENGES,
            "outstanding challenge map must stay within its cap"
        );
        assert!(
            inner.challenge_order.len() <= limits::MAX_OUTSTANDING_POW_CHALLENGES,
            "challenge order deque must stay within its cap"
        );
    }
}
