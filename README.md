# chat (working name)

A private messenger. Conversations are end-to-end encrypted with MLS
(OpenMLS); relays store-and-forward padded ciphertext and can't read names,
content, or membership.

Status: working protocol skeleton. Relay, encrypted 1:1 chat over the CLI,
and the Kotlin Multiplatform app shell all run today, paired by exchanging a
QR/contact link — no phone, no email, no account, real app screens are in
progress. A Signal-model identity/directory pivot is underway (not yet
implemented): phone required at signup (hashed, verification-only, hidden by
default), username as the public/searchable discovery surface, no email. See
[docs/plans/tasks-identity-directory-pivot.md](docs/plans/tasks-identity-directory-pivot.md).

## Try it

Three terminals, ~5 minutes: **[Getting started](docs/tutorials/getting-started.md)**.

```bash
cargo run -p relay                                        # 1: relay (prints its fingerprint)
cargo run -p cli -- listen --relay-fingerprint <hex>      # 2: alice → prints contact link
cargo run -p cli -- connect "<link>"                      # 3: bob → chat
```

## Documentation

| Doc | What it's for |
|-----|---------------|
| [Getting started](docs/tutorials/getting-started.md) | Tutorial: full stack running locally, first encrypted message |
| [How to run a relay](docs/how-to/run-a-relay.md) | Operators: binary or Docker, persistence, fingerprint handling |
| [How to chat from the CLI](docs/how-to/chat-from-the-cli.md) | Pair and chat from the terminal |
| [Command reference](docs/reference/commands.md) | Every flag, file, env var, and protocol limit |
| [Architecture](docs/explanation/architecture.md) | The system design, crate boundaries, and migration plan |
| [FFI contract](docs/reference/ffi-contract.md) | The frozen `ChatEngine` interface between Rust and the app |
| [Identity/directory pivot plan](docs/plans/tasks-identity-directory-pivot.md) | Active task list for the Signal-model identity/directory pivot |

## Repo layout

```
proto/    wire protocol (single source of truth for the client-relay boundary)
core/     MLS core (the only crate that touches OpenMLS)
engine/   client engine: connection, reconnect, dedup, epoch-conflict retry
ffi/      UniFFI surface consumed by the app (frozen contract)
relay/    store-and-forward daemon (content-blind, epoch-aware)
cli/      dogfood REPL — thin shell over engine/
app/      Kotlin Multiplatform app (Compose UI + Gobley bindings)
docs/     everything linked above (tutorials/, how-to/, reference/, explanation/, plans/, design/)
```

## Development

```bash
cargo test --workspace        # the whole Rust stack, incl. 3-client convergence
cd app && ./gradlew :engine-kt:desktopTest   # FFI smoke test across the seam
```

CI runs both on every push (`.github/workflows/ci.yml`). Backend and frontend
tracks work independently against the [FFI contract](docs/reference/ffi-contract.md).
