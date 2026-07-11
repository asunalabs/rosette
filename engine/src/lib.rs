//! Client engine (architecture.md step 2, extracted from cli/). Owns the
//! relay connection + reconnect loop, the subscribe set, orchestration
//! between MLS (`chatcore`) and the wire (`proto`), own-echo and
//! foreign-duplicate dedup (OV5), and the epoch-conflict auto-retry loop
//! (OV4). The CLI is a thin REPL over this; the FFI surface (ffi/) will wrap
//! the same object. Kotlin never re-implements anything found here.
//!
//! Later milestones tracked elsewhere: SQLCipher persistence (T5/T8) gives
//! the seen-set and session state a disk home; multi-relay endpoints (OQ11).

pub mod chat_engine;
pub mod pairing;
pub mod relay_client;
pub mod tls;

pub use chat_engine::{ChatEngine, Event};
pub use relay_client::{ConnectionClosed, RelayClient};
