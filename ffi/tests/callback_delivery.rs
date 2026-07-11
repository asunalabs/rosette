//! The T7 verification bar (architecture.md step 3): register an
//! EngineEventListener, drive one message through a LOOPBACK RELAY, and
//! assert the callback fires with the decrypted payload — from the dedicated
//! dispatch thread (review OV8), never a tokio worker.
//!
//! This is the full stack the app will ship: FFI surface → engine actor →
//! MLS + TLS + relay wire → fan-out → engine events → dispatch thread →
//! listener callback. No mocks anywhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chat_ffi::{ChatEngine, DeliveryState, EngineEvent, EngineEventListener};
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

/// Records every event plus proof of WHICH thread delivered it.
#[derive(Default)]
struct Collector {
    events: Arc<Mutex<Vec<EngineEvent>>>,
    wrong_thread: Arc<AtomicBool>,
}

impl EngineEventListener for Collector {
    fn on_event(&self, event: EngineEvent) {
        if std::thread::current().name() != Some("chat-ffi-dispatch") {
            self.wrong_thread.store(true, Ordering::Relaxed);
        }
        self.events.lock().unwrap().push(event);
    }
}

fn wait_for_message(events: &Arc<Mutex<Vec<EngineEvent>>>, expected_body: &str) -> EngineEvent {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        {
            let events = events.lock().unwrap();
            for event in events.iter() {
                if let EngineEvent::MessageReceived { message, .. } = event {
                    if message.body == expected_body {
                        return event.clone();
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

#[test]
fn callback_delivers_decrypted_payload_from_dispatch_thread() {
    // The FFI surface is sync (that's the point), so the test is a plain
    // #[test]; only the loopback relay needs a runtime of its own.
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

    // Alice: listen side. Her QR/link comes off the env-configured relay.
    let alice = ChatEngine::new("alice".to_string());
    let alice_collector = Collector::default();
    let alice_events = alice_collector.events.clone();
    let alice_wrong_thread = alice_collector.wrong_thread.clone();
    alice.set_listener(Box::new(alice_collector));
    let link = alice.create_contact_link();
    assert!(!link.is_empty(), "link creation against a live relay works");

    // Bob: scan side. His relay comes from the link — no env needed.
    let bob = ChatEngine::new("bob".to_string());
    let bob_collector = Collector::default();
    let bob_events = bob_collector.events.clone();
    bob.set_listener(Box::new(bob_collector));
    let conv_bob = bob.pair_with_link(link).expect("pairing succeeds");

    // One message through the real stack: encrypted by Bob's MLS session,
    // relayed over TLS, fanned out, decrypted by Alice's, surfaced as a
    // callback on the dispatch thread.
    bob.send(conv_bob.clone(), "hello through the real stack".to_string())
        .expect("send over the live relay succeeds");
    let event = wait_for_message(&alice_events, "hello through the real stack");
    let EngineEvent::MessageReceived {
        conversation,
        message,
    } = event
    else {
        unreachable!("wait_for_message only returns MessageReceived");
    };
    assert!(!message.mine);
    assert_eq!(message.delivery, DeliveryState::Received);

    // The store agrees with the callback, and pairing named the peer from
    // the MLS credential.
    let alice_convs = alice.conversations();
    assert_eq!(alice_convs.len(), 1);
    assert_eq!(alice_convs[0].id, conversation);
    assert_eq!(alice_convs[0].display_name, "bob");
    assert_eq!(alice_convs[0].unread, 1);
    let alice_msgs = alice.messages(conversation.clone());
    assert_eq!(alice_msgs.len(), 1);
    assert_eq!(alice_msgs[0].body, "hello through the real stack");

    // And the reverse direction, exercising Alice's send path.
    alice
        .send(conversation, "hi back".to_string())
        .expect("reply succeeds");
    wait_for_message(&bob_events, "hi back");
    let bob_convs = bob.conversations();
    assert_eq!(bob_convs[0].display_name, "alice");

    // OV8: not a single callback came from anywhere but the dispatch thread.
    assert!(
        !alice_wrong_thread.load(Ordering::Relaxed),
        "callbacks must only ever run on chat-ffi-dispatch"
    );

    // mark_verified round-trips through the same store the UI reads.
    bob.mark_verified(conv_bob.clone()).unwrap();
    assert!(bob.conversations()[0].verified);
}
