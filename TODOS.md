# TODOS

Strategy/legal items surfaced by the eng-review outside voice (2026-07-11),
captured per review decision D21.0-A. All six belong to a /plan-ceo-review
conversation, not engineering.

## 1. Legal posture: operator obligations under EU law
- **What:** Complete the external legal review of relay-operator obligations
  and the entity/jurisdiction setup. Substance is tracked privately, outside
  this repository.
- **Depends on:** Jurisdiction shortlist (design doc Open Question 4).

## 2. Play Store as a launch channel, not a bonus
- **What:** Decide whether Google Play is a first-class launch channel with
  delisting as the planned contingency, or stays a "bonus channel."
- **Why:** The users the law scans (Messenger/Instagram/email users) do not
  sideload APKs or add F-Droid repos; the sideloading crowd already has
  SimpleX/Signal. "Mainstream UX through non-mainstream channels" may be a
  contradiction in the wedge.
- **Pros:** Resolves the audience/distribution mismatch honestly, either way.
- **Cons:** Play presence adds Google developer-account identity exposure and
  policy risk — the things the distribution plan was built to avoid.
- **Context:** Outside voice finding #7. Distribution plan currently treats Play
  as revocable extra; mandate-resistance (APK/F-Droid) remains non-negotiable
  either way.
- **Depends on:** Nothing; decidable now at CEO level.

## 3. Session-refugee window: ship something joinable inside it
- **What:** Decide whether to stand up a joinable community waypoint (hosted
  group/waitlist/Matrix room + public build tracker) within weeks, so the
  ex-Session audience has somewhere to land before private beta.
- **Why:** Displaced users re-home in weeks; the beta is 2-4 months out. The
  "ready-made first audience" claim otherwise expires before the product opens.
- **Pros:** Keeps the timing asset alive; the step-1b community question gets a
  standing audience instead of a one-shot post.
- **Cons:** Community maintenance is ongoing builder time; a waypoint on someone
  else's platform is itself scannable/orderable (optics).
- **Context:** Outside voice finding #8. Step 1b (ask r/SessionMessenger what
  they miss) already exists; this extends it into retention.
- **Depends on:** Name/brand (Open Question 1) at least provisionally.

## 4. Growth mechanic for a discovery-free network
- **What:** Design the explicit growth loop: invite-tree onboarding,
  group-link-first growth, or accept and state a niche ceiling.
- **Why:** No-identifier means no contact discovery; UX polish doesn't fix
  "I can't find my friends." SimpleX's friction is structural, and this plan
  currently assumes polish overcomes it.
- **Pros:** A designed loop (e.g., every group link is an install vector) turns
  the biggest structural weakness into the distribution mechanism.
- **Cons:** Invite-gated growth caps the curve; group-link growth interacts with
  spam/abuse (Open Question 2).
- **Context:** Outside voice finding #9. Interacts with OQ2 (abuse) — invited-only
  growth is also an abuse answer.
- **Depends on:** Nothing hard; best decided before v0.5 onboarding UX is built.

## 5. iOS: state the structural limit now
- **What:** Write the honest iOS position: APNs requires an identity-linked
  revocable Apple account, DMA marketplaces still require Apple notarization,
  and a client-side-scanning order against signed binaries has no iOS
  workaround. Decide whether iOS is "later," "degraded promise, disclosed," or
  "not until the landscape changes."
- **Why:** "Decide iOS timing at beta" dodges whether the core promise can exist
  on half the EU market; better to state the limit before it reads as a broken
  promise.
- **Pros:** Credibility with exactly the audience this app courts; scopes the
  UniFFI iOS work honestly.
- **Cons:** Publicly conceding no full iOS story narrows the addressable market
  on paper.
- **Context:** Outside voice finding #19; replaces the current Open Question 5
  framing (timing) with a feasibility statement.
- **Depends on:** None.

## 8. DESIGN.md + visual signature (identicon system) — DONE 2026-07-14
- **Done:** /design-consultation shipped `DESIGN.md` ("The Instrument":
  IBM Plex superfamily, white/near-black + Aubergine swap-point accent,
  guilloché Rosette identicon with engraved-band verified treatment,
  instrument motion rules) + rendered reference `docs/design/design-preview.html`
  (live tokens + live Rosette algorithm v0). Step 6 UI work is UNBLOCKED.
- **What (original):** Run /design-consultation to produce DESIGN.md — typeface, color
  tokens, spacing scale, motion — and deliberately design the identicon
  algorithm as the app's one visual signature (plus the verified-✓ treatment).
- **Why:** composeApp UI work (architecture.md step 6) is gated on this;
  building screens before tokens exist means per-screen ad-hoc styling that
  never reconciles. The identicon is on every screen and no competitor owns
  it — it's the natural mark that keeps the app from reading as a WhatsApp
  clone (the wedge is mandate-resistance, so familiar-by-intent is fine, but
  anonymous is not).
- **Pros:** Unblocks the UI milestone with a coherent system; gives the app a
  recognizable identity.
- **Cons:** Design-session time before any UI code; not on the backend
  critical path (steps 0-4 don't need it).
- **Context:** Surfaced by the 2026-07-11 design review (plan 3/10 → 8/10).
  Wireframes docs/design/wireframe-v0.html + v1.html are layout-only input; this
  produces the visual layer they defer.
- **Depends on:** Nothing (can start anytime); blocks architecture.md step 6.

## 7. Push notification strategy (per-platform channel decision)
- **What:** Decide the push channel per platform: UnifiedPush/self-hosted vs
  FCM on Android; APNs on iOS (which requires the identity-linked Apple
  account TODO 5 already flags). Includes what a "no push" degraded mode
  looks like (foreground-service polling on Android, nothing on iOS).
- **Why:** A messenger without push is a dead messenger on mobile, and
  FCM/APNs route delivery metadata (timing, volume, token↔device identity)
  through Google/Apple — directly in tension with the no-identifier promise.
- **Pros:** Deciding before the app milestone means the engine's event path
  (docs/explanation/architecture.md, FFI strategy) can account for wake-from-push from
  day one instead of retrofitting.
- **Cons:** UnifiedPush narrows the mainstream audience (needs a distributor
  app); FCM concedes metadata to Google. There is no clean answer, only a
  priced trade.
- **Context:** Surfaced by the 2026-07-11 eng review of docs/explanation/architecture.md
  (listed there as NOT-in-scope). The relay's SUBSCRIBE push model
  (amendment A12) covers only live connections; mobile OS background limits
  make a separate wake channel unavoidable.
- **Depends on:** TODO 2 (Play Store posture) and TODO 5 (iOS position).

## 6. Audit funding + revenue model — decided 2026-07-14
- **Decision:** Grants first — NGI Zero/NLnet-style EU privacy-tech
  programs, non-dilutive — to fund the initial build and the external
  security audit that gates public beta. Ongoing relay costs via a
  Signal-style recurring donor program (Open Collective fiscal host, no
  entity required to start), plus sponsor-a-relay line items and in-kind
  infra credits. **The app is free forever for individuals — no paid tier,
  ever.** Billing requires an identity link, which contradicts the
  no-identifier promise the same way ads and data sale already did.
  Open-core paid hosting/support for orgs running their own relay
  infrastructure stays a separately-tracked future option — it never
  touches an individual user's billing or identity.
- **Why:** The public beta is gated on an audit; grants are the one funding
  path with no tension against the privacy positioning.
- **Depends on:** Entity/jurisdiction work (TODO 1) determines where a
  grant payout lands; it doesn't block applying.

## 10. E2EE audio/video calling — future roadmap (added 2026-07-14, design posture decided 2026-07-14)
- **What:** Add one-to-one (later group) voice and video calls, end-to-end
  encrypted, as a post-v1 feature. Media relays see IP pairs and call
  timing even when content is E2EE — same metadata class as the push
  question, TODO 7.
- **Decision (2026-07-14, founder):** Signal's model, wholesale.
  - **Media stack:** libwebrtc driven from the Rust core, RingRTC-style
    wrapper — not a custom media path, not webrtc-rs. 1:1 E2EE via
    DTLS-SRTP with key material bound to the existing MLS/pairing
    identity (verified contact = verified call, no new trust ceremony).
  - **Signaling:** over the existing relay SUBSCRIBE channel; no new
    server component for 1:1.
  - **Routing policy:** contacts P2P by default, non-contacts/strangers
    always relayed through TURN, plus a settings toggle "always relay
    calls" for users who don't want any contact seeing their IP.
    (Signal's exact defaults.)
  - **Call wake/ringing:** same as Signal — FCM high-priority push where
    Play Services exist, persistent-websocket fallback on de-Googled
    devices, APNs VoIP push + CallKit on iOS. NOTE: this leans TODO 7
    toward the FCM-with-fallback answer and inherits TODO 5's
    identity-linked Apple account problem for iOS calling; recorded here
    as the calling-side posture, TODO 7 still owns the final push
    decision.
  - **Group calls:** explicitly out of scope for this TODO. They require
    an SFU that sees full call membership in real time — a separate,
    bigger metadata concession to be priced as its own future item.
  - **Cost note:** TURN bandwidth is symmetric/constant-bitrate; the
    funding plan's relay-cost lines get materially bigger once relayed
    calls exist.
- **Why:** Founder-stated roadmap intent (2026-07-14). Calls are a
  table-stakes expectation for the mainstream audience TODO 2 targets;
  deciding the media-relay metadata posture early keeps it from being
  retrofitted against the privacy promise later.
- **Pros:** Recording it now means the engine's event/transport layer can
  avoid decisions that would foreclose a calling path (e.g., signaling needs
  low-latency bidirectional messaging the SUBSCRIBE model already gives).
- **Cons:** Real scope — calling is its own project (media stack, NAT
  traversal, battery/permissions UX, per-platform hardware paths). Not
  budgeted in any current milestone; nothing in v0.5/v1 depends on it.
- **Context:** Companion requirement recorded same day: client-side
  SQLCipher/SQLite persistence (Signal's model — nothing durable on relays)
  was confirmed as a hard requirement; that one is already engineering
  scope as T5/T8 (docs/explanation/architecture.md), so only calling needed
  a new TODO.
- **Depends on:** v1 shipping first; TODO 7 (push) shares the
  metadata-vs-mainstream trade and should be decided before or with this.

## 9. Identity/directory service: deferred UX expansions
- **What:** Three expansion candidates surfaced during the 2026-07-12
  /plan-ceo-review of the phone/username directory pivot, held out of that
  plan's scope: (a) mutual-contact trust signal in search results ("3 mutual
  contacts" without revealing who), (b) a unified add-contact flow covering
  QR, username search, and proximity discovery in one UI instead of three,
  (c) invite-link-first growth loop integration with the directory service.
- **Why:** None block the Approach A directory service or proximity
  discovery (TODO see design doc OQ12); all are UX polish or growth-loop
  work that can land after the core identity pivot ships.
- **Pros:** Each is small (S effort) and can be picked up independently
  whenever there's a slow week.
- **Cons:** None are load-bearing — easy to let this rot into "later means
  never" if nobody revisits it.
- **Context:** justfossa-master-design-20260712-135530.md, "Selective
  Expansion — Scope Additions" section.
- **Depends on:** Nothing; the directory service (Approach A) should ship
  first since (a) and (b) both reference it.

## 11. Provision real Twilio Verify credentials (added 2026-07-16, /plan-eng-review on T27)
- **What:** Set up a real Twilio Verify account and configure
  `TWILIO_ACCOUNT_SID`/`TWILIO_AUTH_TOKEN`/`TWILIO_VERIFY_SERVICE_SID` for the
  directory service, so `verify::vendor_from_env()` picks `TwilioOtpVendor`
  instead of requiring the explicit `DIRECTORY_ALLOW_DEV_OTP_VENDOR=1` dev
  opt-in.
- **Why:** T27 (phone verification gates the app itself) and its transport
  half (relay-side attestation tokens, directory-signed) only mean something
  if phone verification is real. `TwilioOtpVendor`'s request/response logic
  is unit-tested but has never made a real HTTP round trip (T26's own task
  note). Any code built on top of "this device verified a phone" — the
  relay attestation scheme especially — has a security ceiling of zero
  until this lands. Easy to miss because the crypto code looks complete
  either way.
- **Pros:** Small, well-isolated — `OtpVendor` was designed as a one-impl-
  block swap (T26); this is a business/ops action (create account, billing),
  not new code.
- **Cons:** Real money (Twilio billing) before revenue exists, same
  category as TODO 1's pre-revenue-cost tradeoff.
- **Context:** Surfaced by the outside-voice pass during /plan-eng-review of
  T27's relay-attestation design (`docs/plans/tasks-identity-directory-pivot.md`
  T27). `directory/src/verify.rs`'s `vendor_from_env()` already refuses to
  start without either real Twilio creds or the explicit dev opt-in — no
  silent insecure default — so this isn't a live security hole, but it
  bounds what T27's transport half is actually worth in production.
- **Depends on:** None. Blocks: T27's transport half being meaningful in
  any real deployment.

## 12. relay create_group_inbox: validate roster membership / cap size (added 2026-07-16, /plan-eng-review on T27)
- **What:** `relay/src/state.rs`'s `create_group_inbox` (lines ~299-329)
  never validates that `fan_out_to` member `QueueId`s reference real,
  existing queues, and has no cap on roster length.
- **Why:** Anyone who solves the queue-creation PoW challenge can call it
  today with an arbitrarily large roster of made-up `QueueId`s — the relay
  persists an inbox row plus one roster row per (bogus) member, a storage-
  abuse vector that costs the attacker only PoW, independent of any phone
  verification.
- **Pros:** Small, isolated fix — one function in one file. Doesn't
  interact with T27's attestation-token work; both can gate the same
  call independently (token proves "caller verified a phone", roster
  validation proves "the roster is real").
- **Cons:** Not urgent pre-launch — no adversary yet, PoW already imposes
  a per-call cost floor.
- **Context:** Surfaced by the outside-voice pass during /plan-eng-review
  of T27 while checking whether "gating `create_mailbox` transitively
  gates `create_group_inbox`" actually holds — it does for delivery
  (`send_to_group_inbox` filters `fan_out_to` against real Mailbox queues
  before fanning out) but not for creation itself, which is the pre-
  existing gap here.
- **Depends on:** None.

## 13. DESIGN.md doesn't actually ship (added 2026-07-16, /plan-design-review on T27)

- **What:** Bundle the IBM Plex OFL TTFs into `composeApp` resources, bundle
  the Lucide vector drawables, and implement DESIGN.md's Motion section.
  Today all three are declared in the design system and absent from the build.
- **Why:** DESIGN.md's identity is currently unexercised in the running app.
  Three concrete consequences: (1) `ChatTheme.kt:92-93` falls back to
  `FontFamily.Default`/`FontFamily.Monospace`, so the app renders in Roboto on
  Android and whatever the JVM picks on desktop — every type decision in
  DESIGN.md "Typography" is theoretical. Worse, the **Mono quarantine is the
  system's central voice mechanism** ("mono means verifiable cryptographic
  fact" — DESIGN.md:56) and it currently resolves to *the platform's default
  monospace*, which means nothing to anyone. (2) No icon assets exist at all;
  every icon is a text glyph in the system font (`←` `ConversationScreen.kt:54`,
  `↑` :98, `⌄` `Primitives.kt:174`) — the exact pattern the founder rejected
  on 2026-07-14 (learning `design-taste-justfossa-chat-quiet-room`, 10/10:
  "Lucide icons only (rejected unicode glyph icons as weird)"). They also
  won't mirror under RTL, which `AndroidManifest.xml` declares support for.
  (3) Zero `animate*`/`tween`/`Easing`/`AnimatedVisibility` in the module —
  DESIGN.md:180-188 is entirely unimplemented, including "The one ceremony".
- **Pros:** Cheap and mechanical — no design decisions left to make, DESIGN.md
  already specifies every value. Unblocks the Rosette verification ceremony
  (TODO/T27 follow-up) having something to animate. Makes every future screen
  land against a design system that's actually rendering.
- **Cons:** Adds APK size (Plex Latin+Cyrillic+Greek subset + ~12 Lucide
  vectors). No user-visible feature ships as a result — it makes existing
  screens correct rather than adding capability.
- **Context:** Found by /plan-design-review on T27 (2026-07-16), corroborated
  by an independent design subagent that checked DESIGN.md commitment-by-
  commitment. The gap is disclosed honestly in a `ponytail:` comment at
  `ChatTheme.kt:87-91`, so it's known debt, not an oversight — this TODO
  exists to stop it compounding as more screens get built against a system
  that isn't rendering. Related drift found in the same pass and tracked in
  `docs/plans/tasks-identity-directory-pivot.md`'s task list: no FAB, no
  timestamps, no bubble grouping, `info` token with zero usages.
- **Depends on:** None. Blocks: the verification ceremony (DESIGN.md:185-187)
  having a motion system to fire in.

## 14. Accessibility baseline + European Accessibility Act applicability (added 2026-07-16, /plan-design-review on T27)

- **What:** Two things. (a) Build the a11y baseline the app has none of:
  `semantics`/`contentDescription` throughout, `Role.Switch`/`Role.Tab` on the
  custom primitives, 48dp minimum touch targets, `heightIn(min=)` instead of
  fixed `height()` so text scaling doesn't clip, SMS OTP autofill. (b) Get a
  written legal answer on whether the **European Accessibility Act**
  (Directive (EU) 2019/882, applicable since 2025-06-28) binds this product.
- **Why:** (a) `grep -rn "semantics\|contentDescription\|Role\." app/composeApp/src`
  returns **nothing**. Concretely: `InstrumentToggle` (`Primitives.kt:196`) is
  an unlabelled `Box` with no role and no checked state, so **a screen-reader
  user cannot find out whether they are discoverable** — in a product whose
  entire promise is control over exposure. Touch targets are under the 48dp
  minimum throughout; the smallest is "Resend code" at ~28dp
  (`Onboarding.kt:225`), which is the control a user reaches for when the flow
  is *already failing*. Every fixed `height()` (52dp buttons/fields, 50dp OTP
  cells, 72dp list rows, 56dp bars) clips at 200% font scale.
  (b) The EAA applies to consumer-facing "electronic communications services"
  in the EU, which a messenger is under the EECC. DESIGN.md:16 states the
  target user is "mainstream EU users". There is a **microenterprise exemption**
  (<10 staff AND <€2M turnover) for services that may well cover this project —
  which is exactly why it's worth a cheap written answer rather than an
  assumption. This is a legal question, NOT a finding of non-compliance.
- **Pros:** a11y retrofits cost multiples of building it in, and the primitives
  are all custom (`Primitives.kt`) so there's exactly one place to fix each
  one — this is unusually cheap right now and gets more expensive per screen
  added. The legal question is one email on the existing review channel
  (TODO #1).
- **Cons:** Neither half ships a user-visible feature. The legal answer may be
  "exempt", making (b) sunk cost — though a written "exempt" is itself worth
  having before launch.
- **Context:** Found by /plan-design-review on T27 (2026-07-16). Color contrast
  is the one piece of a11y that is measured rather than assumed: every
  text/background pair in `ChatTheme.kt:43-83` was checked and DESIGN.md's
  accent-swap thresholds (:88-90) are met. This TODO is about everything else.
  - **Amended 2026-07-16 (ET12).** The "floor is 4.82:1 / don't touch the
    palette, it's correct" claim was true when written and stopped being true
    the moment T27 shipped `InstrumentStatusChip`: it introduced the
    `warning`/`warningSoft` pair, which had **zero usages** at measuring time
    and so was never in the measured set. It landed at **4.47:1** — under
    DESIGN.md's own 4.5:1 bar — and dark `error`/`errorSoft` at **3.91:1**.
    Both fixed (light `warning` → `#7A5500`, dark `error` → `#F16A6F`); the
    per-pair numbers now live in DESIGN.md's color table, next to the tokens,
    so the next new pair is measured where it is defined rather than trusted
    against a summary here. **A palette-wide floor is not a safe claim** — it
    only covers the pairs that existed when someone last looked.
- **Depends on:** None for (a). (b) depends on the existing legal-review channel.

## 15. Directory sessions need a real store — expiry, revocation, bounded growth (added 2026-07-16, /plan-eng-review on the T27 gate diff)

- **What:** Replace `DirectoryStore`'s `sessions: Mutex<HashMap<String, u64>>`
  with a session store that has (a) an expiry per token, (b) revocation on
  `erase_user`, and (c) bounded growth. Persistence remains explicitly optional
  — see below.
- **Why:** `docs/plans/tasks-identity-directory-pivot.md` (T27) currently
  accepts this as a documented tradeoff: *"the `sessions` map is in-memory only
  (no persistence, no expiry) — a directory restart wipes sessions... Rare
  event, acceptable tradeoff."* **That acceptance was made on incomplete
  information.** The framing treats the only consequence as availability (a
  restart forces re-verification). This review found two more the acceptance
  never weighed:
  1. **The map is insert-only.** `store.rs:34` declares it; `store.rs:352` is
     the sole mutation (`insert`); `store.rs:357` is the sole read. `grep
     sessions directory/src/store.rs` returns exactly those four lines — there
     is **no `remove` anywhere in the file**. Every `POST /verify` mints a
     permanent entry (`api.rs:216`, `create_session` runs unconditionally,
     including on a `Degraded` outcome whose token the client discards).
  2. **`erase_user` does not revoke.** `store.rs:260`'s `erase_user` opens a
     transaction and deletes DB rows, never touching the map. `authenticate`
     (`api.rs:107`) resolves a caller *only* through `store.session_user_id(token)`.
     So after `DELETE /account`, the erased user's bearer token still
     authenticates against a dangling `user_id`. T24/OQ5 sell erasure as a
     privacy feature; a token outliving the account makes that erasure
     incomplete in a way a user cannot observe.
- **Pros:** Removes an auth bug (2) that contradicts a shipped privacy promise.
  Bounds a leak (1) that an unauthenticated endpoint drives. Expiry is the same
  pattern T27's attestation tokens already chose for their spent-set (prune by
  the item's own expiry, never by insertion count — see the
  `spent-token-fifo-eviction-fail-open` learning); using it here keeps one
  eviction discipline across the service instead of two.
- **Cons:** Real scope against a service whose stated virtue is staying simple.
  Persistence in particular would add a storage dependency to the exact
  component T21's crash-isolation test keeps deliberately dumb — **do not
  assume this TODO means "persist sessions"**; expiry + revocation are the
  load-bearing parts and neither needs a disk.
- **Context:** Surfaced by `/plan-eng-review` (2026-07-16) reviewing the T27
  gate diff. Two P1 tasks in that plan patch the acute symptoms — an explicit
  `reqwest` timeout + `/verify` rate limit (removes the growth driver), and
  `erase_user` revoking sessions (closes the auth bug). **With both landed, the
  original "restart wipes sessions, acceptable" tradeoff genuinely does stand
  on complete information** — this TODO exists so that conclusion is recorded
  as reconsidered rather than assumed, and so nobody re-accepts it on the
  original incomplete reasoning. Verifying anything here needs live Postgres:
  `docker run -d --rm --name chat-test-pg -e POSTGRES_PASSWORD=test -p
  127.0.0.1:15432:5432 postgres:16-alpine`, then
  `DATABASE_URL=postgres://postgres:test@127.0.0.1:15432/postgres cargo test -p directory`.
- **Depends on:** Nothing hard. Overlaps `directory/src/{store.rs,api.rs}` with
  the two T27 P1 tasks above — land those first, then re-read this; the
  remainder may be small enough to close out.
