# How to run a relay

Stand up a store-and-forward relay that clients can pair and chat through. The
relay is content-blind: it sees encrypted envelopes and queue ids, never
plaintext, names, or membership.

## Prerequisites

- **From source:** Rust stable toolchain (`rustup`), this repo cloned.
- **Docker path:** Docker only.
- A port reachable by your clients (default `7443`).

## Option A: run the binary

1. Build and start:

   ```bash
   cargo run --release -p relay
   ```

   For non-local clients, bind all interfaces:

   ```bash
   cargo run --release -p relay -- 0.0.0.0:7443
   ```

2. Capture the fingerprint from the first output line:

   ```
   relay TLS fingerprint: 3fa4c2…(64 hex chars)
   ```

   Clients need this string to connect. It's stable across restarts because
   the identity is persisted (next step).

3. Know your two files, created in the working directory on first start:

   - `relay_identity.der` — the TLS identity behind that fingerprint.
     **Back this up.** Losing it means a new fingerprint, which invalidates
     every contact link that pinned the old one.
   - `relay_state.sqlite3` — queues, epochs, undelivered messages. Losing it
     bricks existing conversations' queues (clients must re-pair).

   Custom paths: `relay 0.0.0.0:7443 /srv/chat/identity.der /srv/chat/state.sqlite3`.

## Option B: run the container

Build from the **repo root** (the workspace manifest is needed):

```bash
docker build -f relay/Dockerfile -t chat-relay .
docker run -d -v chat-relay-data:/data -p 7443:7443 chat-relay
```

Identity and state both live in `/data` — the named volume is what makes
restarts and redeploys invisible to clients. Read the fingerprint with:

```bash
docker logs <container> | grep fingerprint
```

## Verification

From the repo, pair two clients through your relay (replace the fingerprint):

```bash
cargo run -p cli -- listen --relay <your-host>:7443 --relay-fingerprint <64-hex>
```

If it prints `Share this link: …`, the relay accepted a TLS connection,
minted a mailbox (proof-of-work and all), and subscribed it. Full two-client
check: the [getting started tutorial](guide-getting-started.md).

## Restarts and upgrades

State is write-through SQLite: `kill -9`, restart, and clients reconnect and
replay their unacked backlog with no loss and no duplicates (covered by
`engine/tests/relay_restart.rs`). Just keep `relay_identity.der` and
`relay_state.sqlite3` (or the `/data` volume) intact across upgrades.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Client fails TLS handshake | Fingerprint mismatch — wrong or stale fingerprint | Re-read the fingerprint from relay startup logs; regenerate contact links if the identity file was replaced. |
| `Share this link` never prints | Relay unreachable (wrong addr/port, firewall) | Check the relay is bound to `0.0.0.0`, port open, and `--relay` points at it. |
| `StorageBoundExceeded` rejections | 10 GiB default storage cap hit | Cap is a compile-time constant (`proto/src/limits.rs`) — raise and rebuild, or clear delivered backlog by letting clients ack. |
| Fingerprint changed after redeploy | Identity file not on a persistent volume | Mount `/data` (Docker) or keep `relay_identity.der` on disk. Existing links are unrecoverable — clients re-pair. |

## Related

- [Command reference](reference-commands.md) — all args, files, and limits
- [architecture.md](architecture.md) — the relay's role (content-blind, epoch-aware DS)
