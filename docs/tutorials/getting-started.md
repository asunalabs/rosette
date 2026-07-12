# Getting started: an encrypted conversation on your machine

You'll run the whole stack locally — a relay and two chat clients — and send
an end-to-end encrypted message between two terminals. By the end you'll have
seen everything the project does today: TLS with certificate pinning, MLS
encryption, store-and-forward delivery, and automatic reconnect.

## What you'll need

- Rust stable ([rustup.rs](https://rustup.rs)) — the only hard requirement.
- Three terminals.
- Optional, for the desktop app at the end: JDK 17.

## Step 1: start a relay

```bash
cargo run -p relay
```

First run compiles the workspace (a few minutes), then prints:

```
relay TLS fingerprint: 8c1e…(64 hex chars)
relay listening on 127.0.0.1:7443 (TLS fingerprint 8c1e…)
```

Copy the fingerprint — the next step pins it. The relay is now a working
store-and-forward server; two files appeared in the repo root
(`relay_identity.der`, `relay_state.sqlite3`) that make it restart-safe.

## Step 2: create Alice and get her contact link

Second terminal:

```bash
cargo run -p cli -- listen --name alice --relay-fingerprint <paste-fingerprint>
```

```
Share this link: ARsBAAAAAAAAAAEABQAB…
Waiting to be paired...
```

That base64 string is a contact link — the thing a QR code will encode in the
app. It carries the relay address, the pinned fingerprint, and one-time
pairing material. No account, no phone number, no signup: the link **is** the
identity exchange.

## Step 3: pair Bob and say hello

Third terminal (paste the link from step 2, quoted):

```bash
cargo run -p cli -- connect --name bob "ARsBAAAAAAAAAAEABQAB…"
```

Both client terminals print:

```
Paired. Epoch 1.
Type a message and press enter. Ctrl-D to quit.
```

Type `hello from bob` in Bob's terminal. Alice's terminal shows:

```
< hello from bob
```

That message was MLS-encrypted by Bob, carried over pinned TLS to the relay,
fanned out to Alice's queue, and decrypted by Alice. The relay saw only
padded ciphertext and random queue ids.

## Step 4 (optional): break things

The fun part. With the chat running:

1. Kill the relay (Ctrl-C in terminal 1). Both clients print
   `(connection lost — reconnecting…)`.
2. Type a message anyway — it queues.
3. Restart the relay (`cargo run -p relay` — same identity and state files,
   same fingerprint). Clients print `(reconnected)` and the queued message
   arrives. No loss, no duplicates — this exact scenario is a test
   (`engine/tests/relay_restart.rs`).

## Step 5 (optional): the desktop app shell

The Compose Multiplatform app currently ships a walking skeleton — it proves
the Kotlin → Rust FFI stack links and talks to a real relay, but real screens
land after DESIGN.md. To run it against your relay:

```bash
cd app
CHAT_RELAY_ADDR=127.0.0.1:7443 CHAT_RELAY_FINGERPRINT=<paste-fingerprint> \
  ./gradlew :composeApp:run
```

A window opens showing `engine up — 0 conversation(s)` — the real engine,
across the FFI seam, on your relay.

## What you built

A complete private-messaging loop: relay + two paired clients exchanging
encrypted messages, surviving relay restarts. Where to go next:

- Run the relay somewhere real: [how to run a relay](howto-run-a-relay.md)
- Every flag and limit: [command reference](reference-commands.md)
- Why it's built this way: [architecture.md](architecture.md)
- Building the app UI against the engine: [ffi-contract.md](ffi-contract.md)
