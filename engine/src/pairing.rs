//! The v0.1 pairing bootstrap wire format (moved from cli/main.rs with the
//! engine extraction). Public because it is a client↔client protocol: both
//! sides of a pairing — and any test harness standing in for one — must
//! agree on it byte-for-byte.
//!
//! v0.1 scope cut (disclosed): the payload's ratchet tree and group inbox
//! credentials ride unencrypted past the Welcome's own MLS encryption —
//! fine for a demo, not yet the hardened pairing spec (T4 pairing
//! hardening, architecture.md step 6).

use proto::QueueId;
use serde::{Deserialize, Serialize};

/// What travels through the bootstrap mailbox: the Welcome (self-encrypted
/// by MLS to the invitee's KeyPackage — safe for the relay to forward
/// blind) plus the ratchet tree and the fresh group inbox's credentials.
#[derive(Serialize, Deserialize)]
pub struct BootstrapPayload {
    pub welcome_wire: Vec<u8>,
    pub tree_wire: Vec<u8>,
    pub inbox_qid: QueueId,
    pub inbox_key: [u8; 32],
}
