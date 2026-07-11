//! Explicit rejection codes (amendment A7). A relay that hits a limit rejects
//! with one of these — it never silently drops a send.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum RejectionCode {
    #[error("queue is at its depth cap")]
    QueueFull,
    #[error("message exceeds the max message size")]
    MessageTooLarge,
    #[error("rate limit exceeded for this queue")]
    RateLimited,
    #[error("send is not authorized for this queue")]
    Unauthorized,
    #[error("proof of work is invalid or insufficient")]
    InvalidProofOfWork,
    #[error("queue does not exist")]
    QueueNotFound,
    #[error("relay storage bound exceeded")]
    StorageBoundExceeded,
    /// A group-inbox commit targeted an epoch the relay already resolved.
    /// Amendment A1: the DS enforces exactly one accepted commit per epoch;
    /// the loser of a concurrent commit gets this and must retry against the
    /// new epoch after processing the winner.
    #[error("commit targets an epoch already resolved by another commit")]
    EpochConflict,
    #[error("no such group inbox queue")]
    GroupInboxNotFound,
}
