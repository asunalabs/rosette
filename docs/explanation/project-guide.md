# Project guide — what everything does and why

Orientation for a human working in this repo alongside Claude. Other docs
are precise about one thing each (a tutorial, a command reference, the full
architecture decision log); this one is the map that tells you where to
look and why the pieces are shaped the way they are. When this guide and a
more specific doc disagree, the specific doc wins — update this map instead.

## The pitch, in one paragraph

A private messenger. Conversations are end-to-end encrypted with MLS
(OpenMLS) — relays store-and-forward padded ciphertext and can't read
names, content, or membership. The wedge is *mandate-resistance*: no
identifier the operator (or a court order aimed at the operator) can hand
over, because the operator never has it. That single constraint — "build so
there's nothing to hand over" — is why almost every non-obvious design
choice in this repo exists. Full status: [README.md](../../README.md).

## Repo layout: what each piece does and why it's separate

Ten directories, each a deliberate boundary. The dependency graph is
enforced (`proto ◀── core ◀── engine ◀── ffi`, `proto ◀── relay`,
`proto ◀── directory`) — a crate importing something it shouldn't is an
architecture bug, not a style nit, because the boundaries are what make the
"relay never sees plaintext" and "directory can crash without touching
messaging" promises structural instead of aspirational.

| Dir | What it does | Why it's its own crate |
|-----|--------------|------------------------|
| `proto/` | Wire types: envelope, framing, link (QR/contact-link format), proof-of-work challenges, auth, limits. The single source of truth for anything crossing the client↔relay boundary. | Everything else depends on it; it depends on nothing project-specific. Changing wire format is a one-place change. |
| `core/` | The only crate that touches OpenMLS. Identity, MLS sessions, MLS-native pairing, SQLCipher-backed storage provider. | Keeping MLS usage in one crate means one audit target for the actual cryptography, and means `relay`/`directory` can be content-blind by construction — they never link against OpenMLS at all. |
| `engine/` | The client engine: relay connection + reconnect loop, subscribe/backlog replay, own-echo and foreign-duplicate dedup, epoch-conflict auto-retry, write-through persistence. | This is the orchestration logic that used to live inline in `cli/`. Pulling it into its own crate means the CLI *and* the FFI surface share one implementation instead of two copies drifting apart. |
| `ffi/` | UniFFI surface exposed to the Kotlin app. Thin translation layer only — no logic of its own, just `engine/` objects plus a push-event callback interface. | This is the frozen contract between the Rust and Kotlin tracks (see [FFI contract](../reference/ffi-contract.md)) — freezing it at a crate boundary is what let backend and frontend work move in parallel without blocking each other. |
| `relay/` | Store-and-forward daemon. Content-blind but epoch-aware (enforces one accepted commit per epoch without reading plaintext). Embedded SQLite, restart-survivable. | Depends on `proto/` only — never on `core/` or `engine/`. That's not a convenience, it's the privacy promise: the relay operator's code literally cannot decrypt anything, because the crate that knows how to isn't in its dependency tree. |
| `cli/` | Dogfood REPL. `listen` prints a contact link, `connect` scans one, both land in the same chat prompt. | A thin shell over `engine::ChatEngine` — deliberately contains no protocol logic, so it can't drift out of sync with what the app ships. Doubles as the fastest way to manually exercise the stack. |
| `directory/` | Identity/directory service: phone verification (Argon2id-hashed, pepper-keyed, verification-only) and username search (k-anonymity hash-prefix bucketing, opt-in, off by default). HTTP/JSON API (axum), PostgreSQL. | A fifth crate *outside* the client↔relay path, not a variant of it. Depends on `proto/` only, never `core/`/`engine/` — checked via `cargo tree -p directory -e normal` — so a compromised or crashed directory process cannot touch message delivery. Real client-server Postgres (not relay's embedded-SQLite-per-operator pattern) because directory is one centrally-run service, not something every relay operator self-hosts. |
| `app/` | Kotlin Multiplatform app: Compose UI (`composeApp/`, currently a walking shell — Android/desktop entry points exist, real screens are gated on a design-tokens pass) + generated FFI bindings (`engine-kt/`, via Gobley). | Compose Multiplatform because one UI codebase needs to reach Android + desktop + iOS from day one (decision D5, [architecture.md](architecture.md)). |
| `docs/` | Everything documentation. See "How the docs are organized" below. | Diataxis (tutorial/how-to/reference/explanation) split, so a reader in "how do I do X" mode never has to wade through "why does X work this way" prose, and vice versa. |

**What deliberately doesn't exist:** a shared `common/`/`utils/` crate
(nothing needs a home `proto/` doesn't already provide), a backend API
server (the relay *is* the entire server surface for messaging), a web
client (a fourth toolchain for an audience the wedge doesn't target).

## Why the big decisions were made

Condensed from [architecture.md](architecture.md)'s full decision log —
read that doc for the complete rationale, amendment history, and the
outside-review findings that shaped it. The short version:

- **Monorepo, not multi-repo.** The client embeds the Rust core via FFI;
  cross-language changes (a new engine method needs a new Kotlin binding)
  are the common case. Atomic commits beat publish/consume ceremony for a
  small team.
- **Thick Rust core, thin FFI.** The hard correctness logic — epoch-conflict
  retry, echo dedup, at-least-once delivery — already exists and is tested
  in Rust. One implementation, one audit target, Kotlin never re-implements
  protocol behavior.
- **Content-blind relay by construction.** Not a policy the relay code
  promises to follow — a dependency-graph fact (`relay/` never imports
  `core/`). If the relay operator's binary can't link against MLS, it can't
  decrypt, full stop.
- **Directory is a fifth, isolated crate.** Same logic as the relay: a
  crash or compromise in `directory/` (the piece that touches phone numbers
  — the one identifier in the whole system) cannot cascade into messaging,
  because there's no import path from `directory/` into `core/`/`engine/`.
- **SQLCipher client-side persistence, nothing durable server-side.**
  Signal's model exactly: an encrypted SQLite DB on-device is the *only*
  durable store for messages/contacts/keys. Relay persistence (T9, done) is
  queue/epoch state for restart-survivability — never message history.
- **Free forever for individuals, no exceptions.** A paid tier requires an
  identity link (billing), which breaks the no-identifier promise the same
  way ads or data-sale would. This is now a locked, one-way decision — see
  [TODOS.md](../../TODOS.md) #6.

## Current status (as of 2026-07-14)

The fastest way to get an accurate, up-to-date status is `git log` and the
task lists below — this section is a snapshot, not the source of truth.

- **Backend track (protocol + relay + engine + FFI): complete through T9.**
  Wire hardening (TLS, request-id correlation, redelivery/ack), `engine/`
  extraction, `ffi/` real-engine wiring, relay SQLite persistence, and
  SQLCipher client persistence (T5/T8, landed 2026-07-14) are all done and
  tested. Full step-by-step status: [architecture.md → Migration
  steps](architecture.md#migration-steps-ordered-each-independently-shippable).
- **Frontend track: scaffold done, real screens not started.** `app/`
  builds and links on Android + desktop (FFI smoke test passes both
  targets); iOS needs Mac hardware and is a timeboxed spike. UI work is
  deliberately gated on a design-tokens pass (`/design-consultation` →
  `DESIGN.md`) so screens don't get built ad-hoc before there's a system —
  see TODOS.md #8.
- **Identity/directory pivot: T1–T10, T12–T13, T15–T26 done; T14 partial.**
  The directory service (phone verify + username search) is fully built,
  tested, and has a deploy story (Docker/EC2). What's *not* wired yet: the
  client never calls it, and — until T25 landed the backend half — there
  was no bridge from "found someone via search" to an actual MLS pairing
  session. T25 (2026-07-13/14) built that bridge server-side; the
  client-side wiring (generate a KeyPackage, upload it, consume one found
  via search) is still open. Full task-by-task detail:
  [tasks-identity-directory-pivot.md](../plans/tasks-identity-directory-pivot.md).
- **Reserved but not built:** E2EE calling (TODOS #10 — wire-format
  reservation for signaling is done, the feature itself is post-v1), push
  notifications (TODOS #7), proximity discovery (T11, split into its own
  spike).
- **Strategy-level open items** (non-engineering — legal posture, Play
  Store, growth mechanic, iOS distribution stance, funding) live in
  [TODOS.md](../../TODOS.md), not in code or here.

## How the docs are organized

```
docs/
├── tutorials/getting-started.md     Learning-oriented: 3 terminals, first encrypted message
├── how-to/
│   ├── run-a-relay.md               Task-oriented: operate a relay (binary or Docker)
│   └── chat-from-the-cli.md         Task-oriented: pair and chat from the terminal
├── reference/
│   ├── commands.md                  Every flag, file, env var, protocol limit
│   └── ffi-contract.md              The frozen ChatEngine interface (Rust ↔ Kotlin)
├── explanation/
│   ├── architecture.md              Full system design, crate boundaries, decision log, migration plan
│   └── project-guide.md             This document — the orientation map
├── plans/                           Durable, checked-in task lists and specs (see below)
└── design/                          Wireframes and the approved UI direction
```

`docs/plans/` is where a `/plan-*-review` or `/autoplan` pass writes task
lists derived from a review — e.g.
[tasks-identity-directory-pivot.md](../plans/tasks-identity-directory-pivot.md)
and [spec-identity-directory.md](../plans/spec-identity-directory.md) (the
canonical current-state spec for the identity pivot, separate from the task
checklist). It's the checked-in counterpart to the local, per-machine design
docs and CEO plans under `~/.gstack/projects/chat/` — check both when
looking for prior decisions or open questions.

## How work happens in this repo (alongside Claude)

- **`CLAUDE.md` routes requests to skills.** Product ideas → `/office-hours`,
  architecture → `/plan-eng-review`, bugs → `/investigate`, code review →
  `/review`, shipping → `/ship`. When in doubt, a skill gets invoked rather
  than freehanding the equivalent process — this keeps review/plan output
  in the same structured, checkable format project-wide.
- **Plan reviews leave a decision log, not just code.** `architecture.md`'s
  `D3`/`D4`/`D5`/`OV1`–`OV10` numbering and the `T*`/`DT*` task IDs in that
  file and in `tasks-identity-directory-pivot.md` are how a review's
  reasoning stays attached to the resulting code, so a later reader (human
  or Claude) can see *why*, not just *what*. When you see a `TN (P1, human:
  ~Xd / CC: ~Yh)` line, that's a task straight out of a review, ready to run.
- **"Ponytail" ceilings are marked inline.** Grep for `ponytail:` in the
  Rust source — e.g. `directory/src/search.rs`'s prefix-length constant —
  for deliberate v1 shortcuts with their stated upgrade path, so they don't
  quietly rot into permanent decisions nobody chose.
- **Two independent tracks, one CI gate.** Backend (`proto/ core/ engine/
  relay/ cli/ ffi/ directory/`) and frontend (`app/`) move in parallel
  against the frozen FFI contract; `.github/workflows/ci.yml` runs both
  (`cargo test --workspace` + the Gradle FFI smoke test) on every push.

## Where to go next

- Never run the app before: **[Getting started](../tutorials/getting-started.md)** — three terminals, ~5 minutes, one real encrypted message.
- Need to run or operate a relay: **[How to run a relay](../how-to/run-a-relay.md)**.
- Changing wire format, engine behavior, or the FFI surface: read **[architecture.md](architecture.md)** in full first — the dependency rules and threading contract there are load-bearing.
- Picking up identity/directory work: **[tasks-identity-directory-pivot.md](../plans/tasks-identity-directory-pivot.md)** for what's open (T14/T25's client half is the biggest gap), **[spec-identity-directory.md](../plans/spec-identity-directory.md)** for the current-state model.
- Strategy/non-engineering decisions: **[TODOS.md](../../TODOS.md)**.
