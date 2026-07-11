//! Store-and-forward relay. Content-blind but epoch-aware: it enforces
//! Delivery Service ordering (one accepted commit per epoch) without ever
//! reading message plaintext (design doc, relay/ sketch; amendment A1).
//!
//! ```text
//! RELAY QUEUE DATA FLOW (v1) — see design doc Eng Review Amendments for the
//! full diagram (A3/A11 fan-out journal + refcount unification, A20 relay
//! disposability). This crate implements the mailbox + group-inbox halves;
//! journal retention (delete-on-ack, TTL) and refcounted blob storage are
//! disclosed v0.1 cuts — see the skeleton scope note in cli/README or the
//! plan-eng-review tasks JSONL (T3 in tasks-eng-review-*.jsonl).
//! ```

pub mod net;
mod state;

pub use state::{RelayState, RELAY_QUEUE_CAPACITY_HINT};
