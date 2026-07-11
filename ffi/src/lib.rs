//! The FFI contract between the Rust backend and the Kotlin Multiplatform app
//! (design doc D4: thick Rust core, high-level UniFFI surface). This crate is
//! the ONE boundary the two teams share:
//!
//!   - Backend owns this file and freezes the exported signatures. The
//!     internals below are an in-memory STUB so the app runs today; backend
//!     swaps them for the real `engine/` (crate extraction, T6) without
//!     changing a single exported signature. Everything the UI calls stays
//!     put while TLS, request-ids, and reconnect land underneath.
//!   - Frontend owns `app/engine-kt` (Gobley) which generates Kotlin bindings
//!     from these exports, and `app/composeApp` which calls them.
//!
//! Deliberately transport-free: nothing here mentions relays, epochs, MLS, or
//! sockets. That is why the contract survives the backend hardening work — the
//! UI only ever sees conversations, messages, and events.
//!
//! STUB BEHAVIOR (so the frontend can build every wireframe screen against
//! real bindings): pairing creates a fake conversation; `send` stores the
//! message and echoes a canned reply back through the listener. No network,
//! no crypto. Swap-out point is marked `// STUB` throughout.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

uniffi::setup_scaffolding!();

/// A conversation as the UI lists it. `id` is opaque to the UI — pass it back
/// to `send`/`messages`/`mark_verified`.
#[derive(uniffi::Record, Clone)]
pub struct Conversation {
    pub id: String,
    pub display_name: String,
    pub last_message: Option<String>,
    pub unread: u32,
    /// True once the user has confirmed the safety-number words (wireframe-v1
    /// frame C). Drives the quiet ✓ next to the name — never a scary warning.
    pub verified: bool,
}

/// One message in a conversation.
#[derive(uniffi::Record, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub body: String,
    /// True if this device authored it (right-aligned bubble).
    pub mine: bool,
    pub timestamp_ms: i64,
    /// UI maps this to the bubble/state treatment (wireframe-v1 frame D).
    pub delivery: DeliveryState,
}

#[derive(uniffi::Enum, Clone, PartialEq, Eq)]
pub enum DeliveryState {
    /// Composed, not yet accepted by the relay ("Not sent yet · tap to retry").
    Pending,
    /// Left this device.
    Sent,
    /// A send that failed and can be retried.
    Failed,
    /// Inbound message.
    Received,
}

/// Events the engine pushes to the UI. The UI registers one listener via
/// `set_listener`; Kotlin wraps it into a Flow.
#[derive(uniffi::Enum, Clone)]
pub enum EngineEvent {
    /// A new inbound message landed in a conversation.
    MessageReceived {
        conversation: String,
        message: ChatMessage,
    },
    /// A conversation's list metadata changed (last message, unread, verified).
    ConversationUpdated { conversation: String },
    /// Relay connection came up or went down (drives the calm reconnect banner,
    /// wireframe-v1 frame D). Never blocks composing.
    ConnectionStateChanged { online: bool },
    /// A peer's security code changed (the quiet "Review" system line).
    SecurityCodeChanged { conversation: String },
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum EngineError {
    #[error("contact link is malformed or unsupported")]
    InvalidContactLink,
    #[error("no such conversation")]
    UnknownConversation,
    #[error("send failed: {reason}")]
    SendFailed { reason: String },
}

/// The UI implements this to receive pushes. `on_event` may be called from a
/// dedicated dispatch thread (design doc FFI threading contract) — it must not
/// block for long.
#[uniffi::export(callback_interface)]
pub trait EngineEventListener: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// STUB state. The real engine replaces this whole block with a handle to the
// extracted `engine/` crate; the exported API above does not change.
#[derive(Default)]
struct StubState {
    conversations: Vec<Conversation>,
    messages: HashMap<String, Vec<ChatMessage>>,
}

/// The one object the UI holds. Created once at app start.
#[derive(uniffi::Object)]
pub struct ChatEngine {
    display_name: String,
    state: Mutex<StubState>,
    listener: Mutex<Option<Box<dyn EngineEventListener>>>,
    seq: AtomicU64,
}

#[uniffi::export]
impl ChatEngine {
    /// Create the engine. `display_name` is local decoration only — never a
    /// stable network identifier (design doc: no accounts, no phone numbers).
    #[uniffi::constructor]
    pub fn new(display_name: String) -> Arc<Self> {
        Arc::new(ChatEngine {
            display_name,
            state: Mutex::new(StubState::default()),
            listener: Mutex::new(None),
            seq: AtomicU64::new(1),
        })
    }

    /// Register (or replace) the event listener. Call once after construction.
    pub fn set_listener(&self, listener: Box<dyn EngineEventListener>) {
        *self.listener.lock().unwrap() = Some(listener);
    }

    /// The base64 contact-link string this device's QR code encodes
    /// (wireframe-v1 frame B). STUB: a placeholder token, not a real link.
    pub fn create_contact_link(&self) -> String {
        // STUB: real impl builds a proto::ContactLink (KeyPackage + relay
        // endpoint) and base64-encodes it.
        format!("chat-contact-stub:{}", self.display_name)
    }

    /// Consume a scanned/pasted contact link and start a conversation. Returns
    /// the new conversation id. STUB: fabricates a peer named from the link.
    pub fn pair_with_link(&self, link: String) -> Result<String, EngineError> {
        if link.trim().is_empty() {
            return Err(EngineError::InvalidContactLink);
        }
        let id = self.next_id("conv");
        let peer = link
            .strip_prefix("chat-contact-stub:")
            .unwrap_or("New contact")
            .to_string();
        {
            let mut s = self.state.lock().unwrap();
            s.conversations.push(Conversation {
                id: id.clone(),
                display_name: peer,
                last_message: None,
                unread: 0,
                verified: false,
            });
            s.messages.insert(id.clone(), Vec::new());
        }
        self.emit(EngineEvent::ConversationUpdated {
            conversation: id.clone(),
        });
        Ok(id)
    }

    /// All conversations, newest activity first (UI sorts as it likes).
    pub fn conversations(&self) -> Vec<Conversation> {
        self.state.lock().unwrap().conversations.clone()
    }

    /// Messages in a conversation, oldest first.
    pub fn messages(&self, conversation: String) -> Vec<ChatMessage> {
        self.state
            .lock()
            .unwrap()
            .messages
            .get(&conversation)
            .cloned()
            .unwrap_or_default()
    }

    /// Send a text message. STUB: stores it as Sent, then echoes a canned
    /// reply back through the listener so the UI has round-trip behavior.
    pub fn send(&self, conversation: String, body: String) -> Result<(), EngineError> {
        let mine = ChatMessage {
            id: self.next_id("msg"),
            body: body.clone(),
            mine: true,
            timestamp_ms: now_ms(),
            delivery: DeliveryState::Sent,
        };
        {
            let mut s = self.state.lock().unwrap();
            let msgs = s
                .messages
                .get_mut(&conversation)
                .ok_or(EngineError::UnknownConversation)?;
            msgs.push(mine);
            if let Some(c) = s.conversations.iter_mut().find(|c| c.id == conversation) {
                c.last_message = Some(body.clone());
            }
        }
        // STUB: canned inbound echo. Real impl delivers peer messages off the
        // relay fan-out.
        let reply = ChatMessage {
            id: self.next_id("msg"),
            body: format!("echo: {body}"),
            mine: false,
            timestamp_ms: now_ms(),
            delivery: DeliveryState::Received,
        };
        {
            let mut s = self.state.lock().unwrap();
            if let Some(msgs) = s.messages.get_mut(&conversation) {
                msgs.push(reply.clone());
            }
        }
        self.emit(EngineEvent::MessageReceived {
            conversation,
            message: reply,
        });
        Ok(())
    }

    /// Mark a conversation verified after the user confirms the safety-number
    /// words. Idempotent.
    pub fn mark_verified(&self, conversation: String) -> Result<(), EngineError> {
        let mut s = self.state.lock().unwrap();
        let c = s
            .conversations
            .iter_mut()
            .find(|c| c.id == conversation)
            .ok_or(EngineError::UnknownConversation)?;
        c.verified = true;
        drop(s);
        self.emit(EngineEvent::ConversationUpdated { conversation });
        Ok(())
    }
}

impl ChatEngine {
    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.seq.fetch_add(1, Ordering::Relaxed))
    }

    fn emit(&self, event: EngineEvent) {
        if let Some(l) = self.listener.lock().unwrap().as_ref() {
            l.on_event(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct CountingListener {
        count: Arc<AtomicUsize>,
    }
    impl EngineEventListener for CountingListener {
        fn on_event(&self, _event: EngineEvent) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn pair_send_roundtrip_fires_events_and_records_messages() {
        let engine = ChatEngine::new("alice".to_string());
        let count = Arc::new(AtomicUsize::new(0));
        engine.set_listener(Box::new(CountingListener {
            count: count.clone(),
        }));

        let conv = engine
            .pair_with_link(engine.create_contact_link())
            .expect("pairing with a well-formed link succeeds");
        assert_eq!(engine.conversations().len(), 1);

        engine.send(conv.clone(), "hi".to_string()).unwrap();
        // Stub records the sent message plus its echo reply.
        let msgs = engine.messages(conv.clone());
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].mine && msgs[0].delivery == DeliveryState::Sent);
        assert!(!msgs[1].mine && msgs[1].delivery == DeliveryState::Received);

        engine.mark_verified(conv.clone()).unwrap();
        assert!(engine.conversations()[0].verified);

        // Events fired: pair (ConversationUpdated) + send (MessageReceived) +
        // verify (ConversationUpdated) = 3.
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn empty_link_and_unknown_conversation_are_errors() {
        let engine = ChatEngine::new("bob".to_string());
        assert!(matches!(
            engine.pair_with_link("  ".to_string()),
            Err(EngineError::InvalidContactLink)
        ));
        assert!(matches!(
            engine.send("nope".to_string(), "x".to_string()),
            Err(EngineError::UnknownConversation)
        ));
    }
}
