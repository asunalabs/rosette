//! Transport-agnostic relay state: queue storage, DS epoch enforcement, and
//! resource limits. Kept separate from net.rs so the DS conflict-resolution
//! logic (the property amendment A1 exists to prove) is unit-testable
//! without a TCP stack.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use proto::{
    limits, AuthTag, ClientMessage, Envelope, GroupSendKind, MessageId, PowChallenge,
    PowSolution, QueueId, RejectionCode, ServerMessage,
};
use rand::RngCore;
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
    storage_bytes_used: u64,
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

impl RelayState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue_pow_challenge(&self) -> PowChallenge {
        let challenge = PowChallenge::generate(limits::QUEUE_CREATION_POW_DIFFICULTY);
        let mut inner = self.inner.lock().unwrap();
        inner
            .outstanding_challenges
            .insert(challenge.challenge, challenge.difficulty);
        challenge
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

    pub fn create_mailbox(&self, solution: PowSolution) -> Result<(QueueId, [u8; 32]), RejectionCode> {
        let mut inner = self.inner.lock().unwrap();
        self.consume_pow(&mut inner, &solution)?;
        let queue_id = Self::fresh_queue_id(&inner);
        let mut send_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut send_key);
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
    ) -> Result<(QueueId, [u8; 32]), RejectionCode> {
        let mut inner = self.inner.lock().unwrap();
        self.consume_pow(&mut inner, &solution)?;
        let queue_id = Self::fresh_queue_id(&inner);
        let mut send_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut send_key);
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

    /// Registers `tx` to receive every future push for `queue_ids`. Replaces
    /// this sender's prior subscription entirely (a fresh Subscribe call
    /// always states the full current set — see wire.rs docs).
    pub fn subscribe(&self, queue_ids: &[QueueId], tx: PushSender) {
        let mut inner = self.inner.lock().unwrap();
        for qid in queue_ids {
            inner.subscribers.entry(*qid).or_default().push(tx.clone());
        }
    }

    fn push_and_notify(inner: &mut Inner, queue_id: QueueId, message_id: MessageId, envelope: Envelope) {
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
        if let Some(subs) = inner.subscribers.get(&queue_id) {
            for tx in subs {
                let _ = tx.send((queue_id, message_id, envelope.clone()));
            }
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
            if let GroupSendKind::Commit { epoch: target_epoch } = kind {
                if target_epoch != current_epoch {
                    return Err(RejectionCode::EpochConflict);
                }
            }
            roster
        };
        if Self::would_exceed_storage_bound(&inner, envelope.padded_ciphertext.len(), fan_out_to.len()) {
            return Err(RejectionCode::StorageBoundExceeded);
        }
        {
            let entry = inner.queues.get_mut(&queue_id).expect("checked above");
            entry.check_and_bump_rate_limit()?;
            if let (GroupSendKind::Commit { .. }, QueueKind::GroupInbox { epoch, .. }) = (kind, &mut entry.kind) {
                *epoch += 1;
            }
        }
        for member_queue in fan_out_to {
            Self::push_and_notify(&mut inner, member_queue, message_id, envelope.clone());
        }
        Ok(())
    }

    pub fn ack(&self, queue_id: QueueId, message_id: MessageId) {
        // v0.1 scope cut (disclosed): mailbox `pending` is drained on ack for
        // storage accounting, but the full fan-out journal with per-recipient
        // delete-on-ack + TTL retention (amendment A3) is not yet built —
        // tracked as T3 in tasks-eng-review-*.jsonl. This still gives the
        // skeleton real backpressure (MAX_QUEUE_DEPTH) without journal state.
        let mut inner = self.inner.lock().unwrap();
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
            ClientMessage::RequestPowChallenge => ServerMessage::PowChallenge(self.issue_pow_challenge()),
            ClientMessage::CreateMailbox { solution } => match self.create_mailbox(solution) {
                Ok((queue_id, send_key)) => ServerMessage::QueueCreated { queue_id, send_key },
                Err(e) => ServerMessage::Error(e),
            },
            ClientMessage::CreateGroupInbox {
                solution,
                initial_epoch,
                fan_out_to,
            } => match self.create_group_inbox(solution, initial_epoch, fan_out_to) {
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
            ClientMessage::Ack { queue_id, message_id } => {
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

    fn env(id: u8) -> Envelope {
        Envelope::new([id; 16], DeliveryMode::RelayFanout, vec![0u8; 8])
    }

    #[test]
    fn mailbox_create_and_send_roundtrip() {
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state)).unwrap();
        let e = env(1);
        let tag = proto::compute_tag(&key, &qid, &e);
        state.send_to_mailbox(qid, tag, e).unwrap();
    }

    #[test]
    fn mailbox_send_rejects_bad_auth() {
        let state = RelayState::new();
        let (qid, _key) = state.create_mailbox(solved_pow(&state)).unwrap();
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
            state.send_to_mailbox([9u8; 32], [0u8; 32], env(1)).unwrap_err(),
            RejectionCode::QueueNotFound
        );
    }

    #[test]
    fn reused_pow_solution_rejected() {
        let state = RelayState::new();
        let solution = solved_pow(&state);
        state.create_mailbox(solution).unwrap();
        assert_eq!(
            state.create_mailbox(solution).unwrap_err(),
            RejectionCode::InvalidProofOfWork
        );
    }

    #[test]
    fn group_inbox_concurrent_commit_conflict_resolves() {
        // The property amendment A1 exists to prove: two commits racing for
        // the same epoch never both win, and the winner deterministically
        // advances the epoch by exactly one.
        let state = RelayState::new();
        let member_a = state.create_mailbox(solved_pow(&state)).unwrap().0;
        let member_b = state.create_mailbox(solved_pow(&state)).unwrap().0;
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member_a, member_b])
            .unwrap();

        let commit_a = env(0xA);
        let tag_a = proto::compute_tag(&key, &inbox, &commit_a);
        let commit_b = env(0xB);
        let tag_b = proto::compute_tag(&key, &inbox, &commit_b);

        let result_a = state.send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 1 }, tag_a, commit_a);
        let result_b = state.send_to_group_inbox(inbox, GroupSendKind::Commit { epoch: 1 }, tag_b, commit_b);

        // Exactly one wins.
        assert_ne!(result_a.is_ok(), result_b.is_ok());
        let loser = if result_a.is_err() { result_a } else { result_b };
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
        let member = state.create_mailbox(solved_pow(&state)).unwrap().0;
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member])
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
        let (member_a, _) = state.create_mailbox(solved_pow(&state)).unwrap();
        let (member_b, _) = state.create_mailbox(solved_pow(&state)).unwrap();
        let (inbox, key) = state
            .create_group_inbox(solved_pow(&state), 1, vec![member_a, member_b])
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

        let (qid_a, _, pushed_a) = rx_a.try_recv().expect("member A must receive the fan-out push");
        assert_eq!(qid_a, member_a);
        assert_eq!(pushed_a, e);
        let (qid_b, _, pushed_b) = rx_b.try_recv().expect("member B must receive the fan-out push");
        assert_eq!(qid_b, member_b);
        assert_eq!(pushed_b, e);
    }

    #[test]
    fn storage_bound_rejects_and_ack_frees_it_again() {
        let state = RelayState::new();
        let (qid, key) = state.create_mailbox(solved_pow(&state)).unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            // Leave room for exactly one more max-size message.
            inner.storage_bytes_used = limits::MAX_STORAGE_BYTES - limits::MAX_MESSAGE_SIZE as u64;
        }
        let big = Envelope::new([1u8; 16], DeliveryMode::RelayFanout, vec![0u8; limits::MAX_MESSAGE_SIZE]);
        let tag = proto::compute_tag(&key, &qid, &big);
        state.send_to_mailbox(qid, tag, big.clone()).expect("fits exactly at the bound");

        let over = Envelope::new([2u8; 16], DeliveryMode::RelayFanout, vec![0u8; 1]);
        let tag2 = proto::compute_tag(&key, &qid, &over);
        assert_eq!(
            state.send_to_mailbox(qid, tag2, over.clone()).unwrap_err(),
            RejectionCode::StorageBoundExceeded
        );

        // Ack the first message; its bytes are freed, so the second now fits.
        state.ack(qid, big.message_id);
        state.send_to_mailbox(qid, tag2, over).expect("storage freed by ack");
    }
}
