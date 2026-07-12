# chat (working name)

A private messenger with **no identifiers** — no phone number, no email, no
account. Pairing happens by exchanging a QR/contact link; conversations are
end-to-end encrypted with MLS (OpenMLS); relays store-and-forward padded
ciphertext and can't read names, content, or membership.

Status: working protocol skeleton. Relay, encrypted 1:1 chat over the CLI,
and the Kotlin Multiplatform app shell all run today; real app screens are in
progress.

## Try it

Three terminals, ~5 minutes: **[Getting started](docs/guide-getting-started.md)**.

```bash
cargo run -p relay                                        # 1: relay (prints its fingerprint)
cargo run -p cli -- listen --relay-fingerprint <hex>      # 2: alice → prints contact link
cargo run -p cli -- connect "<link>"                      # 3: bob → chat
```

## Documentation

| Doc | What it's for |
|-----|---------------|
| [Getting started](docs/guide-getting-started.md) | Tutorial: full stack running locally, first encrypted message |
| [How to run a relay](docs/howto-run-a-relay.md) | Operators: binary or Docker, persistence, fingerprint handling |
| [How to chat from the CLI](docs/howto-chat-from-the-cli.md) | Pair and chat from the terminal |
| [Command reference](docs/reference-commands.md) | Every flag, file, env var, and protocol limit |
| [Architecture](docs/architecture.md) | The system design, crate boundaries, and migration plan |
| [FFI contract](docs/ffi-contract.md) | The frozen `ChatEngine` interface between Rust and the app |

## Repo layout

```
proto/    wire protocol (single source of truth for the client-relay boundary)
core/     MLS core (the only crate that touches OpenMLS)
engine/   client engine: connection, reconnect, dedup, epoch-conflict retry
ffi/      UniFFI surface consumed by the app (frozen contract)
relay/    store-and-forward daemon (content-blind, epoch-aware)
cli/      dogfood REPL — thin shell over engine/
app/      Kotlin Multiplatform app (Compose UI + Gobley bindings)
docs/     everything linked above
```

## Development

```bash
cargo test --workspace        # the whole Rust stack, incl. 3-client convergence
cd app && ./gradlew :engine-kt:desktopTest   # FFI smoke test across the seam
```

CI runs both on every push (`.github/workflows/ci.yml`). Backend and frontend
tracks work independently against the [FFI contract](docs/ffi-contract.md).
