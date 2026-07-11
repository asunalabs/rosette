//! MLS protocol core. All chats are MLS groups (design doc architecture
//! sketch); this crate owns every OpenMLS interaction, identity, and the
//! MLS-native pairing format (amendment A4). relay/ and cli/ never construct
//! MLS types directly.

pub mod identity;
pub mod pairing;
pub mod provider;
pub mod session;

pub use identity::Identity;
pub use provider::{Provider, CIPHERSUITE};
pub use session::{message_id_for, ChatSession, Incoming, SessionError};
