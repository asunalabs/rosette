# Command & configuration reference

Every runnable surface in this repo: the `relay` daemon, the `cli` chat client,
and the environment variables the app/FFI layer reads. All claims here trace to
code; file paths point at the source of truth.

## `relay` — store-and-forward daemon

```
relay [LISTEN_ADDR] [IDENTITY_FILE] [STATE_DB]
```

Positional arguments (source: `relay/src/main.rs`):

| # | Argument | Default | Effect |
|---|----------|---------|--------|
| 1 | `LISTEN_ADDR` | `127.0.0.1:7443` | TCP address to listen on. Use `0.0.0.0:7443` to accept non-local clients. |
| 2 | `IDENTITY_FILE` | `relay_identity.der` | The relay's persistent self-signed TLS certificate. Created on first start, reused after. Deleting it changes the fingerprint and **breaks every contact link that pinned it**. |
| 3 | `STATE_DB` | `relay_state.sqlite3` | SQLite database holding queues, epochs, and undelivered backlogs. Restarts (including `kill -9`) are invisible to clients as long as this file survives. |

On startup the relay prints one line you need:

```
relay TLS fingerprint: <64 hex chars>
```

Clients pin this fingerprint; it is also embedded in every contact link minted
through this relay. Distribute it alongside the relay address.

Logging: standard `tracing` env filter, e.g. `RUST_LOG=info`.

### Built-in limits

Protocol-level, not config (source: `proto/src/limits.rs`). Clients are written
to handle each rejection, so hitting a limit degrades politely rather than
crashing anything.

| Limit | Value | Rejection |
|-------|-------|-----------|
| Max message size | 64 KiB (largest padding bucket) | `MessageTooLarge` |
| Max undelivered entries per queue | 1,000 | `QueueFull` |
| Total storage per relay instance | 10 GiB | `StorageBoundExceeded` |
| Sends per queue per rolling minute | 60 | `RateLimited` |
| Queue-creation proof-of-work | 16 leading zero bits | `InvalidProofOfWork` |
| Outstanding unsolved PoW challenges | 10,000 (FIFO eviction) | — |
| Undelivered message TTL | 14 days | silently dropped |

Raising a limit means editing `proto/src/limits.rs` and rebuilding — they are
compile-time constants in v0.1.

## `cli` — dogfood chat client

A thin REPL over `engine::ChatEngine` (source: `cli/src/main.rs`). Two
subcommands, one per side of the pairing handshake.

### `cli listen`

```
cli listen --relay-fingerprint <64-HEX> [--name <NAME>] [--relay <ADDR>]
```

| Flag | Default | Effect |
|------|---------|--------|
| `--relay-fingerprint` | required | The fingerprint the relay printed on startup. Baked into the contact link so the peer pins the same relay. |
| `--name` | `alice` | Display name carried in the MLS credential. Decoration only — never a network identifier. |
| `--relay` | `127.0.0.1:7443` | Relay address to connect to. |

Prints `Share this link: <base64>` and waits to be paired.

### `cli connect`

```
cli connect <LINK> [--name <NAME>]
```

| Arg/Flag | Default | Effect |
|----------|---------|--------|
| `LINK` | required | The base64 contact link printed by `listen`. Carries the relay address, fingerprint, and pairing material — no other flags needed. |
| `--name` | `bob` | Display name, as above. |

### The chat REPL

Both subcommands land in the same loop: type a line + Enter to send, Ctrl-D to
quit. Incoming output:

| Line | Meaning |
|------|---------|
| `< hello` | Decrypted message from the peer. |
| `(group state updated, epoch N)` | An MLS commit was applied (key rotation). |
| `(connection lost — reconnecting…)` / `(reconnected)` | The engine's reconnect loop; queued messages replay after reconnect, no loss, no duplicates. |

## App / FFI environment variables

Read by the `ffi/` crate when the Kotlin app calls `create_contact_link`
(source: `docs/reference/ffi-contract.md`, `ffi/src/lib.rs`):

| Variable | Format | Effect |
|----------|--------|--------|
| `CHAT_RELAY_ADDR` | `host:port` | The home relay the app mints its mailbox on. |
| `CHAT_RELAY_FINGERPRINT` | 64 hex chars | That relay's TLS fingerprint. |

Unset or unreachable → `create_contact_link` returns an **empty string** and
emits `ConnectionStateChanged { online: false }` (the frozen signature is
infallible by design). `pair_with_link` needs neither variable — the scanned
link carries its own relay + fingerprint.

## Related

- [Getting started tutorial](guide-getting-started.md) — see all of this working in three steps
- [How to run a relay](howto-run-a-relay.md) — operator's guide (binary + Docker)
- [How to chat from the CLI](howto-chat-from-the-cli.md)
- [architecture.md](architecture.md) — why the pieces are shaped this way
- [ffi-contract.md](ffi-contract.md) — the full frozen `ChatEngine` interface
