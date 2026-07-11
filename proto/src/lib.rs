//! Wire protocol shared by core/, relay/, and cli/. Single source of truth for
//! every type that crosses the client-relay boundary — see plan amendment A6.

pub mod auth;
pub mod envelope;
pub mod error;
pub mod framing;
pub mod limits;
pub mod link;
pub mod pow;
pub mod wire;

pub use auth::{compute_tag, verify_tag};
pub use envelope::{pad, padded_bucket_for, DeliveryMode, Envelope, MessageId};
pub use error::RejectionCode;
pub use link::{ContactLink, Endpoint, LinkError, QueueId, LINK_VERSION_V1};
pub use pow::{PowChallenge, PowSolution};
pub use wire::{AuthTag, ClientMessage, GroupSendKind, ServerMessage};

/// Serialize any wire type with the shared bincode config. Centralized so a
/// future format change (bincode major version, compression) touches one place.
pub fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    bincode::serialize(value).expect("proto types are always serializable")
}

/// Deserialize any wire type with the shared bincode config.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, bincode::Error> {
    bincode::deserialize(bytes)
}
