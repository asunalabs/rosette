//! T5/T8 (SQLCipher client persistence): kill the CLIENT mid-conversation
//! and resume it from its encrypted store. Dropping the engine severs its
//! connection and discards every in-memory structure — the closest
//! in-process stand-in for process death. `ChatEngine::resume` must rebuild
//! identity, MLS state, queue credentials, and the seen set from disk; the
//! relay's unacked backlog (T4) then delivers what arrived while dead,
//! without duplicating anything applied before the crash.

use std::sync::Arc;
use std::time::Duration;

use chatcore::Store;
use engine::{ChatEngine, Event};
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

/// Pump until `expected` arrives; any other Message first is a failure
/// (dedup regression), ConnectionChanged pairs are tolerated.
async fn pump_until_message(engine: &mut ChatEngine, expected: &[u8]) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let event = tokio::time::timeout_at(deadline, engine.next_event())
            .await
            .expect("event must arrive before the deadline")
            .unwrap();
        match event {
            Event::Message(bytes) => {
                assert_eq!(bytes, expected, "no other message may arrive first");
                return;
            }
            Event::ConnectionChanged(_) => continue,
            other => panic!("unexpected event while waiting for a message: {other:?}"),
        }
    }
}

/// Pair a persistent bob (listen side, store attached before pairing so the
/// pairing itself is crash-safe) with an in-memory alice.
async fn paired(dir: &std::path::Path, addr: &str, fp: [u8; 32]) -> (ChatEngine, ChatEngine) {
    let mut bob = ChatEngine::connect("bob", addr, fp).await.unwrap();
    bob.attach_store(Store::open(&dir.join("bob.db"), "bob's key").unwrap())
        .unwrap();
    let link = bob.contact_link().unwrap();
    let alice = ChatEngine::pair_with_link("alice", &link).await.unwrap();
    bob.await_pairing().await.unwrap();
    (alice, bob)
}

#[tokio::test(flavor = "multi_thread")]
async fn conversation_resumes_after_client_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, fp) = start_relay().await;
    let (mut alice, mut bob) = paired(dir.path(), &addr, fp).await;

    alice.send_message(b"one").await.unwrap();
    pump_until_message(&mut bob, b"one").await;
    bob.send_message(b"two").await.unwrap();
    pump_until_message(&mut alice, b"two").await;

    // Process death: connection severed, every in-memory structure gone.
    drop(bob);

    // Arrives while bob is dead — queued unacked on the relay.
    alice.send_message(b"while you were down").await.unwrap();

    let mut bob = ChatEngine::resume(Store::open(&dir.path().join("bob.db"), "bob's key").unwrap())
        .await
        .unwrap();
    assert!(bob.is_paired());
    assert_eq!(bob.peer_name().as_deref(), Some("alice"));

    // The backlog message arrives — and nothing already applied before the
    // crash may be surfaced again (pump_until_message fails on any other
    // Message first).
    pump_until_message(&mut bob, b"while you were down").await;

    // Both directions still work on the resumed MLS state.
    bob.send_message(b"back from the dead").await.unwrap();
    pump_until_message(&mut alice, b"back from the dead").await;
    alice.send_message(b"welcome back").await.unwrap();
    pump_until_message(&mut bob, b"welcome back").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_survives_an_epoch_advance() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, fp) = start_relay().await;
    let (mut alice, mut bob) = paired(dir.path(), &addr, fp).await;

    // Advance the group past its founding epoch, make sure bob has applied
    // the commit (so the post-commit MLS state is what his store holds).
    let epoch = alice.commit_self_update().await.unwrap();
    assert_eq!(bob.next_event().await.unwrap(), Event::EpochAdvanced(epoch));

    drop(bob);
    let mut bob = ChatEngine::resume(Store::open(&dir.path().join("bob.db"), "bob's key").unwrap())
        .await
        .unwrap();
    assert_eq!(bob.epoch().unwrap(), epoch);

    alice.send_message(b"post-commit").await.unwrap();
    pump_until_message(&mut bob, b"post-commit").await;
    bob.send_message(b"still in sync").await.unwrap();
    pump_until_message(&mut alice, b"still in sync").await;
}
