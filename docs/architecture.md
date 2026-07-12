# Architecture & Final Repo Structure (v1)

Status: draft under /plan-eng-review, 2026-07-11
Decisions locked this review: D3 monorepo, D4 thick Rust core, D5 Compose
Multiplatform with Android + Desktop + iOS targets from day one.

## Problem statement

The repo has a working protocol skeleton (4 Rust crates, 3-client convergence
proven over a real relay) and an approved wireframe, but no client app and no
defined end-state structure. This doc fixes the target architecture and repo
layout so every next step (persistence, pairing hardening, the app itself)
lands in a known place instead of accreting.

## Locked decisions

| # | Decision | Choice | Why (one line) |
|---|----------|--------|----------------|
| D3 | Repo topology | Monorepo | Client embeds the Rust core via FFI; cross-language changes are the common case — atomic commits beat publish/consume ceremony for a solo dev. |
| D4 | FFI boundary | Thick Rust core | The hard correctness logic (epoch-conflict retry, echo dedup, at-least-once handling) already exists and is tested in Rust; one implementation, one audit target. |
| D5 | UI stack | Compose Multiplatform: androidMain + desktopMain + iosMain, all active | One UI for the one approved wireframe; desktop is the daily dogfood loop; iOS builds from day one — its *distribution* posture stays governed by TODOS.md item 5. |

## System architecture

```text
┌─────────────────────── app/ (Kotlin Multiplatform, Gradle) ───────────────────────┐
│                                                                                    │
│  composeApp/         Compose Multiplatform UI — the wireframe-v0 direction         │
│    ├─ commonMain     screens, components, navigation (all UI lives here)           │
│    ├─ androidMain    Android entry, QR camera, notifications                       │
│    ├─ desktopMain    JVM window entry (daily dogfood + demo client)                │
│    └─ iosMain        iOS entry glue (builds day one; ships per TODOS #5)           │
│                                                                                    │
│  engine-kt/          generated Kotlin bindings module (Gobley + UniFFI)            │
│                      JVM/Android: JNA over cdylib · iOS: cinterop over staticlib   │
└──────────────────────────────────▲─────────────────────────────────────────────────┘
                                   │ UniFFI boundary: high-level API only
                                   │ (pair, send, observeConversations, …)
┌──────────────────────────────────┴──────────── Rust workspace ─────────────────────┐
│                                                                                    │
│  ffi/       UniFFI surface crate. Thin: exposes engine/ objects + a push-event     │
│             callback interface. NO logic — translation only.                       │
│                                                                                    │
│  engine/    Client engine (NEW — extracted from cli/). Owns:                       │
│             relay connection + reconnect, subscribe set, orchestration,            │
│             own-echo dedup, epoch-conflict retry (winner/loser resolution),        │
│             later: SQLCipher persistence (T5/T8), multi-relay endpoints (OQ11)     │
│                                                                                    │
│  core/      MLS core (unchanged role). Only OpenMLS toucher: sessions,             │
│             identity, MLS-native pairing (A4). Gains storage provider at T5.       │
│                                                                                    │
│  proto/     Wire types (unchanged role). Single source of truth for everything     │
│             crossing the client-relay boundary: envelope, link, framing,           │
│             pow, auth, limits, rejection codes.                                    │
│                                                                                    │
│  relay/     Store-and-forward daemon (server-side). Content-blind, epoch-aware     │
│             DS. Depends on proto/ only — never on core/ or engine/.                │
│                                                                                    │
│  cli/       Dogfood REPL + e2e tests, rewritten as a THIN shell over engine/.      │
│             The 3-client convergence test drives engine/, so the test covers       │
│             exactly what the app ships.                                            │
└────────────────────────────────────────────────────────────────────────────────────┘
                    engine/ ──TCP + TLS (rustls) (framed bincode, request-id correlated)──▶ relay/
```

Dependency rules (enforced by review, cheap to check):

```text
proto ◀── core ◀── engine ◀── ffi ◀── engine-kt ◀── composeApp
proto ◀── relay                 ▲
proto ◀───────── engine         └── cli (dev-only consumer, same API as the app)
```

- `relay/` never imports `core/` or `engine/` (server stays MLS-blind).
- `ffi/` never imports `openmls` types directly — everything crosses as
  engine-level API objects or opaque bytes.
- Kotlin never re-implements protocol/orchestration logic. If Kotlin needs a
  behavior change, the change happens in `engine/`.

## Final repo layout

```text
chat/
├── Cargo.toml                    # workspace: proto, core, engine, ffi, relay, cli
├── proto/                        # (exists) wire protocol
├── core/                         # (exists) MLS core
├── engine/                       # NEW  client engine — see diagram
│   ├── src/
│   └── tests/                    # convergence + reconnect tests move here over time
├── ffi/                          # NEW  UniFFI surface crate (cdylib + staticlib)
├── relay/                        # (exists) relay daemon
│   └── Dockerfile                # NEW  container build for operators
├── cli/                          # (exists) becomes thin REPL over engine/
├── app/                          # NEW  Kotlin Multiplatform project (own Gradle root)
│   ├── settings.gradle.kts
│   ├── gradle/ …
│   ├── engine-kt/                # Gobley module: builds ffi/ via cargo, generates bindings
│   ├── composeApp/               # Compose MP UI: commonMain/androidMain/desktopMain/iosMain
│   └── iosApp/                   # Xcode shell project (thin; owns signing/entitlements)
├── docs/
│   ├── architecture.md           # this document
│   └── wireframe-v0.{html,png}   # (exists) approved UI direction
├── .github/workflows/
│   ├── ci.yml                    # NEW  cargo test + gradle build/test on every push
│   └── release-*.yml             # NEW  per-artifact release pipelines (see Distribution)
├── TODOS.md                      # (exists) strategy backlog
└── CLAUDE.md                     # (exists)
```

What deliberately does NOT exist: a `common/`/`utils/` crate (nothing shared
needs a home that proto/ doesn't already provide), a backend API server
(the relay IS the entire server surface), and a web client (out of scope,
would be a fourth toolchain for an audience the wedge doesn't target).

## FFI strategy

- **Tooling:** UniFFI for interface definition; **Gobley** (uniffi-kotlin-
  multiplatform-bindings) for the KMP side, because vanilla UniFFI's Kotlin
  output is JNA-only (JVM/Android) and cannot serve `iosMain`. Gobley
  generates one KMP `engine-kt` module: JNA on JVM/Android, cinterop on
  Kotlin/Native. [Layer 2 dependency — newer than UniFFI itself; the
  fallback if it disappoints is vanilla UniFFI Kotlin + a hand-written
  cinterop shim for iOS, which is bounded, known work.]
- **Push events:** the relay pushes asynchronously; `ffi/` exposes a UniFFI
  callback interface (`EventListener.onEvent(ConversationEvent)`) that the
  engine invokes. Kotlin wraps it into a `Flow`. No polling.
  **Threading contract (review OV8):** events drain through an internal
  channel consumed by a dedicated dispatch thread — callbacks are NEVER
  invoked from tokio worker threads, so a slow/blocking Kotlin handler can
  stall at most event delivery, never the engine itself.
- **Threading:** engine owns a single tokio runtime created at
  `ChatEngine.create()`; all FFI calls are non-blocking or clearly marked.
- **Bindings are generated at build time** by the Gobley Gradle plugin — never
  committed. A dev touching only Kotlin still needs the Rust toolchain; that
  is an accepted monorepo cost (documented in README setup).

## Native build matrix

| Target | Rust artifact | Built by |
|--------|--------------|----------|
| Android arm64-v8a / armeabi-v7a / x86_64 | `libchat_ffi.so` (cdylib, NDK) | Gobley via cargo-ndk |
| Desktop linux x86_64 / macOS arm64 / windows x86_64 | `libchat_ffi.{so,dylib,dll}` | Gobley, host cargo |
| iOS aarch64 + sim | `libchat_ffi.a` (staticlib) → XCFramework | Gobley + Xcode |

## Distribution (part of the plan, not an afterthought)

| Artifact | Channel | Pipeline |
|----------|---------|----------|
| relay | Container image (GHCR) + static musl binary on GitHub Releases — **gated on the relay-persistence milestone (step 5, review OV1): no distribution artifact ships while relay state is in-memory-only** | `release-relay.yml` |
| Android app | APK on GitHub Releases + F-Droid repo (mandate-resistant, non-negotiable); Play Store per TODOS #2 decision | `release-android.yml` |
| Desktop app | GitHub Releases (AppImage/dmg/msi via Compose packaging) | `release-desktop.yml` |
| iOS app | Deferred — governed by TODOS #5 (build target exists; TestFlight/store/sideload decision is strategic, not technical) | — |

`ci.yml` from day one of app/ scaffolding: `cargo test --workspace` +
`./gradlew build` + the FFI smoke test (below). Release pipelines land with
their first artifact, not before.

## UI / design plan (from /plan-design-review, 2026-07-11)

Design source of truth: `docs/wireframe-v0.html` (chat list + chat) plus
`docs/wireframe-v1.html` (the screens v0 deferred — pairing, verification,
failure states, desktop two-pane). These are LAYOUT specs only; the visual
token layer is a prerequisite, see "Design system gate" below.

### Screen inventory + navigation (Pass 1)

```text
first launch ──▶ EmptyState (= your QR, the pairing screen; NO onboarding carousel)
                       │
ChatList ◀────────────┘
  │  ▲
  │  └──────────────── Chat  (⇄ back)
  │                      ▲
  └──(+)──▶ AddContact ──┤   tabs: [My code] [Scan]
              │          │
              └──▶ Verification sheet (offered ONCE after pairing, skippable)
```

Five screens. **Settings deliberately absent from v1** — nothing to set yet
(subtraction default). Relay address is not user-editable in v1.

### Interaction states (Pass 2) — what the user SEES, mapped to engine events

| State family | User sees | Engine event |
|--------------|-----------|--------------|
| Connection loss | One calm top banner ("Connecting… messages will send when you're back online"). NEVER modal, never blocks composing — sends queue. | reconnect loop (step 2 engine) |
| Failed send | Red-edged bubble stays in place with "Not sent yet · tap to retry". Message never disappears. | send rejection / timeout |
| Key/epoch change | One quiet system line: "X changed their security code. Review". No blocking interstitial (calm > fear). | `Incoming::CommitApplied` with changed keys |
| Loading | Skeleton rows in the list; nothing in an open chat (local-first = instant once persisted). | pre-first-sync |
| Empty (first launch) | The QR pairing screen itself — the empty state IS the onboarding. | no conversations yet |

### User journey (Pass 3)

install → no signup form (the visceral privacy proof: every competitor asks
for a phone number here, this app shows your QR) → scan/share → connected →
verification offered once, positively framed ("compare these five words to be
sure nobody's in the middle"), skippable, never nagging → first message.
Unverified chats get NO scary warnings; verification adds a quiet ✓ by the name.

### Visual identity (Pass 4)

Looking familiar is intentional — the wedge is mandate-resistance, not visual
novelty. But the app needs ONE deliberate signature so it isn't anonymous.
Natural candidate: the identicon system (on every avatar, every screen; no
competitor owns it) plus the verified-✓ treatment. Design deferred to
DESIGN.md, tracked as a TODO — not a v1 blocker.

### Design system gate (Pass 5)

There is no DESIGN.md yet; wireframe CSS is `system-ui` + gray boxes by
admission, not a system. **composeApp UI work (step 6) is gated on running
/design-consultation first** to produce DESIGN.md: typeface, color tokens,
spacing scale, identicon algorithm, motion. Engine/FFI steps (0–4) do not
depend on this and proceed now. Building screens before tokens means
per-screen ad-hoc styling that never reconciles.

### Responsive + accessibility (Pass 6)

- **One breakpoint at 700dp:** below = single pane (list OR chat); at/above =
  two-pane (list + chat), per wireframe-v1 frame E. Same composables, no
  desktop-only chrome.
- **A11y baseline, specified from day one (Compose `semantics {}`, not
  retrofitted):** screen-reader labels (TalkBack / desktop SR) on every
  control, 48dp minimum touch targets, 4.5:1 contrast on body text, full
  keyboard navigation on desktop.

## Parallel work split (2 people: backend + frontend)

To let both people work without blocking each other, the FFI contract was
pulled to the FRONT as a stub (`ffi/` crate, landed 2026-07-11) — see
`docs/ffi-contract.md`. This re-sequences the solo-ordering below: the frontend
builds the whole app against the frozen `ChatEngine` interface (in-memory stub)
while the backend fills in the real engine behind the same signatures.

- **Backend owns:** `proto/ core/ engine/ relay/ cli/ ffi/`. Freezes the `ffi/`
  interface; changes to it are announced + coordinated.
- **Frontend owns:** `app/` (composeApp + engine-kt Gobley + iosApp).
- Disjoint directories → direct pushes to master rarely conflict. CI (T1) runs
  on every push and PR.

The migration steps below are the BACKEND track. The frontend track is:
scaffold `app/` → prove the Gobley 3-target gate (step 4 here) → build wireframe
screens against the stub bindings → real behavior arrives when backend swaps the
stub for `engine/` (T6), no frontend change.

## Migration steps (ordered, each independently shippable)

Reordered after the outside-voice review: wire/relay hardening moved AHEAD of
the engine extraction (OV2/OV3/OV6/OV9 all touch code the extraction moves and
the FFI then freezes — fixing them first is the cheapest point forever). Status
as of 2026-07-11: step 0 (T1 ci.yml) DONE; T5 relay bug fixes DONE; `ffi/`
stub contract DONE (ahead of order, to unblock frontend); T2 TLS + relay-cert
pinning DONE (relay presents a persistent self-signed cert, clients pin its
SHA-256 fingerprint carried in the ContactLink Endpoint; convergence test runs
over real TLS + a fingerprint-mismatch rejection test); T3 request-id
correlation DONE (ClientFrame/ServerFrame wrappers in proto/, relay echoes the
id, client fully pipelined via a pending-reply map — Push split out of
ServerMessage so a push can never be mistaken for a reply); T4
redelivery/ack DONE (subscribe drains the unacked backlog to the subscribing
connection, delete-on-ack frees storage and ends redelivery, all clients ack
after processing — at-least-once delivery, duplicates absorbed client-side
per OV5). Step 1 wire/relay hardening COMPLETE. T6 engine extraction DONE
(2026-07-12): `engine/` crate owns ChatEngine (pairing, send, epoch-conflict
auto-retry per OV4, seen-set dedup per OV5, reconnect + resubscribe +
backlog replay), cli/ is a thin REPL over it, convergence/pinning/pipelining
tests moved to engine/tests plus new commit-retry, dedup, and
proxy-severed-connection reconnect tests. T7 DONE (2026-07-12): ffi/ stub
replaced by the real engine behind the unchanged frozen signatures (engine
actor thread owns a current-thread tokio runtime; callbacks delivered only
via the dedicated chat-ffi-dispatch thread per OV8; loopback-relay
callback-delivery test proves the full stack — see ffi-contract.md
"Real-engine behavior notes" for the two additive EngineError variants and
the CHAT_RELAY_ADDR/CHAT_RELAY_FINGERPRINT bootstrap knob). T9 DONE
(2026-07-12): relay queue/epoch/backlog state persists via write-through
SQLite (rusqlite bundled; group sends transactional so an epoch advance and
its fan-out survive a crash together), unlocking relay/Dockerfile +
release-relay.yml; full-stack proof in engine/tests/relay_restart.rs (relay
runtime hard-dropped mid-conversation, restarted from disk, conversation
resumes through the engines' reconnect loops). **The backend track (steps
0–5) is COMPLETE. T8 (app scaffold) is the frontend track; step 6 UI work
remains gated on DT4 (/design-consultation → DESIGN.md).**

0. **`ci.yml` (cargo-only) lands first** (OV10): `cargo test --workspace` on
   every push, so the convergence test guards every step below. Gradle jobs
   join at step 3.
1. **Harden the wire + relay** (pulled forward by review OV1–OV9):
   - **TLS via rustls** on the relay connection (OV2). Relay identity = pinned
     public key carried in the `ContactLink` `Endpoint` (the versioned,
     additive format exists for exactly this). Kills the cleartext
     `QueueCreated{send_key}` exposure and on-path snooping.
   - **Request-id correlation** in `proto/` (OV6): `ClientMessage` carries a
     `request_id` echoed in replies; relay dispatch + client pipeline it.
     Done now, while zero clients are deployed — later it's a wire break.
   - **Redelivery-on-resubscribe + real ack semantics** (OV3, pulls T3
     forward): `Subscribe` drains the pending backlog to the new connection;
     delete-on-ack actually runs (today nothing ever acks — `mailbox_key` is
     dead code in every client).
   - **Fix three verified relay bugs** (OV9): subscriber-sender leak on
     reconnect (state.rs:170 appends, never prunes dead senders — and the doc
     says "replaces" while the code appends), unbounded
     `outstanding_challenges` growth (PoW-challenge flood = OOM), and the
     resulting double-delivery on re-subscribe.
2. **Extract `engine/`** from cli/ (scope per review OV3+4+5+9 decision):
   - Move `relay_client.rs` + orchestration behind a `ChatEngine`-shaped API;
     cli/ becomes a thin shell.
   - **NEW auto-retry conflict loop** (OV4): discard pending → process winner
     → rebuild commit at new epoch → resend. This is new logic (today it's
     manual test choreography), so it gets its OWN test; the existing
     3-client convergence test stays alive against the low-level engine API
     as the actual regression net — non-negotiable.
   - **Foreign-duplicate dedup** (OV5): engine-level seen-message-id set
     (in-memory now, persisted at T5), because `process_incoming` hard-errors
     on MLS replay and redelivery makes duplicates normal, not exceptional.
   - Reconnect/resubscribe test: kill the connection mid-session; engine
     reconnects, re-subscribes the full queue set, replays the backlog, no
     loss past the ack point, no duplicate surfaced to the caller.
   - Module doc-comments referencing cli/ paths move/update with the code.
3. **Add `ffi/`**: create engine, pair (produce/consume link), send, event
   callback via the dedicated dispatch thread. Rust-side callback-delivery
   test: register an EventListener, drive one message through a loopback
   relay, assert the callback fires with the decrypted payload.
4. **Scaffold `app/`** with Gobley — **split go/no-go gate (1A + OV7)**:
   - FFI smoke test (create engine → produce contact link → non-empty) per
     target, wired into `ci.yml`.
   - **Android + desktop green → UI work unblocks.** The dogfood loop is
     never hostage to iOS.
   - **iOS simulator: 5-working-day timebox.** Still red at the deadline →
     iOS drops to a tracked spike, UI proceeds, and the fallback decision
     (vanilla UniFFI + hand cinterop shim) is made explicitly.
5. **Relay persistence milestone** (OV1): queue/epoch/pending state in
   SQLite or sled, restart-survivable. **Gates** `relay/Dockerfile` +
   `release-relay.yml` — no operator-facing relay artifact before this.
6. Feature work on the proven skeleton: wireframe screens in composeApp,
   SQLCipher client persistence (T5/T8), pairing hardening (T4).

## What already exists (reused, not rebuilt)

- `proto/`, `core/`, `relay/` carry over unchanged in role and boundaries.
- cli/'s orchestration IS the engine's first implementation — extracted, not
  rewritten.
- The convergence test carries over as the engine's flagship test.
- wireframe-v0 is the UI spec for composeApp's first two screens.

## NOT in scope (considered, deferred)

- **Web client** — fourth toolchain, wrong audience for the wedge.
- **Relay federation/discovery** — multi-endpoint links (A2) already reserve
  the wire format; operational story is post-beta.
- **Play Store pipeline** — blocked on TODOS #2 (CEO-level decision).
- **iOS distribution** — blocked on TODOS #5; only the build target ships now.
- **SQLCipher persistence** — T5/T8, next milestone after app skeleton; the
  layout already names its home (core storage provider + engine wiring).
- **Push notifications (FCM/UnifiedPush/APNs)** — needs its own design pass;
  interacts with TODOS #5 and the no-identifier promise.
- **Splitting the monorepo** — revisit only if external platform teams appear.

(Pulled INTO scope by this review, previously implicit deferrals: transport
security → step 1; relay redelivery/ack (T3) → step 1; relay persistence →
step 5.)

## Implementation Tasks
Synthesized from this review's findings. Each task derives from a specific
finding above. Run with Claude Code or Codex; checkbox as you ship.

- [x] **T1 (P1, human: ~1d / CC: ~30min)** — ci — `ci.yml` with `cargo test --workspace` on push
  - Surfaced by: Outside voice OV10 — regression net must run during, not after, the refactor
  - Files: `.github/workflows/ci.yml`
  - Verify: convergence test runs on a push to a branch
- [x] **T2 (P1, human: ~2d / CC: ~1h)** — proto+relay+cli — TLS via rustls; relay pubkey pinned in ContactLink Endpoint
  - Surfaced by: Outside voice OV2 — cleartext send_key in QueueCreated (proto/src/wire.rs:93-96)
  - Files: `proto/src/link.rs`, `relay/src/net.rs`, `cli/src/relay_client.rs`
  - Verify: e2e test over TLS; plaintext socket rejected
- [x] **T3 (P1, human: ~1d / CC: ~45min)** — proto+relay+cli — request-id correlation + client pipelining
  - Surfaced by: Outside voice OV6 — "next frame wins" reply matching (cli/src/relay_client.rs:44-53)
  - Files: `proto/src/wire.rs`, `relay/src/state.rs`, `cli/src/relay_client.rs`
  - Verify: test with two concurrent in-flight requests resolving correctly
- [x] **T4 (P1, human: ~2d / CC: ~1h)** — relay — redelivery-on-resubscribe + delete-on-ack
  - Surfaced by: Outside voice OV3 — subscribe never drains pending (relay/src/state.rs:167-190); nothing acks
  - Files: `relay/src/state.rs`
  - Verify: unit test — enqueue while unsubscribed, resubscribe, receive backlog, ack, storage freed
- [x] **T5 (P1, human: ~1d / CC: ~30min)** — relay — fix subscriber leak, unbounded PoW challenge map, subscribe append-vs-replace
  - Surfaced by: Outside voice OV9 — state.rs:170 appends senders forever; state.rs:83-90 unbounded map
  - Files: `relay/src/state.rs`
  - Verify: unit tests for each (dead-sender pruned, challenge cap, no double-delivery)
- [x] **T6 (P1, human: ~1w / CC: ~2-3h)** — engine — extract crate; auto-retry conflict loop + foreign-dup seen-set, each with own test; convergence test kept as low-level net
  - Surfaced by: D4 + Outside voice OV4/OV5 — retry is manual test choreography today (cli/tests/three_client_convergence.rs:146-166); process_incoming errors on replay
  - Files: `engine/` (new), `cli/`, `Cargo.toml`
  - Verify: `cargo test -p engine` — convergence + retry-loop + reconnect + dedup tests green
- [x] **T7 (P2, human: ~3d / CC: ~1-2h)** — ffi — UniFFI crate, dispatch-thread event delivery, callback test
  - Surfaced by: Architecture review + OV8 threading contract
  - Files: `ffi/` (new)
  - Verify: Rust-side callback-delivery test green
- [ ] **T8 (P2, human: ~1w / CC: ~3-4h)** — app — Gobley scaffold, 3 targets, split gate (Android+desktop unblock; iOS 5-day timebox)
  - Surfaced by: D5 + review 1A + OV7 resolution
  - Files: `app/` (new)
  - Verify: FFI smoke test per target in ci.yml
- [x] **T9 (P2, human: ~3d / CC: ~1-2h)** — relay — persistence (SQLite/sled); gates Dockerfile + release-relay.yml
  - Surfaced by: Outside voice OV1 — in-memory RelayState bricks links on restart
  - Files: `relay/src/state.rs`, `relay/Dockerfile`
  - Verify: kill -9 relay mid-conversation, restart, conversation resumes

### Design tasks (from /plan-design-review)
- [ ] **DT1 (P2, human: ~1d / CC: ~1h)** — app — pairing flow screens (EmptyState=QR, AddContact My-code/Scan, Verification sheet) per wireframe-v1
- [ ] **DT2 (P2, human: ~1d / CC: ~1h)** — app — interaction states (reconnect banner, failed-send bubble, key-change system line, skeleton loading)
- [ ] **DT3 (P2, human: ~4h / CC: ~30min)** — app — 700dp breakpoint + a11y baseline (semantics, 48dp, 4.5:1, keyboard)
- [ ] **DT4 (P3, human: ~1d)** — design — run /design-consultation for DESIGN.md tokens + identicon signature (gates step 6)

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | — | — |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | — | — |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | CLEAR (PLAN) | 7 issues, 0 critical gaps open — all folded into plan (D3-D5, 1A, OV1-OV10) |
| Design Review | `/plan-design-review` | UI/UX gaps | 1 | CLEAR | score 3/10 → 8/10, 6 decisions added (nav map, states, journey, breakpoint, a11y, design-system gate) |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | — | — |

**CROSS-MODEL:** Outside voice (Claude subagent; codex usage-limited) found 10 issues the primary review missed or understated; 6 adopted verbatim (OV1,2,3,6,9,10), 2 adopted with modification (OV4/5 folded into step 2; OV7 resolved as split-gate compromise preserving iOS-day-one), 1 was a doc addition (OV8). One genuine tension (OV7 iOS-day-one) resolved by user decision: keep iOS, split the gate.

**VERDICT:** ENG + DESIGN CLEARED — ready to implement (step 0: ci.yml, then step 1: wire hardening; UI steps gated on /design-consultation per DT4).

NO UNRESOLVED DECISIONS
