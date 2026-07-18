# Rosette

A private messenger for the Chat Control era. Conversations are end-to-end
encrypted with MLS (OpenMLS); relays store-and-forward padded ciphertext and
can't read names, content, or membership. Built for mainstream EU users who
want a familiar messenger that doesn't scan — not a hobbyist threat-model
exercise.

> If we were ever forced to choose between giving government access to user
> data or shutting down, we would shut down.

The name is the identicon: every contact renders as a deterministic
guilloché **rosette** — the anti-forgery engraving on banknotes and
passports — derived from their key fingerprint. Hard to forge, easy to
recognize.

Status: working protocol skeleton. Relay, encrypted 1:1 chat over the CLI,
and the Kotlin Multiplatform app shell all run today, paired by exchanging a
QR/contact link — no phone, no email, no account required for the core path.
A Signal-model identity/directory pivot is implemented as a standalone
service (`directory/`): phone required at signup (Argon2id-hashed,
verification-only, hidden by default), k-anonymity-bucketed phone/username
search opt-in (off by default), backed by PostgreSQL. Not yet wired to the
client or to `core`/`engine`'s pairing flow — see
[docs/plans/tasks-identity-directory-pivot.md](docs/plans/tasks-identity-directory-pivot.md)
for what's done versus open (T25 in particular: no bridge exists yet from a
directory search hit to an actual MLS pairing session).

**Free forever for individuals. No paid tier, ever.** Billing needs an
identity link, and this app's whole point is not having one.

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
| [Project guide](docs/explanation/project-guide.md) | Start here: what everything does, why it's shaped this way, current status |
| [Getting started](docs/tutorials/getting-started.md) | Tutorial: full stack running locally, first encrypted message |
| [How to run a relay](docs/how-to/run-a-relay.md) | Operators: binary or Docker, TLS/identity handling, systemd, attestation gate |
| [How to chat from the CLI](docs/how-to/chat-from-the-cli.md) | Pair and chat from the terminal |
| [Command reference](docs/reference/commands.md) | Every flag, file, env var, and protocol limit |
| [Architecture](docs/explanation/architecture.md) | The system design, crate boundaries, and migration plan |
| [FFI contract](docs/reference/ffi-contract.md) | The frozen `ChatEngine` interface between Rust and the app |
| [Identity/directory pivot plan](docs/plans/tasks-identity-directory-pivot.md) | Active task list for the Signal-model identity/directory pivot |

## Repo layout

```
proto/     wire protocol (single source of truth for the client-relay boundary)
core/      MLS core (the only crate that touches OpenMLS)
engine/    client engine: connection, reconnect, dedup, epoch-conflict retry
ffi/       UniFFI surface consumed by the app (frozen contract)
relay/     store-and-forward daemon (content-blind, epoch-aware), embedded SQLite
directory/ identity/directory service (phone verify, username search) — separate process, PostgreSQL, never imports core/engine
cli/       dogfood REPL — thin shell over engine/
app/       Kotlin Multiplatform app (Compose UI + Gobley bindings)
docs/      everything linked above (tutorials/, how-to/, reference/, explanation/, plans/, design/)
```

## Development

```bash
cargo test --workspace        # the whole Rust stack, incl. 3-client convergence
cd app && ./gradlew :engine-kt:desktopTest   # FFI smoke test across the seam
```

`directory/`'s tests need a real Postgres reachable via `DATABASE_URL` (each
test gets its own ephemeral DB via `sqlx::test`):

```bash
docker run -d --name rosette-directory-postgres -e POSTGRES_PASSWORD=devpassword \
  -e POSTGRES_USER=directory -e POSTGRES_DB=directory -p 5432:5432 postgres:16-alpine
export DATABASE_URL="postgres://directory:devpassword@localhost:5432/directory"
cargo test --workspace
```

Running the directory service itself also needs `DIRECTORY_PEPPER` (or
`DIRECTORY_ALLOW_DEV_PEPPER=1` for local dev only):

```bash
DIRECTORY_ALLOW_DEV_PEPPER=1 cargo run -p directory   # listens on 127.0.0.1:7444
```

CI runs both on every push (`.github/workflows/ci.yml`). Backend and frontend
tracks work independently against the [FFI contract](docs/reference/ffi-contract.md).

## Security

Pre-audit software — see [SECURITY.md](SECURITY.md) for how to report
vulnerabilities privately. The external audit gates public beta.

## License

[GNU AGPL-3.0](LICENSE). Chosen so a modified relay or directory service run
as a hosted network service must release its source too — the same
anti-freeriding logic Signal applies to its own server code, and a match for
this project's free-forever, non-commercial posture.
