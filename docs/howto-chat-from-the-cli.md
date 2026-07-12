# How to chat from the CLI

Pair two people over a relay and exchange end-to-end encrypted messages from
the terminal. The CLI is the dogfood client — a thin shell over the same
`ChatEngine` the app ships, so what works here works in the app.

## Prerequisites

- Rust stable toolchain, this repo cloned.
- A running relay and its TLS fingerprint — yours
  ([how to run a relay](howto-run-a-relay.md)) or someone else's.

## Steps

1. **Listener side** — mint a contact link and wait:

   ```bash
   cargo run -p cli -- listen --name alice \
     --relay 127.0.0.1:7443 \
     --relay-fingerprint <64-hex-from-relay-startup>
   ```

   Output:

   ```
   Share this link: ARsBAAAAAAAAAAEABQAB…
   Waiting to be paired...
   ```

2. **Send the link to the other person** over any channel (in the app this is
   a QR code). The link carries the relay address, its fingerprint, and
   one-time pairing material — it is the whole bootstrap.

3. **Connector side** — consume the link:

   ```bash
   cargo run -p cli -- connect --name bob "ARsBAAAAAAAAAAEABQAB…"
   ```

   Both terminals print:

   ```
   Paired. Epoch 1.
   Type a message and press enter. Ctrl-D to quit.
   ```

4. **Chat.** Type a line, press Enter; it appears on the other side prefixed
   with `< `. Ctrl-D quits.

## Verification

Type `hello` on either side; the other terminal shows `< hello` within a
moment. That one line proves TLS to the relay, MLS encryption, relay fan-out,
and delivery.

To also verify resilience: restart the relay mid-chat. The clients print
`(connection lost — reconnecting…)` then `(reconnected)`, and messages typed
while offline arrive after — nothing lost, nothing duplicated.

## Notes on what you'll see

- `(group state updated, epoch N)` — an MLS key rotation was applied. Normal.
- Messages queue while disconnected and send on reconnect; the engine retries
  for ~10 seconds (40 attempts × 250 ms) before giving up.
- v0.1 scope: one conversation per CLI process, two members.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `relay fingerprint must be 64 hex characters` | Truncated/mistyped fingerprint | Copy the full 64-char hex line from relay startup output. |
| TLS handshake error on `listen` | Fingerprint doesn't match the relay | You're pinning a different relay's (or an old) fingerprint. |
| `connect` fails to parse the link | Link corrupted in transit (line wrap, added whitespace) | Re-copy as one unbroken string; quote it in the shell. |
| `reconnect: relay unreachable` | Relay down or unreachable for >10s | Restart the relay; restart the CLI and re-pair if the relay lost its state file. |

## Related

- [Getting started tutorial](guide-getting-started.md) — the same flow with the relay included
- [Command reference](reference-commands.md) — every flag and default
