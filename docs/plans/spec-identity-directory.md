# Identity/Directory Service — canonical current-state spec

**This is the T23 consolidation.** One clean statement of what was decided
and what exists, as of 2026-07-14. The full decision history (four review
passes, amendments, decision-log ids) stays in the source doc:
`~/.gstack/projects/chat/justfossa-master-design-20260712-135530.md` (local
to the review machine) — read that for *why*, read this for *what is*.
Implementation status per task: `docs/plans/tasks-identity-directory-pivot.md`.

## The model (Approach A — Signal-model, decision `0eed90dd`)

Users register with a **phone number** (verification/anti-abuse gate, never
a public identifier) and claim a **username** (the public discovery
surface). No email identifier — dropped, not deferred. Profile pictures are
v1.1 at the earliest (OQ8 unanswered).

- **Username** = user-chosen nickname + numeric discriminator
  (`nickname#05`), globally unique via `UNIQUE (nickname, discriminator)`.
  Discriminator width is per-nickname and **ratchet-only**: when a
  nickname's 2-digit space fills, the width widens to 3 and every holder
  renders at the new width ("05" becomes "005" — never a second colliding
  value). Charset is ASCII alphanumeric plus `_ - .` — confusable Unicode
  is rejected by the allowlist itself, no confusables table.
- **Phone** is stored ONLY as `phone_hash` = Argon2id (keyed with a
  server-side pepper held outside the DB) over the E.164-normalized number,
  plus a short `phone_hash_prefix` for bucketed search. Raw numbers are
  never persisted.
- **Phone verification gates access to the app itself, Signal-style
  (decided 2026-07-14, not yet enforced — T27):** you cannot get past
  onboarding without registering and verifying a phone number — anti-spam.
  Verification stops being just the ticket into directory search; there is
  no unverified-but-usable app state. Onboarding order: verify phone →
  claim username → app unlocks. Transport-level enforcement at the
  account-less relay is T27's open eng question, and T2's soft-gate
  degraded mode needs a re-decision under this rule.
- **Phone-search visibility is opt-in and off by default** for every
  account regardless of age (`searchable BOOLEAN DEFAULT FALSE`, toggled
  via `POST /searchable`). This, not an age gate, is what limits
  discoverability exposure — matches the completed external legal review
  (`[private legal records]`).
- **Age posture:** 13+, self-declared, no verification (Signal's shipped
  policy). One open item in the private legal record before treating this as fully
  closed.
- **Deletion:** erasure scrubs `phone_hash`/`phone_hash_prefix`/
  `searchable`; the `(nickname, discriminator)` slot is **permanently
  reserved** (retained, inert, never displayed or searchable) as
  anti-impersonation. A deleted phone number re-registers only after a
  24-48h cooldown (`phone_cooldown` table).

## Architecture

`directory/` is its own Rust crate in the workspace AND its own
binary/process (decisions D1A + E1A): own port, own TLS, own database. Its
only workspace dependency is `proto` — it never imports `core/` or
`engine/`, so a directory crash or kill-switch cannot touch message
delivery (proven by `tests/crash_isolation.rs`: real subprocess, real
SIGKILL, live relay chat continues).

- **Storage: PostgreSQL** (supersedes the earlier SQLite decision — the
  directory is one centrally-run service, not a per-operator file like the
  relay's). Migrations in `directory/migrations/`.
- **API: HTTP + JSON via axum** — its own response types, NOT `proto`'s
  relay wire format. `SearchResultEntry { user_id, handle }` has no field
  that could carry a phone hash: hidden-field filtering is structural, not
  a rendering rule.
- **Endpoints:** `/health`, `POST /signup`, `POST /verify`,
  `POST /username`, `POST /searchable`, `DELETE /account`, `GET /search`,
  `POST /pairing-bootstrap`, `POST /pairing-bootstrap/request`.
- **Deploy:** Docker (multi-stage Alpine/musl) + docker-compose
  (directory + Postgres + Caddy for automatic TLS) + systemd unit, target
  AWS EC2 — `directory/deploy/README.md` has the launch steps. The instance
  itself has not been launched; DNS not pointed.

## Security posture (all implemented and tested)

- **Anti-enumeration (the T17 spike's answer, gating search):**
  k-anonymity hash-prefix bucketing, HIBP-style — the client sends a
  5-hex-char (20-bit) prefix, gets the bucket, matches locally. The server
  structurally never receives a full target hash (`search_by_prefix` only
  accepts a prefix), so there is nothing to branch on per-target.
  Not-found and hidden are indistinguishable: lookup time tracks bucket
  cardinality only (timing-variance regression test). Bloom filters were
  rejected (turns an online rate-limitable oracle into an offline
  unrateable one); real PSI/SGX rejected as disproportionate for v1.
  **Open tuning question:** prefix length / bucket size k at real launch
  scale — the 5-hex default is a disclosed placeholder (`ponytail:` marker
  in `search.rs`).
- **Auth + rate limits:** search requires a bearer session token (checked
  before feature flags and rate limits). Verified accounts: 30
  searches/min; unverified: 5/min. Bulk sequential scraping from one
  account hits the wall (tested).
- **No query-content logging:** the rate limiter stores only
  `(caller_id → count, window_start)` — no field anywhere can hold a
  search prefix. "Who searched for whom" never exists to be compelled.
- **Kill switches:** `DIRECTORY_ACCOUNTS_ENABLED` and
  `DIRECTORY_SEARCH_ENABLED` are independent env flags; search can die in
  seconds without touching signup or collected data.
- **`Cache-Control: no-store`** on every response via router-level
  middleware — no future endpoint can forget it.
- **OTP: Twilio Verify v2**, behind the `OtpVendor` trait (a second vendor
  is one new impl block). Loud refusal without credentials;
  `DIRECTORY_ALLOW_DEV_OTP_VENDOR=1` opts into the dev vendor. Vendor
  outage degrades to a **soft gate**: the account is created unverified
  (excluded from search, tighter rate limit) and is never silently
  upgraded. Never yet tested against a live Twilio account — no
  credentials exist.

## Search → pairing handoff (T25, backend half)

A search hit becomes an MLS pairing WITHOUT a QR exchange via one-time
pairing bootstraps:

- A user publishes (`POST /pairing-bootstrap`) the same opaque base64
  ContactLink string `ChatEngine::contact_link()` mints for QR codes — a
  fresh one-time KeyPackage + their bootstrap-mailbox endpoint. Directory
  never parses it.
- A searcher consumes it (`POST /pairing-bootstrap/request`) — the fetch is
  `DELETE ... RETURNING`, so two concurrent requesters can never receive
  the same one-time KeyPackage (DB-enforced). Only served for targets still
  `searchable`; erasure deletes any pending bootstrap.
- Delivery of the pairing request then rides the EXISTING relay mailbox
  queue kind — zero relay changes were needed.
- **Not built (client work):** generating/uploading/replenishing bootstraps
  and feeding a fetched one into `ChatEngine::pair_with_link`. Until that
  app/-side wiring lands, T14's search-to-first-message metric is
  unmeasurable.

## What is NOT in this spec's scope

- **Proximity discovery** (BLE/Wi-Fi Direct/mDNS) — its own spike (T11) and
  its own eng-review; intended to reuse the same KeyPackage pairing flow as
  QR and directory search (one pairing mechanism, three discovery
  front-ends).
- **Legal**: OQ9 cleared 2026-07-12 after external legal review — data
  minimization (nothing to hand over) is the compelled-disclosure answer;
  records kept privately.
- **Deferred UX expansions** (mutual-contact signal, unified add flow,
  invite-link growth loop): TODOS.md #9.

## Superseded along the way (so nobody resurrects them)

- Approach B (phone + email + username, per-identifier visibility) — killed
  by the Telegram-precedent landscape check.
- The three-value visibility enum (`contacts-only`) — v1 is a boolean
  `searchable`; a third state returns only when "what is a contact" is
  designed.
- SQLite for the directory — Postgres (see Architecture).
- Rate-limited raw hash-equality lookup as an "interim" search — never
  shipped; prefix bucketing was the launch bar (Outside Voice Tension 1).
- Username release-on-delete cooldown — replaced by permanent reservation.
