//! T5/T8 at the FFI boundary: `new_persistent` + process death + resume.
//! Same no-mocks stack as callback_delivery.rs — real relay, real MLS, real
//! SQLCipher file — but the listen-side engine is dropped mid-conversation
//! and rebuilt from its database: conversation list and message history are
//! back immediately, the backlog that arrived while dead is delivered, and
//! both directions still flow. Plus the loud-refusal contract: a wrong key
//! is `StorageFailed`, never silently fresh state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chat_ffi::{ChatEngine, EngineError, EngineEvent, EngineEventListener};
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

#[derive(Default)]
struct Collector {
    events: Arc<Mutex<Vec<EngineEvent>>>,
}

impl EngineEventListener for Collector {
    fn on_event(&self, event: EngineEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn wait_for_message(events: &Arc<Mutex<Vec<EngineEvent>>>, expected_body: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        {
            let events = events.lock().unwrap();
            for event in events.iter() {
                if let EngineEvent::MessageReceived { message, .. } = event {
                    if message.body == expected_body {
                        return;
                    }
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for MessageReceived({expected_body:?})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn persistent_engine_resumes_with_history_and_backlog() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (relay_addr, fingerprint) = rt.block_on(start_relay());
    std::env::set_var("CHAT_RELAY_ADDR", &relay_addr);
    std::env::set_var(
        "CHAT_RELAY_FINGERPRINT",
        fingerprint
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("bob.db").to_str().unwrap().to_string();

    // First life: pair and exchange a message.
    let bob =
        ChatEngine::new_persistent("bob".to_string(), db_path.clone(), "bob's key".into()).unwrap();
    let bob_events: Arc<Mutex<Vec<EngineEvent>>> = Arc::default();
    bob.set_listener(Box::new(Collector {
        events: bob_events.clone(),
    }));
    let link = bob.create_contact_link();
    assert!(!link.is_empty());

    let alice = ChatEngine::new("alice".to_string());
    let conv = alice.pair_with_link(link).unwrap();
    wait_until("bob to see the conversation", || {
        !bob.conversations().is_empty()
    });
    let bob_conv = bob.conversations()[0].id.clone();

    alice.send(conv.clone(), "hello".to_string()).unwrap();
    wait_for_message(&bob_events, "hello");

    // Process death: every in-memory structure gone, DB file remains.
    drop(bob);

    // Arrives while bob is dead — queued unacked on the relay.
    alice
        .send(conv.clone(), "while you were down".to_string())
        .unwrap();

    // Second life: resumed from disk. Conversation list and history are
    // available synchronously, before any network round-trip.
    let bob =
        ChatEngine::new_persistent("bob".to_string(), db_path.clone(), "bob's key".into()).unwrap();
    let convs = bob.conversations();
    assert_eq!(convs.len(), 1, "resumed conversation list must be loaded");
    assert_eq!(convs[0].display_name, "alice");
    let history: Vec<String> = bob
        .messages(bob_conv.clone())
        .iter()
        .map(|m| m.body.clone())
        .collect();
    assert!(
        history.contains(&"hello".to_string()),
        "pre-crash history must survive: {history:?}"
    );

    let bob_events: Arc<Mutex<Vec<EngineEvent>>> = Arc::default();
    bob.set_listener(Box::new(Collector {
        events: bob_events.clone(),
    }));
    wait_for_message(&bob_events, "while you were down");

    // Both directions on the resumed MLS state.
    let alice_events: Arc<Mutex<Vec<EngineEvent>>> = Arc::default();
    alice.set_listener(Box::new(Collector {
        events: alice_events.clone(),
    }));
    bob.send(bob_conv, "back from the dead".to_string())
        .unwrap();
    wait_for_message(&alice_events, "back from the dead");
}

#[test]
fn wrong_key_is_a_loud_error() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db").to_str().unwrap().to_string();
    drop(ChatEngine::new_persistent("bob".to_string(), db_path.clone(), "right".into()).unwrap());
    assert!(matches!(
        ChatEngine::new_persistent("bob".to_string(), db_path, "wrong".into()),
        Err(EngineError::StorageFailed { .. })
    ));
}
