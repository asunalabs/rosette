# FFI Contract — the backend/frontend seam

This is the ONE interface the backend and frontend share. It lets both teams
work independently: the frontend builds the whole app against these signatures,
and the backend evolves what's underneath **without changing any signature**.

> **2026-07-12 (T7): the stub is gone.** The same signatures are now backed by
> the real `engine/` crate — MLS encryption, TLS relay connection, reconnect,
> dedup, epoch-conflict retry. Nothing the frontend already built changes.
> Additive updates a frontend dev should know about are marked **ADDITIVE**
> below.

Source of truth: `ffi/src/lib.rs`. This doc summarizes it; the code is canonical.

## Who owns what

| Path | Owner | Notes |
|------|-------|-------|
| `ffi/` | **Backend** | The UniFFI surface. Signatures are FROZEN — a change here is a coordinated breaking change, announced before merge. |
| `proto/ core/ engine/ relay/ cli/` | **Backend** | Protocol, MLS, engine, relay, dogfood CLI. Frontend never edits these. |
| `app/composeApp` | **Frontend** | Compose Multiplatform UI (the wireframes). |
| `app/engine-kt` | **Frontend** | Gobley module: builds `ffi/` via cargo and generates the Kotlin bindings. |
| `app/iosApp` | **Frontend** | Xcode shell (signing/entitlements). |
| `docs/`, `Cargo.toml` (workspace members) | Shared | Coordinate on edits; rare. |

Because the directory sets are disjoint, direct pushes to `master` from both
sides almost never conflict. The only shared merge points are this contract and
the workspace `Cargo.toml` — both already set up, so neither team needs to touch
them to start.

## The interface (frozen)

`ChatEngine` — the one object the UI holds, created once at app start.

| Method | Signature | Purpose |
|--------|-----------|---------|
| `new` | `(display_name: String) -> ChatEngine` | Create the engine. Name is local-only, never a network id. |
| `set_listener` | `(EngineEventListener)` | Register the event callback (once, after construction). |
| `create_contact_link` | `() -> String` | The base64 string your QR code encodes (wireframe-v1 frame B). |
| `pair_with_link` | `(link: String) -> Result<String, EngineError>` | Consume a scanned link, return the new conversation id. |
| `conversations` | `() -> Vec<Conversation>` | Conversation-list data. |
| `messages` | `(conversation: String) -> Vec<ChatMessage>` | Messages in a conversation, oldest first. |
| `send` | `(conversation: String, body: String) -> Result<(), EngineError>` | Send a text message. |
| `mark_verified` | `(conversation: String) -> Result<(), EngineError>` | Confirm the safety-number words (wireframe-v1 frame C). |

Data types: `Conversation { id, display_name, last_message?, unread, verified }`,
`ChatMessage { id, body, mine, timestamp_ms, delivery }`,
`DeliveryState { Pending, Sent, Failed, Received }`.

Events (pushed via `EngineEventListener.on_event`):
`MessageReceived`, `ConversationUpdated`, `ConnectionStateChanged { online }`,
`SecurityCodeChanged`. These map 1:1 to the wireframe-v1 states (reconnect
banner, failed-send bubble, quiet security-code line). Callbacks always run
on the dedicated `chat-ffi-dispatch` thread (review OV8), never a tokio
worker; events fired before `set_listener` are buffered and flushed in order.

The interface is deliberately transport-free — no relay, epoch, MLS, or socket
terms cross it. That is why it stays stable while the backend adds TLS,
request-ids, reconnect, and real MLS underneath.

### Real-engine behavior notes (T7, all within the frozen signatures)

- **ADDITIVE — `EngineError` gained two variants:** `RelayUnreachable { reason }`
  (network problem, distinct from a bad code) and `NotSupported { reason }`
  (v0.1 limits: one conversation per engine — a second `pair_with_link`
  returns this). On the Kotlin side these are new exception subclasses;
  existing `when`/`catch` code keeps compiling.
- **Home relay config:** `create_contact_link` mints a mailbox on the relay
  named by the `CHAT_RELAY_ADDR` + `CHAT_RELAY_FINGERPRINT` (64-char hex) env
  vars (relay address is not user-editable in v1; a baked-in production
  default replaces this when one exists). `pair_with_link` needs no config —
  the scanned link carries its relay + fingerprint.
- **`create_contact_link` failure mode:** the frozen signature is infallible,
  so when the relay is unreachable/unconfigured it returns an EMPTY string
  and emits `ConnectionStateChanged { online: false }`. Treat empty as "show
  the calm offline banner, retry later".
- `pair_with_link` and `send` block for a network round-trip — call them from
  a coroutine off the main thread.
- `display_name` in `Conversation` is the peer's MLS-credential name
  (decoration only, never an identifier); `"New contact"` if absent.

## Frontend: consuming it via Gobley

`app/engine-kt` is a Gobley (`uniffi-kotlin-multiplatform-bindings`) module that:
1. Builds the `ffi/` crate with cargo for each target (Android `.so` via
   cargo-ndk, desktop `.{so,dylib,dll}`, iOS `.a` → XCFramework).
2. Generates the Kotlin bindings from the `#[uniffi::export]` items.

Bindings are generated at build time, never committed. A Kotlin-only dev still
needs the Rust toolchain installed (accepted monorepo cost).

**Go/no-go gate (architecture.md step 4 / review 1A + OV7):** the first frontend
milestone is proving Gobley generates and links on all three targets — the FFI
smoke test (`create_contact_link()` returns non-empty). Android + desktop
passing unblocks UI work; iOS-simulator gets a 5-working-day timebox before it
drops to a tracked spike. If Gobley itself fails to generate bindings, that is
the gate doing its job — fall back to vanilla UniFFI + a hand-written cinterop
shim for iOS (Android/desktop are fine on vanilla UniFFI's JNA output).

## ~~What "stub" means for the frontend right now~~ (retired at T7)

The in-memory stub (canned `echo:` replies, fabricated peers, placeholder
links) was replaced by the real engine on 2026-07-12 — the same bindings now
talk to actual relays with real MLS encryption, exactly as promised: no
frontend code change. For local development, point `CHAT_RELAY_ADDR` /
`CHAT_RELAY_FINGERPRINT` at a locally running `relay` binary (it prints its
fingerprint on startup).

## Changing the contract

If the backend needs to change a signature, it is a coordinated event: announce
it, land the `ffi/` change and the frontend's binding update together (or behind
a short-lived branch). Adding a NEW method or a NEW event variant is safe and
non-breaking — prefer additive changes.
