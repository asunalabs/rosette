//! The FFI contract between the Rust backend and the Kotlin Multiplatform app
//! (design doc D4: thick Rust core, high-level UniFFI surface). This crate is
//! the ONE boundary the two teams share:
//!
//!   - Backend owns this file and freezes the exported signatures. As of T7
//!     the in-memory stub is gone: every call below drives the real
//!     `engine/` crate (MLS, TLS relay connection, reconnect, dedup,
//!     epoch-conflict retry). The exported signatures did not change;
//!     `EngineError` gained two ADDITIVE variants (`RelayUnreachable`,
//!     `NotSupported`) — new exception subclasses on the Kotlin side,
//!     non-breaking for existing code.
//!   - Frontend owns `app/engine-kt` (Gobley) which generates Kotlin bindings
//!     from these exports, and `app/composeApp` which calls them.
//!
//! Deliberately transport-free: nothing here mentions relays, epochs, MLS, or
//! sockets — except one bootstrap knob: the home relay for `create_contact_link`
//! comes from the `CHAT_RELAY_ADDR` + `CHAT_RELAY_FINGERPRINT` (hex) env vars
//! (relay address is not user-editable in v1; a baked-in production default
//! lands when one exists). `pair_with_link` needs no config — the link carries
//! its relay.
//!
//! THREADING (review OV8): the engine lives on its own dedicated thread with
//! its own tokio runtime, created lazily on first use. Listener callbacks are
//! NEVER invoked from tokio worker threads — events drain through a channel
//! consumed by the dedicated `chat-ffi-dispatch` thread, so a slow/blocking
//! Kotlin handler can stall at most event delivery, never the engine.
//!
//! v0.1 scope: ONE conversation per engine (the pairing produces it). The
//! multi-conversation surface is already shaped for more.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chatcore::Store as DiskStore;
use engine::ChatEngine as CoreEngine;
use tokio::sync::{mpsc, oneshot};

uniffi::setup_scaffolding!();

/// A conversation as the UI lists it. `id` is opaque to the UI — pass it back
/// to `send`/`messages`/`mark_verified`.
#[derive(uniffi::Record, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(uniffi::Record, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub body: String,
    /// True if this device authored it (right-aligned bubble).
    pub mine: bool,
    pub timestamp_ms: i64,
    /// UI maps this to the bubble/state treatment (wireframe-v1 frame D).
    pub delivery: DeliveryState,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// T27: one directory-issued attestation token as the app carries it — the
/// Kotlin `DirectoryClient.fetchAttestationTokens` decodes the base64 wire
/// fields into these, then hands a batch to `stock_attestation_tokens`. The
/// engine spends one per queue-creation; the relay verifies it offline.
#[derive(uniffi::Record, Clone)]
pub struct AttestationToken {
    /// 16-byte nonce. A wrong length makes the whole token unusable, so
    /// `stock_attestation_tokens` drops it rather than trusting it.
    pub nonce: Vec<u8>,
    pub expires_at: i64,
    pub signature: Vec<u8>,
}

/// Drop a token whose nonce isn't exactly 16 bytes — a malformed token could
/// never verify at the relay, so it is worthless, not dangerous.
fn attestation_to_proto(t: AttestationToken) -> Option<proto::attestation::AttestationToken> {
    let nonce: [u8; 16] = t.nonce.try_into().ok()?;
    Some(proto::attestation::AttestationToken {
        nonce,
        expires_at: t.expires_at,
        signature: t.signature,
    })
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

/// Recovery bundle as uploaded to the directory (issue #2). Field-for-field
/// mirror of `chatcore::backup::BackupBundle`; every field is ciphertext, a
/// salt, or a hash — safe to hand to the server.
#[derive(uniffi::Record, Clone)]
pub struct BackupBundle {
    pub blob: Vec<u8>,
    pub w_pin: Vec<u8>,
    pub salt_p: Vec<u8>,
    pub w_phrase: Vec<u8>,
    pub salt_f: Vec<u8>,
    pub auth_pin: Vec<u8>,
    pub salt_a: Vec<u8>,
    pub auth_phrase: Vec<u8>,
    pub salt_pa: Vec<u8>,
}

impl From<chatcore::backup::BackupBundle> for BackupBundle {
    fn from(b: chatcore::backup::BackupBundle) -> Self {
        BackupBundle {
            blob: b.blob,
            w_pin: b.w_pin,
            salt_p: b.salt_p,
            w_phrase: b.w_phrase,
            salt_f: b.salt_f,
            auth_pin: b.auth_pin,
            salt_a: b.salt_a,
            auth_phrase: b.auth_phrase,
            salt_pa: b.salt_pa,
        }
    }
}

impl From<BackupBundle> for chatcore::backup::BackupBundle {
    fn from(b: BackupBundle) -> Self {
        chatcore::backup::BackupBundle {
            blob: b.blob,
            w_pin: b.w_pin,
            salt_p: b.salt_p,
            w_phrase: b.w_phrase,
            salt_f: b.salt_f,
            auth_pin: b.auth_pin,
            salt_a: b.salt_a,
            auth_phrase: b.auth_phrase,
            salt_pa: b.salt_pa,
        }
    }
}

/// Issue #3: SHA256(Argon2id(secret, salt)) — the proof the directory's
/// restore endpoint compares against its stored auth hash. Runs one Argon2id
/// derivation (seconds by design); call off the UI thread. Phrase input is
/// normalized exactly like `new_from_backup` normalizes it.
#[uniffi::export]
pub fn backup_auth_proof(secret: String, salt: Vec<u8>) -> Vec<u8> {
    let secret = if chatcore::backup::validate_pin(&secret) {
        secret
    } else {
        chatcore::backup::normalize_phrase(&secret)
    };
    chatcore::backup::auth_hash(&secret, &salt)
}

/// One-time result of `backup_enroll`. The phrase is shown to the user once
/// and never persisted in plaintext anywhere — here or on the server.
#[derive(uniffi::Record)]
pub struct BackupEnrollment {
    pub phrase: String,
    pub bundle: BackupBundle,
}

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum EngineError {
    #[error("contact link is malformed or unsupported")]
    InvalidContactLink,
    #[error("no such conversation")]
    UnknownConversation,
    #[error("send failed: {reason}")]
    SendFailed { reason: String },
    /// ADDITIVE (T7): the relay could not be reached. Distinct from
    /// `InvalidContactLink` so the UI can say "check your connection" instead
    /// of "bad code".
    #[error("relay unreachable: {reason}")]
    RelayUnreachable { reason: String },
    /// ADDITIVE (T7): the operation is outside v0.1's scope (e.g. pairing a
    /// second conversation).
    #[error("not supported: {reason}")]
    NotSupported { reason: String },
    /// ADDITIVE (T5/T8 persistence): the encrypted on-device database could
    /// not be opened — wrong key, or an unreadable/corrupt file. Never
    /// silently answered with fresh state: losing the key means losing the
    /// data, and the UI must say so.
    #[error("storage failed: {reason}")]
    StorageFailed { reason: String },
    /// ADDITIVE (issue #2): the recovery PIN failed validation — it must be
    /// 4-6 ASCII digits.
    #[error("PIN must be 4-6 digits")]
    InvalidPin,
    /// ADDITIVE (issue #3): the PIN or phrase given to `new_from_backup`
    /// does not open this bundle. The AEAD tag cannot say which part was
    /// wrong, and no database is left behind.
    #[error("wrong PIN or recovery phrase")]
    WrongRecoverySecret,
}

/// The UI implements this to receive pushes. `on_event` is always called from
/// the dedicated `chat-ffi-dispatch` thread (review OV8) — never from a tokio
/// worker — so it may block briefly without stalling the engine, but should
/// still hand off to the UI loop quickly.
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

/// v0.1: one conversation per engine, so its id is fixed.
const CONVERSATION_ID: &str = "conv-1";

#[derive(Clone)]
struct RelayConfig {
    addr: String,
    fingerprint: [u8; 32],
}

fn relay_config_from_env() -> Option<RelayConfig> {
    let addr = std::env::var("CHAT_RELAY_ADDR").ok()?;
    let hex = std::env::var("CHAT_RELAY_FINGERPRINT").ok()?;
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut fingerprint = [0u8; 32];
    for (i, byte) in fingerprint.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(RelayConfig {
        addr,
        fingerprint: fingerprint,
    })
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Store {
    conversations: Vec<Conversation>,
    messages: HashMap<String, Vec<ChatMessage>>,
}

/// SQLCipher location + key, kept so the engine thread can open its own
/// connection when it spawns. The key is the caller's problem to derive
/// (platform keystore) — it only transits memory here.
#[derive(Clone)]
struct PersistCfg {
    path: String,
    key: String,
}

enum DispatchMsg {
    Event(EngineEvent),
    ListenerChanged,
}

/// State shared between the FFI object, the engine actor thread, and the
/// dispatch thread.
struct Shared {
    store: Mutex<Store>,
    dispatch_tx: std::sync::mpsc::Sender<DispatchMsg>,
    seq: AtomicU64,
    /// UI-history half of the same SQLCipher database the engine writes its
    /// MLS state into (its own connection — the engine's lives on the engine
    /// thread). None = in-memory engine.
    disk: Mutex<Option<DiskStore>>,
    /// T27: attestation tokens waiting to be handed to the engine when it
    /// spawns (the mailbox/group-inbox spend points live inside the engine
    /// constructors). Drained at spawn; empty when the gate is off.
    attestation: Mutex<Vec<proto::attestation::AttestationToken>>,
}

impl Shared {
    fn dispatch(&self, event: EngineEvent) {
        let _ = self.dispatch_tx.send(DispatchMsg::Event(event));
    }

    fn next_msg_id(&self) -> String {
        format!("msg-{}", self.seq.fetch_add(1, Ordering::Relaxed))
    }

    /// Write the whole UI state (conversations, messages, id counter) to the
    /// encrypted store. No-op for in-memory engines; best-effort otherwise —
    /// the engine's own MLS/seen persistence never depends on this blob.
    // ponytail: whole-state blob per mutation, one kv row — fine at
    // walking-shell scale; per-message rows when history grows real.
    fn persist_ui(&self) {
        let mut disk = self.disk.lock().unwrap();
        let Some(disk) = disk.as_mut() else { return };
        let blob = {
            let store = self.store.lock().unwrap();
            bincode::serialize(&(&*store, self.seq.load(Ordering::Relaxed)))
                .expect("ui state is always serializable")
        };
        let _ = disk.commit(&[("ui.state", &blob)], &[]);
    }
}

/// Requests crossing from FFI callers into the engine actor thread.
enum Command {
    CreateLink {
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    Send {
        body: String,
        reply: oneshot::Sender<Result<(), EngineError>>,
    },
    /// DT6: the verify-ceremony safety number, read from the live session.
    SafetyNumber {
        reply: oneshot::Sender<Result<String, EngineError>>,
    },
}

/// The one object the UI holds. Created once at app start.
#[derive(uniffi::Object)]
pub struct ChatEngine {
    display_name: String,
    relay_cfg: Option<RelayConfig>,
    persist: Option<PersistCfg>,
    shared: Arc<Shared>,
    listener: Arc<Mutex<Option<Box<dyn EngineEventListener>>>>,
    /// Command channel to the engine actor thread; None until the first
    /// operation that needs a live engine spawns it.
    backend: Mutex<Option<mpsc::Sender<Command>>>,
}

#[uniffi::export]
impl ChatEngine {
    /// Create the engine. `display_name` is local decoration only — never a
    /// stable network identifier (design doc: no accounts, no phone numbers).
    /// In-memory: nothing survives process death. The app ships
    /// `new_persistent`; this stays for tests and throwaway sessions.
    #[uniffi::constructor]
    pub fn new(display_name: String) -> Arc<Self> {
        Self::build(display_name, None, None, Store::default(), 1)
    }

    /// ADDITIVE (T5/T8): create the engine backed by the SQLCipher database
    /// at `db_path`, keyed with `db_key` (derive it from the platform
    /// keystore — it is never stored). First run creates the database; later
    /// runs RESUME from it: identity, pairing, MLS state, and message
    /// history all survive process death, and the engine reconnects in the
    /// background (watch `ConnectionStateChanged`). A wrong key fails loudly
    /// with `StorageFailed` — never with silently fresh state.
    #[uniffi::constructor]
    pub fn new_persistent(
        display_name: String,
        db_path: String,
        db_key: String,
    ) -> Result<Arc<Self>, EngineError> {
        let disk = DiskStore::open(Path::new(&db_path), &db_key).map_err(|e| {
            EngineError::StorageFailed {
                reason: e.to_string(),
            }
        })?;
        let map_err = |e: chatcore::StoreError| EngineError::StorageFailed {
            reason: e.to_string(),
        };
        let has_engine_state = disk.get("engine").map_err(map_err)?.is_some();
        let (ui_state, seq) = match disk.get("ui.state").map_err(map_err)? {
            Some(blob) => bincode::deserialize(&blob).map_err(|e| EngineError::StorageFailed {
                reason: format!("ui state blob is corrupt: {e}"),
            })?,
            None => (Store::default(), 1),
        };

        let cfg = PersistCfg {
            path: db_path,
            key: db_key,
        };
        let engine = Self::build(display_name, Some(cfg.clone()), Some(disk), ui_state, seq);
        if has_engine_state {
            // A paired engine lives in the store: resume it in the
            // background (reconnect loops there; the constructor must not
            // block app startup on the network).
            let cmd_tx = spawn_resume(engine.shared.clone(), cfg);
            *engine.backend.lock().unwrap() = Some(cmd_tx);
        }
        Ok(engine)
    }

    /// ADDITIVE (issue #3): create the engine by restoring an account from
    /// its recovery bundle on a NEW device. `secret` is the PIN or the
    /// 5-word phrase (normalized here). Unwraps BK, decrypts the blob, and
    /// seeds a fresh SQLCipher database at `db_path` with the restored
    /// identity, username, contacts, and recovery material — then behaves
    /// exactly like `new_persistent`. A wrong secret fails with
    /// `WrongRecoverySecret` and leaves no database behind. Runs an Argon2id
    /// derivation — call from a background thread.
    #[uniffi::constructor]
    pub fn new_from_backup(
        display_name: String,
        db_path: String,
        db_key: String,
        bundle: BackupBundle,
        secret: String,
    ) -> Result<Arc<Self>, EngineError> {
        let core_bundle: chatcore::backup::BackupBundle = bundle.into();
        // Unwrap BK before touching the filesystem: a wrong secret must
        // leave no trace. PIN-shaped secrets try the PIN wrap, everything
        // else the phrase wrap.
        let bk = if chatcore::backup::validate_pin(&secret) {
            chatcore::backup::unwrap_bk(&secret, &core_bundle.salt_p, &core_bundle.w_pin)
        } else {
            let phrase = chatcore::backup::normalize_phrase(&secret);
            chatcore::backup::unwrap_bk(&phrase, &core_bundle.salt_f, &core_bundle.w_phrase)
        }
        .map_err(|_| EngineError::WrongRecoverySecret)?;
        let payload_bytes = chatcore::backup::open_blob(&bk, &core_bundle.blob)
            .map_err(|_| EngineError::WrongRecoverySecret)?;
        let payload: chatcore::backup::BackupPayload = bincode::deserialize(&payload_bytes)
            .map_err(|e| EngineError::StorageFailed {
                reason: format!("backup blob is corrupt: {e}"),
            })?;

        if Path::new(&db_path).exists() {
            return Err(EngineError::StorageFailed {
                reason: "a database already exists at this path".to_string(),
            });
        }
        let mut disk = DiskStore::open(Path::new(&db_path), &db_key).map_err(|e| {
            EngineError::StorageFailed {
                reason: e.to_string(),
            }
        })?;

        let name = payload.username.clone().unwrap_or(display_name);
        let mut ui_state = Store::default();
        if let Some(first) = payload.contacts.first() {
            // ponytail: v0.1 single-conversation model — restore surfaces
            // the contact as an unpaired stub; the normal pairing flow
            // re-pairs it (create_conversation upserts by id). Widen when
            // multi-conversation lands.
            ui_state.conversations.push(Conversation {
                id: CONVERSATION_ID.to_string(),
                display_name: first.clone(),
                last_message: None,
                unread: 0,
                verified: false,
            });
            ui_state
                .messages
                .insert(CONVERSATION_ID.to_string(), Vec::new());
        }
        let ui_blob =
            bincode::serialize(&(&ui_state, 1u64)).expect("ui state is always serializable");
        let bundle_bytes = bincode::serialize(&core_bundle).expect("bundle is always serializable");

        let mut kv: Vec<(&str, &[u8])> = vec![
            ("backup.bk", &bk[..]),
            ("backup.bundle", &bundle_bytes),
            ("ui.state", &ui_blob),
        ];
        if let Some(identity) = payload.identity.as_deref() {
            kv.push(("session", identity));
        }
        if let Err(e) = disk.commit(&kv, &[]) {
            drop(disk);
            let _ = std::fs::remove_file(&db_path);
            return Err(EngineError::StorageFailed {
                reason: e.to_string(),
            });
        }

        let cfg = PersistCfg {
            path: db_path,
            key: db_key,
        };
        Ok(Self::build(name, Some(cfg), Some(disk), ui_state, 1))
    }

    /// The engine's display name. After `new_from_backup` this is the
    /// restored account's username — the app reads it back for its session.
    pub fn display_name(&self) -> String {
        self.display_name.clone()
    }

    /// Register (or replace) the event listener. Call once after construction.
    /// Events that arrived earlier are delivered immediately, in order.
    pub fn set_listener(&self, listener: Box<dyn EngineEventListener>) {
        *self.listener.lock().unwrap() = Some(listener);
        let _ = self.shared.dispatch_tx.send(DispatchMsg::ListenerChanged);
    }

    /// T27: hand the engine a batch of directory attestation tokens (from
    /// `DirectoryClient.fetchAttestationTokens`). Call BEFORE the first
    /// `create_contact_link` / `pair_with_link`, which mint the queues that
    /// spend them. A no-op when the relay's gate is off (queue creation then
    /// needs no token); malformed tokens are dropped, not trusted.
    pub fn stock_attestation_tokens(&self, tokens: Vec<AttestationToken>) {
        let mut stash = self.shared.attestation.lock().unwrap();
        stash.extend(tokens.into_iter().filter_map(attestation_to_proto));
    }
}

/// Constructor internals — uniffi::export impls only take exported methods.
impl ChatEngine {
    /// The recovery blob's plaintext: identity-only session snapshot (None
    /// until the first connect creates one — the debounced re-upload
    /// refreshes it), username (`display_name` IS the directory handle), and
    /// v0.1 contact display names.
    fn backup_payload(&self, disk: &DiskStore) -> Result<Vec<u8>, EngineError> {
        let storage_err = |reason: String| EngineError::StorageFailed { reason };
        let identity = disk
            .get("session")
            .map_err(|e| storage_err(e.to_string()))?
            .map(|b| chatcore::strip_snapshot_to_identity(&b))
            .transpose()
            .map_err(|e| storage_err(e.to_string()))?;
        let contacts = self
            .shared
            .store
            .lock()
            .unwrap()
            .conversations
            .iter()
            .map(|c| c.display_name.clone())
            .collect();
        let payload = chatcore::backup::BackupPayload {
            identity,
            username: Some(self.display_name.clone()),
            contacts,
        };
        Ok(bincode::serialize(&payload).expect("payload is always serializable"))
    }

    fn build(
        display_name: String,
        persist: Option<PersistCfg>,
        disk: Option<DiskStore>,
        ui_state: Store,
        seq: u64,
    ) -> Arc<Self> {
        let (dispatch_tx, dispatch_rx) = std::sync::mpsc::channel::<DispatchMsg>();
        let listener: Arc<Mutex<Option<Box<dyn EngineEventListener>>>> = Arc::new(Mutex::new(None));

        // OV8: the ONLY place listener callbacks ever run. Events arriving
        // before set_listener are buffered, then flushed in order.
        let dispatch_listener = listener.clone();
        std::thread::Builder::new()
            .name("chat-ffi-dispatch".to_string())
            .spawn(move || {
                let mut buffer: Vec<EngineEvent> = Vec::new();
                while let Ok(msg) = dispatch_rx.recv() {
                    match msg {
                        DispatchMsg::Event(event) => buffer.push(event),
                        DispatchMsg::ListenerChanged => {}
                    }
                    let guard = dispatch_listener.lock().unwrap();
                    if let Some(l) = guard.as_ref() {
                        for event in buffer.drain(..) {
                            l.on_event(event);
                        }
                    }
                }
            })
            .expect("spawning the dispatch thread never fails");

        Arc::new(ChatEngine {
            display_name,
            relay_cfg: relay_config_from_env(),
            persist,
            shared: Arc::new(Shared {
                store: Mutex::new(ui_state),
                dispatch_tx,
                seq: AtomicU64::new(seq),
                disk: Mutex::new(disk),
                attestation: Mutex::new(Vec::new()),
            }),
            listener,
            backend: Mutex::new(None),
        })
    }
}

#[uniffi::export]
impl ChatEngine {
    /// The base64 contact-link string this device's QR code encodes
    /// (wireframe-v1 frame B): a fresh MLS KeyPackage plus this device's
    /// bootstrap mailbox on its home relay. Returns an EMPTY string when the
    /// relay is unreachable or unconfigured (the signature is frozen and
    /// infallible) — a `ConnectionStateChanged { online: false }` event
    /// accompanies that case so the UI can show the calm banner.
    pub fn create_contact_link(&self) -> String {
        // Engine already running (e.g. link regeneration): ask it directly.
        let existing = self.backend.lock().unwrap().clone();
        if let Some(cmd_tx) = existing {
            let (reply_tx, reply_rx) = oneshot::channel();
            if cmd_tx
                .blocking_send(Command::CreateLink { reply: reply_tx })
                .is_ok()
            {
                if let Ok(Ok(link)) = reply_rx.blocking_recv() {
                    return link;
                }
            }
            self.shared
                .dispatch(EngineEvent::ConnectionStateChanged { online: false });
            return String::new();
        }

        let Some(cfg) = self.relay_cfg.clone() else {
            self.shared
                .dispatch(EngineEvent::ConnectionStateChanged { online: false });
            return String::new();
        };
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (init_tx, init_rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();
        let name = self.display_name.clone();
        let shared = self.shared.clone();
        let persist = self.persist.clone();
        // T27: hand the just-fetched tokens to the engine that's about to mint
        // this device's mailbox. Empty (gate off) → the mailbox is created with
        // no token, which the relay accepts.
        let tokens = std::mem::take(&mut *self.shared.attestation.lock().unwrap());
        std::thread::Builder::new()
            .name("chat-ffi-engine".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("building the engine runtime never fails");
                rt.block_on(async move {
                    let session = restored_session(persist.as_ref())
                        .unwrap_or_else(|| chatcore::ChatSession::new(&name));
                    let attempt = CoreEngine::connect_with_session(
                        &name,
                        &cfg.addr,
                        cfg.fingerprint,
                        session,
                        tokens,
                    )
                    .await;
                    let mut core = match attempt {
                        Ok(core) => core,
                        Err(e) => {
                            let _ = init_tx.send(Err(e));
                            return;
                        }
                    };
                    if let Err(e) = attach_disk(&mut core, persist.as_ref()) {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                    let link = match core.contact_link() {
                        Ok(link) => link,
                        Err(e) => {
                            let _ = init_tx.send(Err(e));
                            return;
                        }
                    };
                    let _ = init_tx.send(Ok(link));
                    actor_loop(core, cmd_rx, shared, None).await;
                });
            })
            .expect("spawning the engine thread never fails");

        match init_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(link)) => {
                *self.backend.lock().unwrap() = Some(cmd_tx);
                link
            }
            _ => {
                self.shared
                    .dispatch(EngineEvent::ConnectionStateChanged { online: false });
                String::new()
            }
        }
    }

    /// Consume a scanned/pasted contact link and start a conversation.
    /// Connects to the relay named IN the link, founds the 2-member MLS
    /// group, and delivers the Welcome. Returns the new conversation id.
    pub fn pair_with_link(&self, link: String) -> Result<String, EngineError> {
        if link.trim().is_empty() {
            return Err(EngineError::InvalidContactLink);
        }
        if self.backend.lock().unwrap().is_some() {
            return Err(EngineError::NotSupported {
                reason: "v0.1 supports a single conversation per engine".to_string(),
            });
        }

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (init_tx, init_rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();
        let name = self.display_name.clone();
        let shared = self.shared.clone();
        let persist = self.persist.clone();
        // T27: the scanner mints two queues (its mailbox + the group inbox), so
        // it spends up to two of these. Empty (gate off) → both created
        // token-less, which the relay accepts.
        let tokens = std::mem::take(&mut *self.shared.attestation.lock().unwrap());
        std::thread::Builder::new()
            .name("chat-ffi-engine".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("building the engine runtime never fails");
                rt.block_on(async move {
                    let session = restored_session(persist.as_ref());
                    let mut core =
                        match CoreEngine::pair_with_link_using(&name, &link, session, tokens).await
                        {
                            Ok(core) => core,
                            Err(e) => {
                                let _ = init_tx.send(Err(e));
                                return;
                            }
                        };
                    if let Err(e) = attach_disk(&mut core, persist.as_ref()) {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                    let conversation = create_conversation(&shared, core.peer_name());
                    let _ = init_tx.send(Ok(conversation.clone()));
                    actor_loop(core, cmd_rx, shared, Some(conversation)).await;
                });
            })
            .expect("spawning the engine thread never fails");

        match init_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(conversation)) => {
                *self.backend.lock().unwrap() = Some(cmd_tx);
                Ok(conversation)
            }
            Ok(Err(e)) => {
                if e.downcast_ref::<proto::LinkError>().is_some()
                    || e.downcast_ref::<base64::DecodeError>().is_some()
                {
                    Err(EngineError::InvalidContactLink)
                } else {
                    Err(EngineError::RelayUnreachable {
                        reason: e.to_string(),
                    })
                }
            }
            Err(_) => Err(EngineError::RelayUnreachable {
                reason: "timed out reaching the relay".to_string(),
            }),
        }
    }

    /// All conversations, newest activity first (UI sorts as it likes).
    pub fn conversations(&self) -> Vec<Conversation> {
        self.shared.store.lock().unwrap().conversations.clone()
    }

    /// Messages in a conversation, oldest first.
    pub fn messages(&self, conversation: String) -> Vec<ChatMessage> {
        self.shared
            .store
            .lock()
            .unwrap()
            .messages
            .get(&conversation)
            .cloned()
            .unwrap_or_default()
    }

    /// Send a text message: MLS-encrypted, relayed, end to end. Blocks until
    /// the relay accepts (or the engine gives up); failed sends are recorded
    /// with `DeliveryState::Failed` for the retry bubble.
    pub fn send(&self, conversation: String, body: String) -> Result<(), EngineError> {
        if !self
            .shared
            .store
            .lock()
            .unwrap()
            .messages
            .contains_key(&conversation)
        {
            return Err(EngineError::UnknownConversation);
        }
        let cmd_tx = self
            .backend
            .lock()
            .unwrap()
            .clone()
            .ok_or(EngineError::UnknownConversation)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .blocking_send(Command::Send {
                body,
                reply: reply_tx,
            })
            .map_err(|_| EngineError::SendFailed {
                reason: "engine stopped".to_string(),
            })?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::SendFailed {
                reason: "engine stopped".to_string(),
            })?
    }

    /// DT6: the safety number the two peers read aloud and compare in the
    /// verify ceremony. Bound to both MLS signature keys, so an active MITM
    /// produces different digits on each side. Blocks on the engine actor
    /// (like `send`); errors until paired. `conversation` is accepted for API
    /// symmetry — v0.1 has a single group, which is the one queried.
    pub fn security_code(&self, conversation: String) -> Result<String, EngineError> {
        let _ = conversation;
        let cmd_tx = self
            .backend
            .lock()
            .unwrap()
            .clone()
            .ok_or(EngineError::UnknownConversation)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        cmd_tx
            .blocking_send(Command::SafetyNumber { reply: reply_tx })
            .map_err(|_| EngineError::SendFailed {
                reason: "engine stopped".to_string(),
            })?;
        reply_rx
            .blocking_recv()
            .map_err(|_| EngineError::SendFailed {
                reason: "engine stopped".to_string(),
            })?
    }

    /// Mark a conversation verified after the user confirms the safety-number
    /// words. Idempotent.
    pub fn mark_verified(&self, conversation: String) -> Result<(), EngineError> {
        let mut s = self.shared.store.lock().unwrap();
        let c = s
            .conversations
            .iter_mut()
            .find(|c| c.id == conversation)
            .ok_or(EngineError::UnknownConversation)?;
        c.verified = true;
        drop(s);
        self.shared.persist_ui();
        self.shared
            .dispatch(EngineEvent::ConversationUpdated { conversation });
        Ok(())
    }

    /// Enroll in account recovery (issue #2): validate the PIN, mint the
    /// 5-word phrase and backup key, seal the identity blob, and persist
    /// BK + bundle in the encrypted store so Change PIN (2c) can re-wrap
    /// later. Runs four Argon2id derivations (seconds by design) — call
    /// from a background thread. Requires the persistent engine.
    pub fn backup_enroll(&self, pin: String) -> Result<BackupEnrollment, EngineError> {
        if !chatcore::backup::validate_pin(&pin) {
            return Err(EngineError::InvalidPin);
        }
        let mut disk = self.shared.disk.lock().unwrap();
        let Some(disk) = disk.as_mut() else {
            return Err(EngineError::NotSupported {
                reason: "recovery needs the persistent engine (new_persistent)".to_string(),
            });
        };
        let phrase = chatcore::backup::generate_phrase();
        let bk = chatcore::backup::random_bytes::<32>();
        let payload = self.backup_payload(disk)?;
        let bundle = chatcore::backup::build_bundle(&pin, &phrase, &bk, &payload)
            .map_err(|_| EngineError::InvalidPin)?;
        let bundle_bytes = bincode::serialize(&bundle).expect("bundle is always serializable");
        disk.commit(
            &[("backup.bk", &bk[..]), ("backup.bundle", &bundle_bytes)],
            &[],
        )
        .map_err(|e| EngineError::StorageFailed {
            reason: e.to_string(),
        })?;
        Ok(BackupEnrollment {
            phrase,
            bundle: bundle.into(),
        })
    }

    /// The stored bundle with a freshly rebuilt blob — the contact-change
    /// re-upload path. The wraps and auth hashes are reused untouched (they
    /// cover BK, which never changes here). None until `backup_enroll` ran.
    pub fn backup_bundle_current(&self) -> Result<Option<BackupBundle>, EngineError> {
        let mut disk = self.shared.disk.lock().unwrap();
        let Some(disk) = disk.as_mut() else {
            return Ok(None);
        };
        let map_err = |e: chatcore::StoreError| EngineError::StorageFailed {
            reason: e.to_string(),
        };
        let Some(bk) = disk.get("backup.bk").map_err(map_err)? else {
            return Ok(None);
        };
        let Some(bundle_bytes) = disk.get("backup.bundle").map_err(map_err)? else {
            return Ok(None);
        };
        let bk: [u8; 32] = bk.try_into().map_err(|_| EngineError::StorageFailed {
            reason: "stored backup key is corrupt".to_string(),
        })?;
        let mut bundle: chatcore::backup::BackupBundle = bincode::deserialize(&bundle_bytes)
            .map_err(|e| EngineError::StorageFailed {
                reason: e.to_string(),
            })?;
        let payload = self.backup_payload(disk)?;
        bundle.blob = chatcore::backup::seal_blob(&bk, &payload);
        let bundle_bytes = bincode::serialize(&bundle).expect("bundle is always serializable");
        disk.commit(&[("backup.bundle", &bundle_bytes)], &[])
            .map_err(map_err)?;
        Ok(Some(bundle.into()))
    }

    /// Issue #4 (Change PIN gate): check a PIN or phrase against the stored
    /// bundle locally — no server call, no attempt counting. With the DB
    /// already open BK is readable anyway; this is consent UX, not a
    /// security boundary. False when not enrolled or not persistent. Runs
    /// an Argon2id derivation — call from a background thread.
    pub fn backup_verify_secret(&self, secret: String) -> Result<bool, EngineError> {
        let mut disk = self.shared.disk.lock().unwrap();
        let Some(disk) = disk.as_mut() else {
            return Ok(false);
        };
        let Some(bundle_bytes) =
            disk.get("backup.bundle")
                .map_err(|e| EngineError::StorageFailed {
                    reason: e.to_string(),
                })?
        else {
            return Ok(false);
        };
        let bundle: chatcore::backup::BackupBundle =
            bincode::deserialize(&bundle_bytes).map_err(|e| EngineError::StorageFailed {
                reason: e.to_string(),
            })?;
        let ok = if chatcore::backup::validate_pin(&secret) {
            chatcore::backup::unwrap_bk(&secret, &bundle.salt_p, &bundle.w_pin).is_ok()
        } else {
            let phrase = chatcore::backup::normalize_phrase(&secret);
            chatcore::backup::unwrap_bk(&phrase, &bundle.salt_f, &bundle.w_phrase).is_ok()
        };
        Ok(ok)
    }

    /// Issue #3 (phrase-path restore) and 2c (Change PIN): re-wrap the
    /// stored BK under a new PIN with fresh salts and refresh the stored
    /// bundle. The phrase wrap and blob are untouched — BK never changes.
    /// Returns the bundle for re-upload. Runs two Argon2id derivations —
    /// call from a background thread.
    pub fn backup_rewrap_pin(&self, new_pin: String) -> Result<BackupBundle, EngineError> {
        if !chatcore::backup::validate_pin(&new_pin) {
            return Err(EngineError::InvalidPin);
        }
        let mut disk = self.shared.disk.lock().unwrap();
        let Some(disk) = disk.as_mut() else {
            return Err(EngineError::NotSupported {
                reason: "recovery needs the persistent engine (new_persistent)".to_string(),
            });
        };
        let map_err = |e: chatcore::StoreError| EngineError::StorageFailed {
            reason: e.to_string(),
        };
        let (Some(bk), Some(bundle_bytes)) = (
            disk.get("backup.bk").map_err(map_err)?,
            disk.get("backup.bundle").map_err(map_err)?,
        ) else {
            return Err(EngineError::NotSupported {
                reason: "no recovery enrollment to re-wrap".to_string(),
            });
        };
        let bk: [u8; 32] = bk.try_into().map_err(|_| EngineError::StorageFailed {
            reason: "stored backup key is corrupt".to_string(),
        })?;
        let mut bundle: chatcore::backup::BackupBundle = bincode::deserialize(&bundle_bytes)
            .map_err(|e| EngineError::StorageFailed {
                reason: e.to_string(),
            })?;
        let salt_p = chatcore::backup::random_bytes::<16>();
        let salt_a = chatcore::backup::random_bytes::<16>();
        bundle.w_pin = chatcore::backup::wrap_bk(&new_pin, &salt_p, &bk);
        bundle.salt_p = salt_p.to_vec();
        bundle.auth_pin = chatcore::backup::auth_hash(&new_pin, &salt_a);
        bundle.salt_a = salt_a.to_vec();
        let bundle_bytes = bincode::serialize(&bundle).expect("bundle is always serializable");
        disk.commit(&[("backup.bundle", &bundle_bytes)], &[])
            .map_err(map_err)?;
        Ok(bundle.into())
    }
}

/// Wire the engine's SQLCipher write-through, opening its own connection to
/// the database `new_persistent` already validated. No-op without a cfg.
fn attach_disk(core: &mut CoreEngine, persist: Option<&PersistCfg>) -> anyhow::Result<()> {
    if let Some(cfg) = persist {
        core.attach_store(DiskStore::open(Path::new(&cfg.path), &cfg.key)?)?;
    }
    Ok(())
}

/// Resume a persisted engine in the background: retry until the relay is
/// reachable (app launches offline are normal), then run the ordinary actor
/// loop. Commands arriving while offline are answered with a failure
/// instead of blocking their caller until the network returns.
fn spawn_resume(shared: Arc<Shared>, cfg: PersistCfg) -> mpsc::Sender<Command> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(16);
    std::thread::Builder::new()
        .name("chat-ffi-engine".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("building the engine runtime never fails");
            rt.block_on(async move {
                let mut reported_offline = false;
                let core = loop {
                    let attempt = async {
                        CoreEngine::resume(DiskStore::open(Path::new(&cfg.path), &cfg.key)?).await
                    };
                    match attempt.await {
                        Ok(core) => break core,
                        Err(_) => {
                            if !reported_offline {
                                shared.dispatch(EngineEvent::ConnectionStateChanged {
                                    online: false,
                                });
                                reported_offline = true;
                            }
                            while let Ok(cmd) = cmd_rx.try_recv() {
                                reject_offline(cmd);
                            }
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                };
                if reported_offline {
                    shared.dispatch(EngineEvent::ConnectionStateChanged { online: true });
                }
                // The conversation (if pairing completed before the crash)
                // was persisted with the UI state; actor_loop's top-of-loop
                // check re-creates it from the engine if the UI blob lagged.
                let conversation = shared
                    .store
                    .lock()
                    .unwrap()
                    .conversations
                    .first()
                    .map(|c| c.id.clone());
                actor_loop(core, cmd_rx, shared, conversation).await;
            });
        })
        .expect("spawning the engine thread never fails");
    cmd_tx
}

fn reject_offline(cmd: Command) {
    match cmd {
        Command::CreateLink { reply } => {
            let _ = reply.send(Err(anyhow::anyhow!("still reconnecting")));
        }
        Command::Send { reply, .. } => {
            let _ = reply.send(Err(EngineError::SendFailed {
                reason: "still reconnecting to the relay".to_string(),
            }));
        }
        Command::SafetyNumber { reply } => {
            // The number itself is a pure function of local group state, but
            // this path has no `core` handle; the UI retries once connected.
            let _ = reply.send(Err(EngineError::UnknownConversation));
        }
    }
}

/// Issue #3: a restored device has a "session" identity in kv but no
/// "engine" record yet — pick that identity up for the first connect so
/// peers see the same signer. Never fails the connect path: a fresh session
/// is the correct fallback for every non-restore case.
fn restored_session(persist: Option<&PersistCfg>) -> Option<chatcore::ChatSession> {
    let cfg = persist?;
    let disk = DiskStore::open(Path::new(&cfg.path), &cfg.key).ok()?;
    if disk.get("engine").ok()?.is_some() {
        return None; // a live engine resumes instead — never re-connects here
    }
    let bytes = disk.get("session").ok()??;
    chatcore::ChatSession::restore(&bytes).ok()
}

/// Register the (single, v0.1) conversation in the store once pairing
/// completes, on either side. Upserts by id: a restored contact stub
/// (issue #3) becomes the live pairing instead of a duplicate row.
fn create_conversation(shared: &Shared, peer_name: Option<String>) -> String {
    let id = CONVERSATION_ID.to_string();
    {
        let mut store = shared.store.lock().unwrap();
        let display_name = peer_name.unwrap_or_else(|| "New contact".to_string());
        if let Some(existing) = store.conversations.iter_mut().find(|c| c.id == id) {
            existing.display_name = display_name;
        } else {
            store.conversations.push(Conversation {
                id: id.clone(),
                display_name,
                last_message: None,
                unread: 0,
                verified: false,
            });
        }
        store.messages.entry(id.clone()).or_default();
    }
    shared.persist_ui();
    shared.dispatch(EngineEvent::ConversationUpdated {
        conversation: id.clone(),
    });
    id
}

/// The engine actor: sole owner of the `CoreEngine`, running on the dedicated
/// engine thread. Interleaves FFI commands with engine events; exits when the
/// FFI object (and thus the command channel) is dropped.
async fn actor_loop(
    mut core: CoreEngine,
    mut cmd_rx: mpsc::Receiver<Command>,
    shared: Arc<Shared>,
    mut conversation: Option<String>,
) {
    loop {
        // The listen side becomes paired mid-loop (await_pairing below).
        if conversation.is_none() && core.is_paired() {
            conversation = Some(create_conversation(&shared, core.peer_name()));
        }
        match conversation.clone() {
            Some(conv) => {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        let Some(cmd) = cmd else { return };
                        handle_command(cmd, &mut core, &shared, &conv).await;
                    }
                    event = core.next_event() => match event {
                        Ok(event) => forward_event(event, &shared, &conv),
                        Err(_) => {
                            // Reconnect exhausted its patience: the engine is
                            // wedged. Tell the UI and stop.
                            shared.dispatch(EngineEvent::ConnectionStateChanged { online: false });
                            return;
                        }
                    }
                }
            }
            None => {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        let Some(cmd) = cmd else { return };
                        handle_command(cmd, &mut core, &shared, CONVERSATION_ID).await;
                    }
                    paired = core.await_pairing() => {
                        if paired.is_err() {
                            shared.dispatch(EngineEvent::ConnectionStateChanged { online: false });
                            return;
                        }
                        // Conversation creation happens at the top of the loop.
                    }
                }
            }
        }
    }
}

async fn handle_command(cmd: Command, core: &mut CoreEngine, shared: &Shared, conversation: &str) {
    match cmd {
        Command::CreateLink { reply } => {
            let _ = reply.send(core.contact_link());
        }
        Command::SafetyNumber { reply } => {
            // Errors (no group) until paired — the UI only asks for a real
            // conversation, so that maps to UnknownConversation.
            let _ = reply.send(
                core.safety_number()
                    .map_err(|_| EngineError::UnknownConversation),
            );
        }
        Command::Send { body, reply } => {
            if !core.is_paired() {
                let _ = reply.send(Err(EngineError::SendFailed {
                    reason: "not paired yet".to_string(),
                }));
                return;
            }
            let result = core.send_message(body.as_bytes()).await;
            let delivery = if result.is_ok() {
                DeliveryState::Sent
            } else {
                DeliveryState::Failed
            };
            {
                let mut store = shared.store.lock().unwrap();
                let msg = ChatMessage {
                    id: shared.next_msg_id(),
                    body: body.clone(),
                    mine: true,
                    timestamp_ms: now_ms(),
                    delivery,
                };
                if let Some(msgs) = store.messages.get_mut(conversation) {
                    msgs.push(msg);
                }
                if let Some(c) = store
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == conversation)
                {
                    c.last_message = Some(body);
                }
            }
            shared.persist_ui();
            shared.dispatch(EngineEvent::ConversationUpdated {
                conversation: conversation.to_string(),
            });
            let _ = reply.send(result.map_err(|e| EngineError::SendFailed {
                reason: e.to_string(),
            }));
        }
    }
}

fn forward_event(event: engine::Event, shared: &Shared, conversation: &str) {
    match event {
        engine::Event::Message(bytes) => {
            let message = ChatMessage {
                id: shared.next_msg_id(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
                mine: false,
                timestamp_ms: now_ms(),
                delivery: DeliveryState::Received,
            };
            {
                let mut store = shared.store.lock().unwrap();
                if let Some(msgs) = store.messages.get_mut(conversation) {
                    msgs.push(message.clone());
                }
                if let Some(c) = store
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == conversation)
                {
                    c.last_message = Some(message.body.clone());
                    c.unread += 1;
                }
            }
            shared.persist_ui();
            shared.dispatch(EngineEvent::MessageReceived {
                conversation: conversation.to_string(),
                message,
            });
        }
        engine::Event::EpochAdvanced(_) => {
            shared.dispatch(EngineEvent::SecurityCodeChanged {
                conversation: conversation.to_string(),
            });
        }
        engine::Event::ConnectionChanged(online) => {
            shared.dispatch(EngineEvent::ConnectionStateChanged { online });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Network-free contract checks; the full stack (real relay, real MLS,
    // dispatch-thread delivery) is proven in tests/callback_delivery.rs.

    #[test]
    fn empty_and_malformed_links_are_invalid() {
        let engine = ChatEngine::new("bob".to_string());
        assert!(matches!(
            engine.pair_with_link("  ".to_string()),
            Err(EngineError::InvalidContactLink)
        ));
        assert!(matches!(
            engine.pair_with_link("not b64 at all!!!".to_string()),
            Err(EngineError::InvalidContactLink)
        ));
    }

    #[test]
    fn attestation_conversion_drops_wrong_length_nonces() {
        // A well-formed 16-byte nonce converts.
        let good = AttestationToken {
            nonce: vec![7u8; 16],
            expires_at: 42,
            signature: vec![1, 2, 3],
        };
        let proto = attestation_to_proto(good).expect("16-byte nonce converts");
        assert_eq!(proto.nonce, [7u8; 16]);
        assert_eq!(proto.expires_at, 42);

        // Wrong-length nonces are dropped, never truncated or padded.
        for bad_len in [0, 15, 17, 32] {
            let bad = AttestationToken {
                nonce: vec![0u8; bad_len],
                expires_at: 1,
                signature: vec![],
            };
            assert!(
                attestation_to_proto(bad).is_none(),
                "len {bad_len} is dropped"
            );
        }

        // stock_attestation_tokens keeps only the valid ones.
        let engine = ChatEngine::new("bob".to_string());
        engine.stock_attestation_tokens(vec![
            AttestationToken {
                nonce: vec![1u8; 16],
                expires_at: 1,
                signature: vec![],
            },
            AttestationToken {
                nonce: vec![2u8; 3],
                expires_at: 1,
                signature: vec![],
            },
        ]);
        assert_eq!(engine.shared.attestation.lock().unwrap().len(), 1);
    }

    #[test]
    fn unknown_conversation_is_an_error() {
        let engine = ChatEngine::new("bob".to_string());
        assert!(matches!(
            engine.send("nope".to_string(), "x".to_string()),
            Err(EngineError::UnknownConversation)
        ));
        assert!(matches!(
            engine.mark_verified("nope".to_string()),
            Err(EngineError::UnknownConversation)
        ));
        assert!(engine.conversations().is_empty());
        assert!(engine.messages("nope".to_string()).is_empty());
    }

    #[test]
    fn backup_enroll_needs_a_valid_pin_and_persistence() {
        let engine = ChatEngine::new("bob".to_string());
        assert!(matches!(
            engine.backup_enroll("12".to_string()),
            Err(EngineError::InvalidPin)
        ));
        assert!(matches!(
            engine.backup_enroll("1234".to_string()),
            Err(EngineError::NotSupported { .. })
        ));
        assert!(engine.backup_bundle_current().unwrap().is_none());
    }

    #[test]
    fn backup_enroll_roundtrips_through_the_pin_wrap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db").to_string_lossy().into_owned();
        let engine =
            ChatEngine::new_persistent("bob#01".to_string(), path, "key".to_string()).unwrap();

        let enrollment = engine.backup_enroll("123456".to_string()).unwrap();
        assert_eq!(enrollment.phrase.split(' ').count(), 5);

        let bundle = engine.backup_bundle_current().unwrap().expect("enrolled");
        let bk = chatcore::backup::unwrap_bk("123456", &bundle.salt_p, &bundle.w_pin).unwrap();
        let payload: chatcore::backup::BackupPayload =
            bincode::deserialize(&chatcore::backup::open_blob(&bk, &bundle.blob).unwrap()).unwrap();
        assert_eq!(payload.username.as_deref(), Some("bob#01"));
        assert!(
            payload.identity.is_none(),
            "no session exists before first connect"
        );

        // Phrase path recovers the same BK.
        assert_eq!(
            chatcore::backup::unwrap_bk(&enrollment.phrase, &bundle.salt_f, &bundle.w_phrase)
                .unwrap(),
            bk
        );
    }

    #[test]
    fn restore_roundtrips_and_wrong_secret_leaves_no_db() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a.db").to_string_lossy().into_owned();
        let a =
            ChatEngine::new_persistent("mira#04".to_string(), path_a, "ka".to_string()).unwrap();
        let enrollment = a.backup_enroll("4321".to_string()).unwrap();

        // Wrong secret: typed error, no file left behind.
        let path_bad = dir.path().join("bad.db");
        let result = ChatEngine::new_from_backup(
            "ignored".to_string(),
            path_bad.to_string_lossy().into_owned(),
            "kb".to_string(),
            enrollment.bundle.clone(),
            "9999".to_string(),
        );
        assert!(matches!(result, Err(EngineError::WrongRecoverySecret)));
        assert!(!path_bad.exists(), "wrong secret must not create a DB");

        // PIN path restores the username as the display name.
        let path_b = dir.path().join("b.db").to_string_lossy().into_owned();
        let b = ChatEngine::new_from_backup(
            "ignored".to_string(),
            path_b,
            "kb".to_string(),
            enrollment.bundle.clone(),
            "4321".to_string(),
        )
        .unwrap();
        assert_eq!(b.display_name(), "mira#04");
        assert!(b.backup_bundle_current().unwrap().is_some());

        // Phrase path, typed sloppily, normalizes and works too.
        let path_c = dir.path().join("c.db").to_string_lossy().into_owned();
        let sloppy = format!("  {}  ", enrollment.phrase.to_uppercase());
        let c = ChatEngine::new_from_backup(
            "ignored".to_string(),
            path_c,
            "kc".to_string(),
            enrollment.bundle,
            sloppy,
        )
        .unwrap();
        assert_eq!(c.display_name(), "mira#04");
    }

    #[test]
    fn rewrap_pin_moves_the_pin_wrap_and_keeps_the_phrase_wrap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db").to_string_lossy().into_owned();
        let e = ChatEngine::new_persistent("bob#02".to_string(), path, "k".to_string()).unwrap();
        let enrollment = e.backup_enroll("1111".to_string()).unwrap();

        let rewrapped = e.backup_rewrap_pin("222222".to_string()).unwrap();
        assert!(
            chatcore::backup::unwrap_bk("1111", &rewrapped.salt_p, &rewrapped.w_pin).is_err(),
            "old PIN must stop working"
        );
        let bk =
            chatcore::backup::unwrap_bk("222222", &rewrapped.salt_p, &rewrapped.w_pin).unwrap();
        assert_eq!(
            chatcore::backup::unwrap_bk(&enrollment.phrase, &rewrapped.salt_f, &rewrapped.w_phrase)
                .unwrap(),
            bk,
            "phrase wrap must survive a PIN rewrap"
        );
    }

    #[test]
    fn verify_secret_checks_locally_without_touching_the_wraps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.db").to_string_lossy().into_owned();
        let e = ChatEngine::new_persistent("ann#09".to_string(), path, "k".to_string()).unwrap();
        assert!(
            !e.backup_verify_secret("1234".to_string()).unwrap(),
            "not enrolled yet"
        );
        let enrollment = e.backup_enroll("1234".to_string()).unwrap();
        assert!(e.backup_verify_secret("1234".to_string()).unwrap());
        assert!(!e.backup_verify_secret("1235".to_string()).unwrap());
        assert!(e
            .backup_verify_secret(format!(" {} ", enrollment.phrase.to_uppercase()))
            .unwrap());
        assert!(!ChatEngine::new("x".to_string())
            .backup_verify_secret("1234".to_string())
            .unwrap());
    }

    #[test]
    fn auth_proof_matches_core_and_normalizes_phrases() {
        let salt = chatcore::backup::random_bytes::<16>().to_vec();
        assert_eq!(
            backup_auth_proof("1234".to_string(), salt.clone()),
            chatcore::backup::auth_hash("1234", &salt)
        );
        assert_eq!(
            backup_auth_proof("  Word ONE  two ".to_string(), salt.clone()),
            chatcore::backup::auth_hash("word one two", &salt)
        );
    }
}
