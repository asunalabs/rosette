//! Client-relay wire protocol. One persistent connection per relay: the
//! client SUBSCRIBEs every queue it owns and the relay pushes (amendment
//! A12) instead of the client polling each queue on a timer.
//!
//! Disclosed limitation (amendment A13): a single SUBSCRIBE listing every
//! owned queue lets the relay cluster those queues to one connection/IP.
//! Open Question 3 owns the unlinkable-fetch + IP-protection mechanism that
//! would remove this. v1 does not claim relay-unlinkability.
//!
//! Two queue kinds cross this protocol (amendment A1, architecture sketch):
//! - **Mailbox**: plain store-and-forward, used for pairing bootstrap and as
//!   a group member's fan-out target. No ordering semantics.
//! - **Group inbox**: the relay's Delivery Service role. Content-blind but
//!   epoch-aware — it enforces exactly one accepted commit per epoch so two
//!   concurrent commits can never both land; the loser gets `EpochConflict`
//!   and must retry against the new epoch.

use serde::{Deserialize, Serialize};

use crate::envelope::{Envelope, MessageId};
use crate::error::RejectionCode;
use crate::link::QueueId;
use crate::pow::PowSolution;
pub use crate::pow::PowChallenge;

/// MAC over (queue_id, envelope) using the per-queue send key established at
/// pairing/creation. "No accounts" never means "no send authorization."
pub type AuthTag = [u8; 32];

/// Distinguishes the two group-inbox send behaviors. Only a Commit advances
/// the epoch and can conflict; an Application message never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupSendKind {
    Commit { epoch: u64 },
    Application,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Send to a plain mailbox queue (pairing bootstrap, or a group member's
    /// fan-out target).
    SendToMailbox {
        queue_id: QueueId,
        auth_tag: AuthTag,
        envelope: Envelope,
    },
    /// Send to a group inbox queue: a Commit (epoch-gated — the DS accepts
    /// only the first commit it sees for each epoch, rejecting the rest with
    /// `EpochConflict`) or an Application message (fans out unconditionally;
    /// multiple senders may share an epoch without conflict). Fan-out targets
    /// are the roster fixed at group-inbox creation (v0.1 scope cut: dynamic
    /// membership changes are a later milestone — design doc Next Steps #5).
    SendToGroupInbox {
        queue_id: QueueId,
        kind: GroupSendKind,
        auth_tag: AuthTag,
        envelope: Envelope,
    },
    /// Replace the full set of queues this connection watches. Sent once at
    /// connect and again whenever the local queue set changes.
    Subscribe { queue_ids: Vec<QueueId> },
    /// Acknowledge receipt so the relay can apply the delete-on-ack retention
    /// rule (amendment A3) to its fan-out journal.
    Ack {
        queue_id: QueueId,
        message_id: MessageId,
    },
    /// Ask the relay to mint a challenge before creating a queue (A18).
    RequestPowChallenge,
    /// Create a plain mailbox queue.
    CreateMailbox { solution: PowSolution },
    /// Create a group inbox queue starting at epoch 1 (the epoch right after
    /// the founding Add commit, which never touches the relay — see A1 test
    /// design) with a fixed initial fan-out roster.
    CreateGroupInbox {
        solution: PowSolution,
        initial_epoch: u64,
        fan_out_to: Vec<QueueId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Ok,
    Error(RejectionCode),
    /// Pushed to a subscribed connection as soon as a new envelope lands in
    /// one of its queues — no polling.
    Push {
        queue_id: QueueId,
        envelope: Envelope,
    },
    PowChallenge(PowChallenge),
    QueueCreated {
        queue_id: QueueId,
        send_key: [u8; 32],
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::DeliveryMode;

    #[test]
    fn send_roundtrip() {
        let msg = ClientMessage::SendToMailbox {
            queue_id: [1u8; 32],
            auth_tag: [2u8; 32],
            envelope: Envelope::new([3u8; 16], DeliveryMode::RelayFanout, vec![0u8; 16]),
        };
        let bytes = crate::encode(&msg);
        let decoded: ClientMessage = crate::decode(&bytes).unwrap();
        match decoded {
            ClientMessage::SendToMailbox { queue_id, .. } => assert_eq!(queue_id, [1u8; 32]),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn send_commit_roundtrip() {
        let msg = ClientMessage::SendToGroupInbox {
            queue_id: [1u8; 32],
            kind: GroupSendKind::Commit { epoch: 5 },
            auth_tag: [2u8; 32],
            envelope: Envelope::new([3u8; 16], DeliveryMode::RelayFanout, vec![0u8; 16]),
        };
        let bytes = crate::encode(&msg);
        let decoded: ClientMessage = crate::decode(&bytes).unwrap();
        match decoded {
            ClientMessage::SendToGroupInbox {
                kind: GroupSendKind::Commit { epoch },
                ..
            } => assert_eq!(epoch, 5),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn subscribe_roundtrip() {
        let msg = ClientMessage::Subscribe {
            queue_ids: vec![[1u8; 32], [2u8; 32]],
        };
        let bytes = crate::encode(&msg);
        let decoded: ClientMessage = crate::decode(&bytes).unwrap();
        match decoded {
            ClientMessage::Subscribe { queue_ids } => assert_eq!(queue_ids.len(), 2),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn server_error_roundtrip() {
        let msg = ServerMessage::Error(RejectionCode::EpochConflict);
        let bytes = crate::encode(&msg);
        let decoded: ServerMessage = crate::decode(&bytes).unwrap();
        matches!(decoded, ServerMessage::Error(RejectionCode::EpochConflict));
    }
}
