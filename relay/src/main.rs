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

    // T27: enable the attestation gate iff the directory's public key is
    // configured. `RELAY_ATTESTATION_PUBKEY` is the base64 the directory logs
    // at startup. No var → gate stays off (PoW-only), same as before T27.
    match std::env::var("RELAY_ATTESTATION_PUBKEY") {
        Ok(b64) => {
            use base64::Engine as _;
            let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("RELAY_ATTESTATION_PUBKEY must be base64 of a 32-byte key")
                })?;
            let key = proto::attestation::verifying_key_from_bytes(&bytes)
                .ok_or_else(|| anyhow::anyhow!("RELAY_ATTESTATION_PUBKEY is not a valid key"))?;
            state.set_attestation_key(Some(key));
            println!("attestation gate ENABLED (queue creation requires a directory token)");
        }
        Err(_) => println!("attestation gate off (RELAY_ATTESTATION_PUBKEY unset)"),
    }

    relay::net::serve(&addr, state, &identity).await
}
