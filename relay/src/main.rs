use std::path::PathBuf;
use std::sync::Arc;

use relay::{RelayIdentity, RelayState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7443".to_string());
    // Persistent so the pinned fingerprint in already-shared contact links
    // survives restarts (see identity.rs). Override with arg 2.
    let identity_path = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("relay_identity.der"));
    // Queue/epoch/backlog state (T9) — the file that makes restarts (and
    // kill -9) invisible to clients. Override with arg 3.
    let state_path = std::env::args()
        .nth(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("relay_state.sqlite3"));

    let identity = RelayIdentity::load_or_create(&identity_path)?;
    println!("relay TLS fingerprint: {}", identity.fingerprint_hex());

    let state = Arc::new(RelayState::open(&state_path)?);
    relay::net::serve(&addr, state, &identity).await
}
