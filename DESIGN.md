# Design System — "Quiet Room" (chat, working name — OQ1)

Source of truth for every visual/UI decision. Rendered reference (live
tokens, live Rosette algorithm, mocked phone + desktop screens, light+dark
toggle): `docs/design/design-preview.html` — open it in a browser; it IS
this file executed.

Supersedes "The Instrument" (2026-07-14, same day): the founder dropped the
engraved-instrument structural language after seeing it built; the identity
assets (accent, Rosette, Plex) survive. See Decisions Log.

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
- **Direction:** Quiet Room — Signal's structural language wearing this
  product's identity. A dark, calm, soft-surfaced messenger: plum-black
  canvas, rounded cards, big friendly statement type, pill buttons, roomy
  whitespace. Confidence through calm, not engraving.
- **Reference:** founder-supplied Signal iOS/desktop screenshots (Figma
  file `7Zz8Iglr5Q2bIliXLmvWZx`). Structure is borrowed; color and the
  identicon are ours.
- **Decoration level:** minimal — surfaces and type do the work; the
  Rosette is the only ornament. No gradients, no blobs, no glassmorphism;
  one soft shadow tier for modals/FAB.
- **Mood:** calm, private, quietly confident. Familiar messenger anatomy so
  it never reads experimental; aubergine + Rosette so it never reads
  Signal-clone. Must NOT read as generic Material — no Material default
  colors, elevation tints, or stock component shapes.
- **Hard NOs (user-vetoed or field-owned):** cream/warm-paper surfaces,
  serif faces, orange/clay/amber accents, green accents, Signal/Telegram
  blue, black+neon paranoia styling, purple *gradients* (the flat aubergine
  accent is deliberate; gradients of it are not).

## Typography
One superfamily. Voice = weight and size, never a second family.
- **Display/statement:** IBM Plex Sans 700, tracking -0.02em — onboarding
  headlines (centered), empty states. 27–34sp mobile.
- **Body:** IBM Plex Sans 400 — messages 15.5–16sp / 1.4–1.5, settings copy.
  Explanation text under statements is `muted`, never competing.
- **UI/labels:** IBM Plex Sans 500–600 — names 600 (16sp), button labels
  600 (16sp) on pills, secondary/meta 500 (13–14sp), caption 12sp.
- **Data:** IBM Plex Sans with `tnum` — timestamps, unread counts.
- **Code/crypto facts:** IBM Plex Mono 400 — safety-number words, key
  fingerprints, OTP digits, username discriminators (`mira#07`).
  QUARANTINED to those moments: mono means "verifiable cryptographic fact".
- **Coverage:** Plex covers Latin extended + Cyrillic + Greek. OFL,
  bundleable in the APK.
- **Loading:** bundle OFL TTFs in the app (`composeApp` resources); web
  surfaces may use Google Fonts.
- **Scale:** 11 / 12 / 13.5 / 15 / 16 / 17 / 22 / 27 / 32–34 (sp).

## Iconography
- **Set:** Lucide (ISC license) — 2dp stroke, round caps/joins,
  `currentColor`. Never filled Material icons, never emoji as UI icons
  (the one exception: country flag emoji in the phone country-code segment).
- **Sizes:** 16dp inline/list, 18–20dp buttons/rail, 22dp FAB.
- **Compose:** bundle the needed Lucide glyphs as vector drawables (or the
  `lucide-compose` port) — do not pull Material Icons extended.

## Color
- **Approach:** restrained — neutrals do the living; ONE working accent;
  semantic colors only for their one job. Light = pure white + near-black
  ink; dark = plum-black (near-true-black with a plum undertone) + off-white.

### THE ACCENT IS A SWAP-POINT (explicit requirement, 2026-07-14)
The accent must be trivially replaceable. Rules:
1. **Semantic naming only.** The token is `accent` (+ `accentStrong`,
   `accentSoft`, `bubbleMine`) — never "aubergine" in code, resources, or
   component styles. Nothing references the hex outside the theme object.
2. **One definition site.** Compose: a single `ChatPalette` (light+dark)
   in the theme package. Web/preview: the `:root`/`[data-theme]` CSS
   variables in `docs/design/design-preview.html`.
3. **What follows automatically on swap:** primary pill buttons, unread
   badges, verified ✓, links/ghost buttons, focus rings, FAB, send button,
   own-bubble fill (`bubbleMine` = accent family, solid), `accentSoft` fills.
4. **What must be re-checked on swap (30-min checklist):** accent-on-white
   ≥ 4.5:1 and dark-accent-on-`#0B090C` ≥ 4.5:1; white text on `bubbleMine`
   ≥ 4.5:1 in both themes; error red still clearly distinct; re-curate the
   Rosette 12-ink palette so ink #0/#1 harmonize with the new accent.

### Tokens (current accent: Aubergine)
| Token | Light | Dark | Job |
|---|---|---|---|
| `bg` | `#FFFFFF` | `#0B090C` | app background (dark = plum-black) |
| `surface` | `#F7F6F6` | `#1C1920` | cards, bars, search/input pills |
| `surface2` | `#EFEDEE` | `#262229` | pressed/nested surfaces, icon chips |
| `ink` | `#1A1618` | `#ECE9EB` | primary text |
| `muted` | `#625B5E` | `#948D91` | secondary text, timestamps |
| `hairline` | `#E5E1E3` | `#2A252B` | dividers (sparingly — inside grouped cards, pane borders) |
| `accent` | `#5B3A6C` | `#B08CC9` | THE working color: pills, badges, verified, links, FAB |
| `accentStrong` | `#432852` | `#C3A4D8` | pressed accent |
| `accentSoft` | `#EDE4F2` | `#2B2233` | verified/positive fills, selected rail item |
| `bubbleMine` | `#5B3A6C` | `#6B4183` | own bubbles — SOLID accent family |
| `onBubbleMine` | `#FFFFFF` | `#FFFFFF` | text/time on own bubbles (time at 72% opacity) |
| `bubbleTheirs` | `#F0EEF0` | `#242027` | incoming bubbles |
| `error` | `#C62828` | `#F16A6F` | real failures only |
| `errorSoft` | `#FBE9E7` | `#35201B` | error fills |
| `warning` | `#7A5500` | `#D4A945` | expiring links etc. |
| `warningSoft` | `#F5ECD4` | `#322B18` | warning fills |

Every `*Soft` fill is a text background, not decoration — `InstrumentStatusChip`
renders `warning` **on** `warningSoft` — so each pair owes the 4.5:1 above, not
3:1 (chip text is `labelMedium`, 13.5sp, which is not "large text"). Measured:

| pair | light | dark |
| --- | --- | --- |
| `warning` on `warningSoft` | 5.70 | 6.40 |
| `error` on `errorSoft` | 4.80 | 5.12 |

The two that moved were introduced failing and fixed on 2026-07-16 (ET12), not
re-tuned for taste: light `warning` was `#8F6400` (**4.47**, under this file's
own bar) and dark `error` was `#E5484D` (**3.91**). Re-measure this table when
any of the four change — the pair is the unit, not the token.
| `info` | `#4C5A6B` | `#94A3B8` | connection banner text |
| `onAccent` | `#FFFFFF` | `#121013` | text on `accent` fills |
- **Success/verified = accent**, not green: trust wears the brand color.
- **Dark mode strategy:** redesigned surfaces (plum-tinted near-blacks),
  accent lightened for contrast; never gray-flipped. `bubbleMine` stays a
  mid, saturated aubergine in dark so own messages read as color (white
  text on `#6B4183` ≈ 7.6:1).
- QR codes always sit on a white tile (`#FFFFFF`, 12dp radius) so they scan
  in either theme.

## The Rosette — the one visual signature (kept from The Instrument)
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
- **Special placements:** center of the device-link QR (52dp, light inks on
  the white tile); onboarding welcome art (~116dp).
- **Anti-patterns:** no grids/blockies, no humanoid/animal forms, no sharp
  spikes, no letterforms, no initials-circles.

## Shape
- **Buttons & inputs:** full-pill (999dp). Primary = `accent` fill +
  `onAccent` 600 label; secondary = `surface` fill + `ink` label. Height
  ~50dp, generous 24dp horizontal padding.
- **Cards/sheets:** 16dp. Grouped settings cards are inset (16dp side
  margins) with hairline row dividers inside.
- **Bubbles:** uniformly rounded 18dp — no tail, no cut corner. Rounded won
  over rectangular because pill CTAs + circular rosettes make a rectangle
  the only sharp element on screen.
- **OTP cells:** 12dp. Small chips/badges: pill.
- **Elevation:** flat; one soft shadow tier for modals/FAB only.

## Spacing
- **Base unit:** 8dp (4dp half-step). Scale: 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64.
- **Density:** comfortable. Chat-list rows 72dp (48dp rosette); bars 56dp;
  screen gutters 24dp on onboarding, 16dp in lists.
- **Bubbles:** 14dp horizontal / 9dp vertical padding; 5dp between
  same-sender bubbles, 12dp on sender change. Timestamp floats inline at
  the end of the last text line (11sp) — never its own row.
- **Onboarding rhythm:** one idea per screen; art/headline centered;
  explanation muted below; CTA stack pinned to the bottom (primary pill,
  then optional secondary pill 12dp below).

## Layout

> **Read the SPECCED, NOT BUILT markers below before filing a QA bug.** CLAUDE.md
> makes this file normative and tells QA to flag code that doesn't match it, so a
> spec written in the present tense about something unbuilt manufactures a
> permanent QA violation — the reviewer can only conclude the code is wrong.
> Marked entries are the design we intend; the "Today:" line under each is what
> the app actually renders. Added by ET9 (2026-07-16) after DOC-1 found three of
> them; drop a marker only in the commit that makes its claim true.

- **Approach:** standard messenger anatomy on purpose (familiarity is the
  wedge). Onboarding flow: welcome → phone verify (flag + country code in
  the input pill's leading segment) → OTP (mono cells) → username claim.
  Secondary welcome CTA is "Restore account".
- **Breakpoint:** 700dp — desktop is icon rail (60dp) + chat list pane
  (290dp) + conversation pane. Device link is a centered card: QR on white
  tile + numbered steps.
- **Bottom navigation (mobile):** floating pill tab bar, selected tab =
  `surface2` pill. **Settings is NOT a tab** (amended 2026-07-16) — see "You
  menu" below.
  - **SPECCED, NOT BUILT (DT9).** *Today:* `App.kt:92` renders
    `listOf("Chats", "Find people")`. Neither the old spec (Chats / Calls) nor
    the new one (Find people is a FAB destination, below) describes it — the tab
    bar currently matches nothing, which is why this marker exists.
- **You menu (amended 2026-07-16):** the chat list's top-left is your own
  Rosette (36dp). Tapping it expands a Signal-style dropdown carrying your
  handle and a link to the Settings screen. Rationale: Settings is chrome,
  not a destination — the tab bar is for places you go, and your identity
  belongs where Signal's structural language puts it. The dropdown is the
  ONLY route to Settings.
  - **SPECCED, NOT BUILT (DT4).** *Today:* `ChatListScreen.kt` renders a plain
    `Text("Chats")` top-left, no Rosette and no dropdown. **There is no Settings
    screen at all**, so "the dropdown is the only route to Settings" is
    currently true only in the vacuous sense.
- **Your handle is never hidden.** It appears in the You menu in Plex Mono
  (`mira#07` is a crypto fact, per Typography) and is tap-to-copy. A user who
  cannot recite their own handle cannot be found, which defeats the directory.
  - **SPECCED, NOT BUILT (DT4)** — depends on the You menu above.
- **FAB:** 52dp accent circle, bottom-right above the tab bar. Opens **Find
  people** as a pushed screen (amended 2026-07-16 — Find people is a FAB
  destination, not a tab; that's why it has a back affordance).
  - **SPECCED, NOT BUILT (DT9).** *Today:* no FAB exists anywhere, and Find
    people is reached as a tab (`App.kt:92`) — the exact arrangement this entry
    amends away from.
- **Verification lives in the conversation (amended 2026-07-16):** the
  conversation header's name is tappable → contact sheet (Rosette large,
  handle in mono, "Verify safety number") → compare screen → on success,
  the ceremony fires. "Is this really them?" is a question asked *inside a
  conversation*, so it is answered there — never in Settings.
  - **SPECCED, NOT BUILT (DT6).** *Today:* `ConversationScreen.kt` renders the
    name as plain text with no `clickable`, there is no contact sheet or compare
    screen, and `markVerified` has no call site in the app.

## Motion
- **Approach:** minimal-functional; calm.
- **Easing:** deceleration only (Compose `FastOutSlowInEasing` /
  `LinearOutSlowInEasing`); no springs, no overshoot, nothing bounces.
- **Duration:** micro 100ms · short 150–200ms · medium 250ms ·
  ceremony 400ms.
- **The one ceremony:** verification — the second guilloché band draws
  itself around the rosette (400ms stroke-reveal). Fired once, at the
  moment of verification.
- **Security ops:** steady 400ms fades. Never skeleton shimmer.

## Voice quarantine (keeps the system honest)
- Plex Mono only for cryptographic facts.
- 700-weight statement type only for onboarding/empty states — if it shows
  up in settings chrome, the system is being misused.
- `accent` is the only color allowed to mean "we intend this"; `error`
  only ever means a real failure.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-07-14 | Initial system "The Instrument" created by /design-consultation | Product context + competitive research + blind second opinion |
| 2026-07-14 | Direction 1 "Kept Promise" (cream, serif, clay) REJECTED at preview | Cream reads dated; serif wrong voice; clay reads "Claude orange" |
| 2026-07-14 | Accent = Aubergine `#5B3A6C`/`#B08CC9` as a swap-point token | Picked in 3-way rendered face-off; must stay trivially replaceable |
| 2026-07-14 | Identicon = guilloché Rosette, verified = engraved band + quiet ✓ | Anti-forgery engraving heritage; no competitor owns it |
| 2026-07-14 | **"Quiet Room" replaces "The Instrument"** — structural language dropped after founder saw it built ("reads generic Material"); Signal iOS/desktop screenshots supplied as the structural reference | Keep identity (aubergine, Rosette, Plex), adopt Signal's skeleton: plum-black canvas, pill CTAs, 16dp cards, statement onboarding |
| 2026-07-14 | Bubbles: solid-accent outgoing + white text, quiet-gray incoming, uniform 18dp, inline trailing timestamps | Old tinted `bubbleMine` read washed-out; timestamp-as-row wasted space; rounded beats rectangular next to pills + circles |
| 2026-07-14 | Iconography = Lucide, strokes only | Founder rejected placeholder glyphs; Lucide is ISC, consistent, non-Material |
| 2026-07-14 | Onboarding priority order; flag emoji in country-code segment; "Restore account" CTA copy | Founder: old onboarding screens were the weakest surface |
| 2026-07-16 | **Settings moves out of the tab bar** into a top-left Rosette → Signal-style dropdown. Tab bar becomes Chats / Calls | /plan-design-review: Settings is chrome, not a destination; matches the Signal skeleton already adopted; frees the tab bar |
| 2026-07-16 | **Find people is a FAB destination, not a tab** | Its back arrow proved it was never a tab; restores DESIGN.md's specced FAB and gives the chat list its missing compose affordance |
| 2026-07-16 | **Verification = conversation header → contact sheet → safety numbers**, then the ceremony | The trust question is asked inside a conversation, so it's answered there. `markVerified` had zero call sites; the Rosette's verified band and "the one ceremony" were unreachable code |
| 2026-07-16 | **`warning` token owns "we're waiting on our own infrastructure"; `error` stays quarantined to real user-facing failures** | A vendor outage is not the user's failure. Rendering it in `error` both spends the one token that means something and blames the wrong party (T27 held state) |
| 2026-07-16 | **Onboarding steps get a back affordance; `error` clears on every transition** | A mistyped number was an unescapable dead end; `onRestore`'s message leaked across steps and glowed red under an unrelated CTA |
