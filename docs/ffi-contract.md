# FFI Contract — the backend/frontend seam

This is the ONE interface the backend and frontend share. It lets both teams
work independently: the frontend builds the whole app against these signatures
today (they're backed by an in-memory stub), and the backend swaps the stub for
the real engine later **without changing any signature**.

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
banner, failed-send bubble, quiet security-code line).

The interface is deliberately transport-free — no relay, epoch, MLS, or socket
terms cross it. That is why it stays stable while the backend adds TLS,
request-ids, reconnect, and real MLS underneath.

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

## What "stub" means for the frontend right now

`send` stores the message and echoes a canned `echo: <text>` reply through the
listener, `pair_with_link` fabricates a peer, `create_contact_link` returns a
placeholder token. No network, no crypto. This is enough to build and exercise
every wireframe screen (list, chat, pairing, verification, states) against real
generated bindings. When the backend lands the real engine (T6), the same
bindings start talking to actual relays — no frontend code change.

## Changing the contract

If the backend needs to change a signature, it is a coordinated event: announce
it, land the `ffi/` change and the frontend's binding update together (or behind
a short-lived branch). Adding a NEW method or a NEW event variant is safe and
non-breaking — prefer additive changes.
