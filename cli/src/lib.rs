//! Orchestration glue: wires `chatcore` (MLS) to `relay_client` (transport).
//! `main.rs` is a thin interactive REPL over this; `tests/` drives the same
//! API to prove the full stack end-to-end, including the walking skeleton's
//! hardest test — concurrent-commit convergence (amendment A1).

pub mod relay_client;
pub mod tls;

pub use relay_client::RelayClient;
