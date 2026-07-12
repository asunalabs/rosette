//! The T9 verify bar (architecture.md step 5): kill the relay
//! mid-conversation, restart it from its persisted state, and the
//! conversation resumes. "Kill" here is dropping the relay's entire tokio
//! runtime — every task and connection dies at once, the closest in-process
//! stand-in for kill -9. Write-through persistence (relay/src/state.rs)
//! means there is no shutdown path to flush; the restarted relay serves the
//! same identity (pinned fingerprint), queues, epochs, and unacked backlog
//! from disk, and the engines' reconnect loops sew the session back
//! together with no client-side ceremony.

use std::sync::Arc;
use std::time::Duration;

use engine::{ChatEngine, Event};
use relay::{RelayIdentity, RelayState};
use tokio::runtime::Runtime;

/// Serve a persistent relay (state + identity from `dir`) on `addr` inside
/// its own runtime, so dropping the runtime is a hard kill. `addr` may be
/// "127.0.0.1:0" (first boot) — the actual address is returned.
fn boot_relay(rt: &Runtime, dir: &std::path::Path, addr: &str) -> String {
    let identity = RelayIdentity::load_or_create(&dir.join("relay_identity.der")).unwrap();
    let state = Arc::new(RelayState::open(&dir.join("relay_state.sqlite3")).unwrap());
    rt.block_on(async move {
        // On restart the port was just torn down with the old runtime.
        // SO_REUSEADDR lets the rebind go through while the old accepted
        // sockets sit in TIME_WAIT (without it, Windows refuses the port for
        // 1-2 minutes); the bounded retry covers the small window before the
        // OS finishes the teardown. Bounded so a regression fails loudly
        // instead of hanging the suite.
        let mut last_err = None;
        let listener = 'bound: {
            for _ in 0..100 {
                let socket = tokio::net::TcpSocket::new_v4().unwrap();
                socket.set_reuseaddr(true).unwrap();
                match socket.bind(addr.parse().unwrap()) {
                    Ok(()) => match socket.listen(1024) {
                        Ok(listener) => break 'bound listener,
                        Err(e) => last_err = Some(e),
                    },
                    Err(e) => last_err = Some(e),
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("relay could not rebind {addr}: {last_err:?}");
        };
        let bound = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            relay::net::serve_on(listener, state, &identity).await.ok();
        });
        bound
    })
}

/// Pump events until `expected` arrives, tolerating the ConnectionChanged
/// pair a reconnect produces — but no other Message may come first.
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

#[test]
fn conversation_resumes_after_relay_restart() {
    let dir = tempfile::tempdir().unwrap();

    let relay_rt = Runtime::new().unwrap();
    let addr = boot_relay(&relay_rt, dir.path(), "127.0.0.1:0");
    let fingerprint = RelayIdentity::load_or_create(&dir.path().join("relay_identity.der"))
        .unwrap()
        .fingerprint;

    // The clients outlive the relay, so they live on their own runtime.
    let client_rt = Runtime::new().unwrap();
    let (mut alice, mut bob) = client_rt.block_on(async {
        let mut alice = ChatEngine::connect("alice", &addr, fingerprint)
            .await
            .unwrap();
        let link = alice.contact_link().unwrap();
        let mut bob = ChatEngine::pair_with_link("bob", &link).await.unwrap();
        alice.await_pairing().await.unwrap();
        bob.send_message(b"one").await.unwrap();
        assert_eq!(
            alice.next_event().await.unwrap(),
            Event::Message(b"one".to_vec())
        );
        (alice, bob)
    });

    // Kill -9: the relay's runtime — accept loop, every connection task, the
    // lot — is dropped without any shutdown handshake.
    drop(relay_rt);

    // Restart from disk on the same address with the same identity.
    let relay_rt = Runtime::new().unwrap();
    let rebound = boot_relay(&relay_rt, dir.path(), &addr);
    assert_eq!(rebound, addr);

    // The conversation resumes: sends trigger the engines' reconnect loops,
    // the pinned fingerprint still matches, the relay still knows every
    // queue, send key, and epoch. Nothing was re-paired.
    client_rt.block_on(async {
        bob.send_message(b"two").await.unwrap();
        pump_until_message(&mut alice, b"two").await;
        alice.send_message(b"three").await.unwrap();
        pump_until_message(&mut bob, b"three").await;
    });
}
