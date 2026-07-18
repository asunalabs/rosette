# Tasks: iOS app (the deferred half of T8)

The frontend track (T8) split in the architecture: Android + desktop
unblocked first, **iOS is the timeboxed follow-up**. This doc is that
follow-up — what it takes to bring the existing Compose Multiplatform app
(`app/`) up on iOS, ordered, with the Mac-gated work called out separately
from what can be authored on Linux.

Nothing here is started: there is no `iosApp/`, no `iosMain`, no iOS task in
the pivot doc. `engine-kt` already Mac-gates its iOS targets and `ffi` already
declares the `staticlib` crate-type, so the Rust seam is ready — the gap is
entirely on the Kotlin/Swift/Xcode side.

**Author:** JustFossa, 2026-07-18.

---

## First: how to build iOS without owning a Mac

You cannot produce or run an iOS build on Linux alone. The Apple SDK, code
signing, the simulator, and Kotlin/Native's Apple linker are all macOS-only.
Rust can *cross-compile* object code to `aarch64-apple-ios` from Linux (via
osxcross), but the final framework link, the `.app`, and running anything
still need a real Mac. So the question isn't "how do I avoid a Mac" — it's
"where does the Mac live." Ranked laziest-first:

1. **GitHub Actions `macos-14` runners (recommended for the build/test loop).**
   This repo already runs on GitHub Actions (`.github/workflows/ci.yml`). Add
   one job on `runs-on: macos-14` that builds the framework and runs the
   simulator tests. You develop Kotlin/Swift on Linux, push, and a real
   Apple-hosted Mac compiles it. This is the "something else" that is **not** a
   VM and **not** a license gray area. Free for public repos; private repos
   bill macOS minutes at 10× the Linux rate, so keep the iOS job on a path
   filter (only when `app/**` changes) — see iOS-9.

2. **A KMP-aware CI service — Codemagic / Bitrise / Xcode Cloud.** Codemagic
   specifically markets Compose Multiplatform iOS builds and hands you the
   `.ipa` + a simulator preview. Worth it if you want managed signing and
   artifacts rather than hand-rolling the workflow.

3. **A cloud or physical Mac for interactive work.** CI compiles, but you
   still need a real Mac to *drive the simulator* and debug UI. Options: AWS
   EC2 mac instances, MacStadium/MacinCloud (rent by the hour), or — cheapest
   long-term if iOS is a real target — a used Mac mini you SSH into. Run
   `./gradlew :composeApp:iosSimulatorArm64Test` and Xcode there.

4. **A macOS VM on this Arch box (OSX-KVM / Docker-OSX) — not recommended.**
   It technically boots, but it **violates Apple's macOS license** (macOS may
   only be virtualized on Apple hardware), has no real GPU, and the simulator
   is flaky. Fine to know it exists; don't build a workflow on it.

**Bottom line for a Linux-primary dev:** CI on `macos-14` for the
build/test loop (iOS-9), plus a cheap Mac mini or hourly cloud Mac for the
handful of times you need to *see* the simulator. The Kotlin and Swift are
just files — you write them here; a real Mac links and runs them.

---

## What "build the iOS app" actually is, in this codebase

The Compose UI in `composeApp/` is already multiplatform — `commonMain` holds
every screen (onboarding, chat list, conversation, settings, find-people).
iOS needs four things layered on top:

1. **Targets + framework** — `composeApp` currently declares only
   `androidTarget` + `jvm("desktop")`. Add the three iOS targets and a
   `binaries.framework` so Xcode has something to link.
2. **Six `expect`/`actual` seams** — every `expect` in `commonMain` has an
   `.android.kt` and `.desktop.kt` actual but **no iOS actual**. Missing iOS
   actuals = the module won't link. They are: `formatClockTime`,
   `defaultDirectoryBaseUrl`, `sha256Hex`, `defaultRegionCode`,
   `rememberSessionStore`, `rememberDbConfig`/`deleteDbFile`.
3. **A Compose entry point** — `MainViewController.kt` in `iosMain` wrapping
   `App()` in a `ComposeUIViewController`.
4. **An Xcode host** — an `iosApp/` project whose `App` embeds that view
   controller and links `ComposeApp.framework` + the Gobley-built Rust
   `staticlib`.

`engine-kt` needs almost nothing: it already conditionally declares the iOS
targets, and Gobley cross-builds the `ffi` staticlib for them. The only setup
is `rustup target add` for the three Apple triples (iOS-3), which runs on the
Mac.

---

## Task list

Legend: **[L]** authorable on Linux (won't compile until a Mac links it),
**[M]** requires a Mac to complete/verify. Effort is human / CC.

**Status (2026-07-18):** the Linux-authorable batch (iOS-1, iOS-2, iOS-4,
iOS-5, iOS-7, iOS-9) plus the iOS-8 Xcode-host spec and iOS-11 unsigned-.ipa
release workflow are written and awaiting their first macOS CI run.
`Sha256Test` and the composeApp config verified green on the desktop target
here (`./gradlew :composeApp:desktopTest`); all workflow/XcodeGen YAML lints
clean. The iosMain + Swift sources compile only on a Mac / the `macos-14`
jobs. Mac-verified tasks (iOS-3, iOS-6, iOS-10) remain open.

**Install decision (2026-07-18):** distribution to the founder's iPhone is
**unsigned .ipa + sideload** (free Apple ID via AltStore/Sideloadly), not
paid ad-hoc or TestFlight. So CI carries no Apple certificates and the device
UDID is unused — the sideload tool registers the device and signs at install
time. Revisit if a paid Developer account lands (would unlock OTA install and
TestFlight; see iOS-11).

- [ ] **iOS-1 [L] (human ~30min / CC ~10min)** — composeApp — declare iOS targets + framework
  - Add `iosArm64()`, `iosSimulatorArm64()`, `iosX64()` to `composeApp/build.gradle.kts`, each with a `binaries.framework { baseName = "ComposeApp"; isStatic = true }` block. Add an `iosMain` source set depending on `commonMain`.
  - Guard the targets the same way `engine-kt` does (`if (GobleyHost.Platform.MacOS.isCurrent)`) so `:composeApp` still configures on Linux/Windows for Android+desktop.
  - Files: `composeApp/build.gradle.kts`
  - Verify (Mac): `./gradlew :composeApp:tasks` lists `linkDebugFrameworkIos*`.

- [ ] **iOS-2 [L] (human ~15min / CC ~5min)** — composeApp — Ktor engine for iosMain
  - `commonMain` uses `ktor-client-cio` (JVM/Android/desktop). Ktor 3.0.3's CIO client does support Native, but the idiomatic iOS engine is Darwin (URLSession). Decision: try CIO first (one engine everywhere, laziest); if `iosMain` fails to resolve CIO, add `io.ktor:ktor-client-darwin` to an `iosMain.dependencies` block and move the engine selection behind an `expect fun httpClientEngine()`.
  - Files: `composeApp/build.gradle.kts`, version catalog if Darwin is added.
  - Verify (Mac): framework links with `DirectoryClient` reachable.

- [ ] **iOS-3 [M] (human ~15min / CC ~5min)** — engine-kt — Rust iOS targets + staticlib link
  - On the Mac: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`. Gobley then builds `ffi`'s `staticlib` per iOS target automatically (crate-type is already declared). No Gradle change expected — `engine-kt` already adds the iOS targets under the Mac guard.
  - Files: none (toolchain only), unless Gobley needs an explicit `cargo { builds.native { … } }` embed flag — mirror the existing `builds.jvm` embed guard if so.
  - Verify (Mac): `./gradlew :engine-kt:linkDebugFrameworkIosSimulatorArm64` produces the framework; `FfiSmokeTest` runs on the sim.

- [ ] **iOS-4 [L] (human ~20min / CC ~10min)** — composeApp — trivial iOS actuals (ClockTime, DirectoryBaseUrl, DialCode)
  - `ClockTime.ios.kt`: `formatClockTime` via `NSDateFormatter` (time style short) on `NSDate.dateWithTimeIntervalSince1970(epochMs / 1000.0)`.
  - `DirectoryBaseUrl.ios.kt`: return the same production/default URL string the Android+desktop actuals use (check both before writing — keep them identical).
  - `DialCode.ios.kt`: `defaultRegionCode` from `NSLocale.currentLocale.countryCode` (falls back to `""`, which `dialCodeFor` already tolerates).
  - Files: three new `*.ios.kt` under the matching packages in `iosMain`.
  - Verify (Mac): compiles; onboarding shows the right dial code in the sim.

- [ ] **iOS-5 [L] (human ~30min / CC ~15min)** — composeApp — `sha256Hex` iOS actual
  - Android/desktop use `MessageDigest`. iOS: `platform.CoreCrypto`/`CommonCrypto` `CC_SHA256`, or `platform.CryptoKit.SHA256` via cinterop. CommonCrypto is the fewer-moving-parts choice (no cinterop def file). Hex-encode lowercase to match the other two actuals exactly — the directory's k-anonymity prefix bucketing depends on byte-identical hashing across platforms, so add a shared test vector.
  - Files: `Sha256.ios.kt` in `iosMain`; a `commonTest` vector asserting a known input→digest if one doesn't already exist.
  - Verify (Mac): the shared SHA-256 vector passes on `iosSimulatorArm64Test`.

- [ ] **iOS-6 [M] (human ~1-1.5d / CC ~2-3h)** — composeApp — **replace the NSUserDefaults placeholders with Keychain** for `SessionStore` + `DbKeyStore`
  - **Placeholder landed (authored):** `SessionStore.ios.kt` and `DbKeyStore.ios.kt` currently back onto `NSUserDefaults` so the framework links and the app runs for sideload dogfooding. **This is not at-rest secure** — the SQLCipher key and session token sit in the app's plist in plaintext. Marked `ponytail:` in both files. Random key uses `SecRandomCopyBytes`; `dbPath` under Application Support; `deleteDbFile` via `NSFileManager`.
  - **The actual work (this task):** swap both onto the **Keychain** (`Security`: `SecItemAdd`/`SecItemCopyMatching`/`SecItemDelete`, `kSecClassGenericPassword`, accessibility `kSecAttrAccessibleAfterFirstUnlock` to match the "released when the device is unlocked, no unlock screen" contract in `DbKeyStore.kt`). Factor a tiny `Keychain.ios.kt` helper (get/set/delete over `service`+`account`) shared by both — `ponytail:` one helper, not a keychain abstraction layer. The CoreFoundation dictionary interop is the fiddly part and is exactly why the placeholder shipped first: it's unverifiable off-device.
  - **Blocking gate:** the Keychain swap MUST land before any distribution past the founder's own device. A build with the plaintext-plist key must never reach real users.
  - Files: `SessionStore.ios.kt`, `DbKeyStore.ios.kt`, new `Keychain.ios.kt` in `iosMain`.
  - Verify (Mac + device): install → onboard → kill → relaunch keeps you logged in and the DB opens; reset wipes both; nothing sensitive in the app's `NSUserDefaults` plist.

- [ ] **iOS-7 [L] (human ~15min / CC ~5min)** — composeApp — Compose entry point
  - `MainViewController.kt` in `iosMain`: `fun MainViewController(): UIViewController = ComposeUIViewController { App() }`. Nothing else — `App()` already owns the nav and theme.
  - Files: `composeApp/src/iosMain/kotlin/chat/app/MainViewController.kt`
  - Verify (Mac): referenced by the Xcode host (iOS-8) and renders.

- [x] **iOS-8 [L authored / M verify] (human ~0.5-1d / CC ~1h)** — iosApp — Xcode host project (XcodeGen)
  - **Authored.** `app/iosApp/` uses an **XcodeGen** `project.yml` (CI runs `xcodegen generate`; the `.xcodeproj` is not committed, same as the Gobley bindings) instead of a hand-written `project.pbxproj`. A SwiftUI `@main App` (`App.swift`) hosts a `UIViewControllerRepresentable` (`ContentView.swift`) wrapping `MainViewControllerKt.MainViewController()`. The `Compile Kotlin Framework` preBuildScript runs `./gradlew :composeApp:embedAndSignAppleFrameworkForXcode`; `FRAMEWORK_SEARCH_PATHS` points at `composeApp/build/xcode-frameworks/$(CONFIGURATION)/$(SDK_NAME)`. `ComposeApp.framework` linked, not embedded (static). `ENABLE_USER_SCRIPT_SANDBOXING=NO` so the Gradle phase can write the build dir. Bundle id `chat.app`, display name `chat`, deployment target iOS 15.
  - Files: `app/iosApp/project.yml`, `app/iosApp/iosApp/App.swift`, `app/iosApp/iosApp/ContentView.swift` (Info.plist is generated by XcodeGen from `project.yml`).
  - **Deferred within this task:** IBM Plex TTF bundling (confirm Compose Resources serves fonts on iOS; if not, add to bundle + `UIAppFonts`) and a plum-black launch screen (avoid white first-paint flash, DESIGN.md). Neither blocks a first installable build.
  - Verify (Mac/CI): `xcodegen generate && xcodebuild ... archive` succeeds (exercised by iOS-11); app launches to onboarding on device.

- [ ] **iOS-11 [L authored / M verify] (human ~30min / CC ~20min)** — CI — unsigned .ipa to Releases (sideload)
  - **Authored.** `.github/workflows/release-ios.yml` on `macos-14`: `xcodegen generate` → `xcodebuild archive` with `CODE_SIGNING_ALLOWED=NO` → zip `Payload/iosApp.app` into `chat-unsigned.ipa` → publish via `gh release`. Triggers: `workflow_dispatch` (updates a rolling `ios-latest` prerelease) and `push` tag `ios-v*` (pinned release). **Unsigned by design** — the install path is AltStore/Sideloadly, which re-signs against a free Apple ID at install time, so CI holds no Apple secrets and the device UDID is unused.
  - Files: `.github/workflows/release-ios.yml`
  - Verify (CI): run the workflow → an `ios-latest` release appears with `chat-unsigned.ipa`. Verify (device): AltStore/Sideloadly installs it; it launches to onboarding.
  - If a paid Developer account lands later: add signing secrets (cert `.p12` + password, ad-hoc provisioning profile with the UDID, team id), sign in `xcodebuild`, and emit an `itms-services` manifest for tap-to-install OTA — or switch to a TestFlight upload step.

- [ ] **iOS-9 [L] (human ~30min / CC ~15min)** — CI — iOS build/test job on macos-14
  - Add a job to `.github/workflows/ci.yml`: `runs-on: macos-14`, path-filtered to `app/**`, that runs `./gradlew :composeApp:iosSimulatorArm64Test` (and `:engine-kt:` iOS tests). This is the Linux dev's actual build machine (see top section). Keep it a separate job so the existing `ubuntu-latest` cargo job is unaffected; gate on the path filter to avoid burning 10×-billed macOS minutes on Rust-only changes.
  - Files: `.github/workflows/ci.yml`
  - Verify: green iOS job on a PR that touches `app/`.

- [ ] **iOS-10 [M] (human ~0.5d / CC —)** — QA — first real-device pass
  - Once the sim is green: run `/ios-qa` (the gstack live-device iOS skill) against a physical iPhone. Check the six screens against DESIGN.md, Keychain persistence across reinstall, and the FFI pairing path end-to-end. Distribution/signing (`architecture.md`) is out of scope here — this is dogfood, not App Store.
  - Verify (Mac + device): onboarding → pair → send an encrypted message on hardware.

---

## Ordering / critical path

Authored on Linux, verified by CI on `macos-14`:
**iOS-1 → iOS-2 → iOS-4 → iOS-5 → iOS-7** (targets, engine, trivial actuals,
SHA-256, entry point), **iOS-8** (XcodeGen host), the **iOS-9** test job, and
**iOS-11** (unsigned-.ipa release). Everything needed for a first installable
build is written; `macos-14` is the compiler.

Still needs Mac-verified iteration:
**iOS-3** (Rust targets — the release/test workflows add
`aarch64-apple-ios`, but a full sim run may want the sim triples too) and
**iOS-6** (Keychain `SessionStore`/`DbKeyStore` — the one real chunk of code
left), then **iOS-10** (device QA).

The path to an installable build on the founder's iPhone: push everything, run
the **release-ios** workflow (Actions → Run workflow → release-ios), download
`chat-unsigned.ipa` from the `ios-latest` release, and sideload it with
AltStore/Sideloadly (sign in with your free Apple ID; the tool registers the
device and signs — no UDID needed from CI). The app runs and persists across
restarts on the NSUserDefaults placeholder; the Keychain swap (iOS-6) hardens
at-rest storage and gates any wider distribution.

First CI red, if any, is most likely one of: CIO-on-iOS not linking (→ iOS-2's
one-line Darwin fallback), an `embedAndSignAppleFrameworkForXcode` env quirk
(static framework lowers that risk), or a Kotlin/Native Foundation-interop
signature mismatch in the placeholders (`setObject`/`createDirectoryAtPath`
arg shapes). All are quick fixes once the runner shows the exact error — which
is the whole point of pushing to `macos-14` rather than guessing here.
