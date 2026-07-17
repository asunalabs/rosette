//! MLS protocol core. All chats are MLS groups (design doc architecture
//! sketch); this crate owns every OpenMLS interaction, identity, and the
//! MLS-native pairing format (amendment A4). relay/ and cli/ never construct
//! MLS types directly.

pub mod backup;
pub mod identity;
pub mod pairing;
pub mod provider;
pub mod session;
pub mod storage;

pub use identity::Identity;
pub use provider::{Provider, CIPHERSUITE};
pub use session::{
    message_id_for, strip_snapshot_to_identity, ChatSession, Incoming, SessionError,
};
pub use storage::{Store, StoreError};
