# How to run a relay

Stand up a store-and-forward relay that Rosette clients can pair and chat
through. The relay is content-blind: it sees encrypted envelopes and queue
ids, never plaintext, names, or membership.

## Prerequisites

- **From source:** Rust stable toolchain (`rustup`), this repo cloned.
- **Docker path:** Docker only.
- A TCP port reachable by your clients (default `7443`).
- **No TLS certificate and no domain needed** — the relay generates its own
  TLS identity and clients pin its fingerprint (see below).

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
     **This is a private key** (see "Protect the identity file").
   - `relay_state.sqlite3` — queues, epochs, undelivered messages. Losing it
     bricks existing conversations' queues (clients must re-pair).

   Custom paths: `relay 0.0.0.0:7443 /srv/rosette/identity.der /srv/rosette/state.sqlite3`.

## Option B: run the container

Build from the **repo root** (the workspace manifest is needed):

```bash
docker build -f relay/Dockerfile -t rosette-relay .
docker run -d --name rosette-relay --restart unless-stopped \
  -v rosette-relay-data:/data -p 7443:7443 rosette-relay
```

Identity and state both live in `/data` — the named volume is what makes
restarts and redeploys invisible to clients. Read the fingerprint with:

```bash
docker logs rosette-relay | grep fingerprint
```

## TLS is built in — don't terminate it in front of the relay

The relay speaks TLS itself, and clients authenticate it by **pinning the
fingerprint** carried in contact links — not through CA certificates. Two
consequences:

- Do **not** put the relay behind a TLS-terminating reverse proxy or CDN
  (nginx `proxy_pass` with TLS, Caddy, Cloudflare orange-cloud). Clients
  would see the proxy's certificate, the pin check fails, nobody connects.
  If you must route through something, it has to be plain TCP passthrough
  (nginx `stream {}`, HAProxy `mode tcp`).
- There is nothing to renew, ever. The identity file is the cert and key.

## Protect the identity file

`relay_identity.der` is the relay's private key. Anyone holding it can
impersonate your relay to every client that pinned your fingerprint.

- `chmod 600 relay_identity.der`, owned by the service user.
- Back it up offline. Losing it means a new fingerprint, which invalidates
  every contact link that pinned the old one — clients must re-pair.
- If it leaks, treat the relay as burned: delete it, start with a fresh
  identity, and tell your users to re-pair.

## Optional: the attestation gate (requires a directory service)

By default, anyone can create queues after solving a proof-of-work
challenge. If you also run the identity/directory service, the relay can
additionally require a directory-signed attestation token ("this caller
verified a phone number") for queue creation:

1. Start the directory service — it logs its attestation public key
   (base64) at startup.
2. Start the relay with that key in the environment:

   ```bash
   RELAY_ATTESTATION_PUBKEY=<base64-from-directory-log> relay 0.0.0.0:7443
   ```

   Startup prints `attestation gate ENABLED (queue creation requires a
   directory token)`.
3. Leave the variable unset and the gate stays off (PoW-only) — the right
   choice for a standalone community relay with no directory.

Hosting the directory service itself (Postgres, Caddy, Twilio Verify,
`DIRECTORY_PEPPER`): [directory/deploy/README.md](../../directory/deploy/README.md).

## Run it as a service (systemd)

`/etc/systemd/system/rosette-relay.service`:

```ini
[Unit]
Description=Rosette relay
After=network-online.target
Wants=network-online.target

[Service]
User=rosette
WorkingDirectory=/srv/rosette
ExecStart=/srv/rosette/relay 0.0.0.0:7443 /srv/rosette/relay_identity.der /srv/rosette/relay_state.sqlite3
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```bash
cargo build --release -p relay
sudo useradd -r -d /srv/rosette rosette && sudo mkdir -p /srv/rosette
sudo cp target/release/relay /srv/rosette/ && sudo chown -R rosette: /srv/rosette
sudo systemctl enable --now rosette-relay
journalctl -u rosette-relay | grep fingerprint
```

## Sizing and firewall

- A 1 vCPU / 1 GB VPS is plenty to start: the relay is I/O-light (embedded
  SQLite, padded envelopes) and storage is capped at 10 GiB by default.
- Firewall: allow inbound TCP on your relay port (and SSH); nothing else.
  The relay makes no outbound connections.

## Verification

From the repo, pair two clients through your relay (replace the fingerprint):

```bash
cargo run -p cli -- listen --relay <your-host>:7443 --relay-fingerprint <64-hex>
```

If it prints `Share this link: …`, the relay accepted a TLS connection,
minted a mailbox (proof-of-work and all), and subscribed it. Full two-client
check: the [getting started tutorial](../tutorials/getting-started.md).

## Restarts and upgrades

State is write-through SQLite: `kill -9`, restart, and clients reconnect and
replay their unacked backlog with no loss and no duplicates (covered by
`engine/tests/relay_restart.rs`). Just keep `relay_identity.der` and
`relay_state.sqlite3` (or the `/data` volume) intact across upgrades — swap
the binary, restart the service, done.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Client fails TLS handshake | Fingerprint mismatch — wrong or stale fingerprint | Re-read the fingerprint from relay startup logs; regenerate contact links if the identity file was replaced. |
| Client fails TLS handshake behind a proxy | TLS terminated in front of the relay | Remove the proxy or switch it to plain TCP passthrough — see "TLS is built in". |
| `Share this link` never prints | Relay unreachable (wrong addr/port, firewall) | Check the relay is bound to `0.0.0.0`, port open, and `--relay` points at it. |
| `StorageBoundExceeded` rejections | 10 GiB default storage cap hit | Cap is a compile-time constant (`proto/src/limits.rs`) — raise and rebuild, or clear delivered backlog by letting clients ack. |
| Fingerprint changed after redeploy | Identity file not on a persistent volume | Mount `/data` (Docker) or keep `relay_identity.der` on disk. Existing links are unrecoverable — clients re-pair. |

## Related

- [Command reference](../reference/commands.md) — all args, files, and limits
- [Architecture](../explanation/architecture.md) — the relay's role (content-blind, epoch-aware DS)
- [Directory service deploy](../../directory/deploy/README.md) — the optional identity service behind the attestation gate
