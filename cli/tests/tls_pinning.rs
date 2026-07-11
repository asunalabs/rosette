//! T2 security check: the client pins the relay's cert fingerprint, so a relay
//! presenting any other certificate must be rejected at the TLS handshake. This
//! is the whole point of pinning — without this test, a silently-broken
//! verifier (e.g. one that always returns "verified") would pass the happy-path
//! convergence test just fine.

use std::sync::Arc;

use cli::RelayClient;
use relay::{RelayIdentity, RelayState};

async fn start_relay() -> (String, [u8; 32]) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let state = Arc::new(RelayState::new());
    let identity = RelayIdentity::generate();
    let fingerprint = identity.fingerprint;
    tokio::spawn(async move {
        relay::net::serve_on(listener, state, &identity).await.ok();
    });
    (addr, fingerprint)
}

#[tokio::test]
async fn correct_fingerprint_connects() {
    let (addr, fp) = start_relay().await;
    // A create_mailbox round-trip proves the TLS session is actually usable,
    // not just that the handshake bytes flowed.
    let client = RelayClient::connect(&addr, fp)
        .await
        .expect("pinned connect with the right fingerprint succeeds");
    client
        .create_mailbox()
        .await
        .expect("authenticated relay session works end to end");
}

#[tokio::test]
async fn wrong_fingerprint_is_rejected() {
    let (addr, mut fp) = start_relay().await;
    fp[0] ^= 0xff; // flip a byte: no longer the relay's real fingerprint

    let result = RelayClient::connect(&addr, fp).await;
    assert!(
        result.is_err(),
        "connecting with a mismatched pin must fail the TLS handshake, not succeed"
    );
}
