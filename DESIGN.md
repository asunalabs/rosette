# Design System — "The Instrument" (chat, working name — OQ1)

Source of truth for every visual/UI decision. Rendered reference (live
tokens, live Rosette algorithm, three mocked screens, light+dark toggle):
`docs/design/design-preview.html` — open it in a browser; it IS this file
executed.

## Product Context
- **What this is:** a private messenger (MLS E2EE, Rust core, Kotlin
  Multiplatform Compose UI — Android + desktop, iOS later).
- **Who it's for:** mainstream EU users unsettled by Chat Control-era
  scanning — not threat-model hobbyists.
- **Space:** vs. Signal (periwinkle nonprofit-soft), Telegram (toy),
  Session (black+neon paranoia), SimpleX (techy gradients), Threema
  (corporate compliance).
- **Project type:** mobile-first chat app + desktop.
- **The memorable thing (founder, 2026-07-14, every decision serves it):**
  *"If we were ever forced to choose between giving government access to
  user data or shutting down, we would shut down."*

## Aesthetic Direction
- **Direction:** The Instrument — precision-engineered software in the
  visual language of things built to be trusted: banknotes, passports,
  instruments. Conviction expressed as engineering, not warmth or menace.
- **Decoration level:** minimal — typography, hairlines, and the Rosette
  do all the work. No gradients, no blobs, no glassmorphism, no shadows
  beyond one soft elevation for modals/FAB.
- **Mood:** calm, exact, quietly powerful. Familiar messenger anatomy
  (chat list, bubbles) so it never reads experimental; instrument detailing
  so it never reads WhatsApp-clone.
- **Hard NOs (user-vetoed or field-owned):** cream/warm-paper surfaces,
  serif faces, orange/clay/amber accents, green accents, Signal/Telegram
  blue, black+neon paranoia styling, purple *gradients* (the flat aubergine
  accent is deliberate; gradients of it are not).

## Typography
One engineered superfamily. Voice = weight and tracking, never a second family.
- **Display/statement:** IBM Plex Sans 700, tracking -0.02em — pledge,
  onboarding headlines, empty states. sizes: 28–34sp mobile.
- **Body:** IBM Plex Sans 400 — messages, settings copy. 16sp / 1.5.
- **UI/labels:** IBM Plex Sans 500–600 — names 600, buttons 600 (15sp),
  secondary/meta 500 (13–14sp), caption 12sp, micro 11sp.
- **Data:** IBM Plex Sans with `tnum` (tabular figures) — timestamps,
  unread counts, and username discriminators (`mira#07` aligns in lists;
  usernames stay in Sans because they belong to people, not terminals).
- **Code/crypto facts:** IBM Plex Mono 400 — safety-number words, key
  fingerprints, OTP entry. QUARANTINED to those moments: mono means "this
  is a verifiable cryptographic fact" and nothing else.
- **Coverage:** Plex covers Latin extended + Cyrillic + Greek — EU names
  render with no fallback seam. License: OFL, bundleable in the APK.
- **Loading:** bundle OFL TTFs in the app (`composeApp` resources); web
  surfaces may use Google Fonts (`IBM+Plex+Sans`, `IBM+Plex+Mono`).
- **Scale:** 11 / 12 / 13.5 / 15 / 16 / 17 / 22 / 25 / 28–34 (sp).

## Color
- **Approach:** restrained — neutrals do the living; ONE working accent;
  semantic colors only for their one job. Light = pure white + near-black
  ink; dark = near-black (subtle plum undertone) + off-white.

### THE ACCENT IS A SWAP-POINT (explicit requirement, 2026-07-14)
The accent must be trivially replaceable. Rules:
1. **Semantic naming only.** The token is `accent` (+ `accentStrong`,
   `accentSoft`, `bubbleMine`) — never "aubergine" in code, resources, or
   component styles. Nothing references the hex outside the theme object.
2. **One definition site.** Compose: a single `ChatPalette` (light+dark)
   in the theme package. Web/preview: the `:root`/`[data-theme]` CSS
   variables in `docs/design/design-preview.html`.
3. **What follows automatically on swap:** primary buttons, unread badges,
   verified ✓ + engraved band, links/ghost buttons, focus borders,
   own-bubble tint (`bubbleMine` = accent at ~12% over surface),
   `accentSoft` fills.
4. **What must be re-checked on swap (30-min checklist):** contrast of
   accent-on-white ≥ 4.5:1 and dark-accent-on-`#121013` ≥ 4.5:1; error red
   still clearly distinct from the new accent; re-curate the Rosette
   12-ink palette so ink #0/#1 harmonize with the new accent (inks live in
   ONE array next to the Rosette code).

### Tokens (current accent: Aubergine)
| Token | Light | Dark | Job |
|---|---|---|---|
| `bg` (paper) | `#FFFFFF` | `#121013` | app background |
| `surface` | `#F7F6F6` | `#1B181B` | cards, bars, incoming bubbles |
| `surface2` | `#EFEDEE` | `#242023` | pressed/nested surfaces |
| `ink` | `#1A1618` | `#ECE9EB` | primary text |
| `muted` | `#625B5E` | `#948D91` | secondary text, timestamps |
| `hairline` | `#E5E1E3` | `#2F2A2E` | dividers, borders |
| `accent` | `#5B3A6C` | `#B08CC9` | THE working color: buttons, badges, verified, pledge highlight |
| `accentStrong` | `#432852` | `#C3A4D8` | pressed accent, verified-alert text |
| `accentSoft` | `#EDE4F2` | `#2B2233` | verified/positive fills |
| `bubbleMine` | `#ECE3F1` | `#2C2235` | own message bubbles |
| `error` | `#C62828` | `#E5484D` | real failures only — brighter+warmer than accent, never blurs with it |
| `errorSoft` | `#FBE9E7` | `#35201B` | error fills |
| `warning` | `#8F6400` | `#D4A945` | expiring links etc. |
| `warningSoft` | `#F5ECD4` | `#322B18` | warning fills |
| `info` | `#4C5A6B` | `#94A3B8` | connection banner text |
- **Success/verified = accent**, not green: trust wears the brand color.
  (Green, orange, blue are all vetoed/owned — see Hard NOs.)
- **Dark mode strategy:** redesigned surfaces (plum-tinted near-blacks),
  accent lightened + slightly desaturated for contrast; never gray-flipped.
- **On-accent text:** white in light theme, `#121013` in dark theme.

## The Rosette — the one visual signature
Deterministic per-contact identicon, generated from the MLS key
fingerprint. Heritage: guilloché — the fine-line engraving on banknotes and
passports that exists because it is mathematically hard to forge. Reference
implementation (JS/SVG v0): `docs/design/design-preview.html`.
- **Algorithm v0:** closed curve `r(t) = base + amp1·cos(k·t + φ) +
  amp2·cos(2k·t)` drawn as 3–5 concentric bands; parameters read from
  successive fingerprint bytes: petal count k ∈ 5–9, base radius, two
  amplitudes, phase, per-band shrink/phase-drift, core-dot size.
- **Inks:** 2 per rosette, selected from a CURATED 12-ink engraving
  palette ([light, dark] pairs — plum/wine/slate/graphite/sepia family,
  first inks harmonized with the accent). Never random RGB.
- **Scale behavior:** ≥56dp → 5 bands @ ~1.1 stroke; <56dp → 3 bands @
  ~1.7 stroke. Must stay legible at 24dp.
- **Verified treatment:** a second concentric fine band (frequency 3k)
  engraves itself around the rosette + the quiet accent-colored ✓ beside
  the name. NEVER a shield, warning, or color inversion; unverified marks
  render at full confidence (unverified must not read as unsafe).
- **Anti-patterns:** no grids/blockies (GitHub/crypto-wallet), no
  humanoid/animal forms, no sharp spikes, no letterforms.

## Spacing
- **Base unit:** 8dp (4dp half-step).
- **Density:** comfortable. Chat-list rows 72dp; bars 56dp.
- **Scale:** 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64.
- **Bubbles:** 12dp horizontal / 8dp vertical padding; 4dp between
  same-sender bubbles, 12dp on sender change.

## Layout
- **Approach:** grid-disciplined; standard messenger anatomy on purpose
  (familiarity is the wedge — see wireframes `docs/design/wireframe-v*.html`).
- **Breakpoint:** 700dp — list+detail two-pane on desktop (per DT3).
- **Border radius:** sm 6 / md 9 (buttons, inputs, alerts) / lg 12
  (cards, sheets) / bubbles 12dp with the corner nearest the sender's
  rosette cut to 3dp ("instrument cut") / NEVER full-pill.
- **Elevation:** flat + hairlines; one soft shadow tier for modals/FAB only.

## Motion
- **Approach:** instrument precision — minimal-functional.
- **Easing:** deceleration only (Compose `FastOutSlowInEasing` /
  `LinearOutSlowInEasing`); no springs, no overshoot, nothing bounces.
- **Duration:** micro 100ms · short 150–200ms · medium 250ms ·
  ceremony 400ms.
- **The one ceremony:** verification — the second guilloché band draws
  itself around the rosette (400ms stroke-reveal). An engraving, not
  confetti. Fired once, at the moment of verification.
- **Security ops:** steady 400ms fades. Never skeleton shimmer — crypto
  should feel deliberate, not magical.

## Voice quarantine (keeps the system honest)
- Plex Mono only for cryptographic facts.
- 700-weight statement type only for pledge/onboarding/empty states —
  if it shows up in settings chrome, the system is being misused.
- `accent` is the only color allowed to mean "we intend this"; `error`
  only ever means a real failure.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-07-14 | Initial system created by /design-consultation | Product context + competitive research (Signal/Telegram/Session/SimpleX/Threema screenshots) + blind subagent second opinion |
| 2026-07-14 | Direction 1 "Kept Promise" (cream paper, Fraunces serif, clay accent) REJECTED at preview | User: cream reads dated; serif wrong voice; clay reads "Claude orange" |
| 2026-07-14 | Direction 2 "The Instrument" accepted; green accent REJECTED, then Passport Burgundy passed over | User picked Aubergine from a 3-way rendered face-off (burgundy / aubergine / iron-ink) |
| 2026-07-14 | Accent = Aubergine `#5B3A6C`/`#B08CC9`, defined as a swap-point token | Founder wants the option to change color later at near-zero cost — see "THE ACCENT IS A SWAP-POINT" |
| 2026-07-14 | Identicon = guilloché Rosette, verified = engraved band + quiet ✓ | Anti-forgery engraving heritage fits a key-derived mark; no competitor owns rotational fine-line geometry |
