//! T21: kill the `directory` process mid-test, confirm relay/ message
//! delivery is unaffected. Real subprocess kill (SIGKILL via
//! `Child::kill()`), not an in-process stand-in — `directory` is started as
//! the actual compiled binary (`CARGO_BIN_EXE_directory`), same as a real
//! deployment would run it.
//!
//! `engine`/`relay` are dev-dependencies here only, to drive a real chat
//! exchange as the thing being proven unaffected — this does not change
//! directory's production dependency graph (T1's constraint is about
//! `[dependencies]`, checked via `cargo tree -e normal`).

use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

use engine::{ChatEngine, Event};
use relay::{RelayIdentity, RelayState};
use tokio::runtime::Runtime;

fn boot_relay(rt: &Runtime, dir: &std::path::Path) -> (String, [u8; 32]) {
    let identity = RelayIdentity::load_or_create(&dir.join("relay_identity.der")).unwrap();
    let fingerprint = identity.fingerprint;
    let state = Arc::new(RelayState::open(&dir.join("relay_state.sqlite3")).unwrap());
    let addr = rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            relay::net::serve_on(listener, state, &identity).await.ok();
        });
        bound
    });
    (addr, fingerprint)
}

/// Spawns the real `directory` binary against the live Postgres this test
/// suite already runs against (`DATABASE_URL`), waits for its health
/// endpoint to answer, and returns the child handle plus its address.
fn spawn_directory_subprocess() -> (Child, String) {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a running Postgres for this test");
    // Port 0 isn't usable here — this test can't introspect the bound port
    // of a subprocess it doesn't control the stdout of. Pick a high,
    // likely-free fixed port instead of plumbing that back.
    let port = 58080 + (std::process::id() % 500);
    let addr = format!("127.0.0.1:{port}");
    let bin = env!("CARGO_BIN_EXE_directory");
    let child = Command::new(bin)
        .env("DATABASE_URL", &database_url)
        .env("DIRECTORY_ALLOW_DEV_PEPPER", "1")
        .env("DIRECTORY_ADDR", &addr)
        .spawn()
        .expect("failed to spawn the directory binary");
    (child, addr)
}

fn wait_for_health(addr: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if reqwest::blocking::get(format!("http://{addr}/health"))
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("directory subprocess never became healthy at {addr}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

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
fn killing_directory_mid_conversation_does_not_affect_relay_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let relay_rt = Runtime::new().unwrap();
    let (addr, fingerprint) = boot_relay(&relay_rt, dir.path());

    let client_rt = Runtime::new().unwrap();
    let (mut alice, mut bob) = client_rt.block_on(async {
        let mut alice = ChatEngine::connect("alice", &addr, fingerprint)
            .await
            .unwrap();
        let link = alice.contact_link().unwrap();
        let mut bob = ChatEngine::pair_with_link("bob", &link).await.unwrap();
        alice.await_pairing().await.unwrap();
        bob.send_message(b"before directory dies").await.unwrap();
        assert_eq!(
            alice.next_event().await.unwrap(),
            Event::Message(b"before directory dies".to_vec())
        );
        (alice, bob)
    });

    let (mut directory_child, directory_addr) = spawn_directory_subprocess();
    wait_for_health(&directory_addr);

    // The kill: SIGKILL via Child::kill(), no shutdown handshake — the
    // closest real-process equivalent of `kill -9`.
    directory_child
        .kill()
        .expect("failed to kill directory subprocess");
    directory_child
        .wait()
        .expect("failed to reap directory subprocess");

    // Relay message delivery must be completely unaffected — they share no
    // process, no socket, no state.
    client_rt.block_on(async {
        alice.send_message(b"after directory died").await.unwrap();
        pump_until_message(&mut bob, b"after directory died").await;
        bob.send_message(b"still fine").await.unwrap();
        pump_until_message(&mut alice, b"still fine").await;
    });
}
