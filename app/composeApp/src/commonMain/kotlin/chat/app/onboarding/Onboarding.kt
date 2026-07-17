package chat.app.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.app.directory.DirectoryClient
import chat.app.directory.DirectoryException
import chat.app.directory.RestoreBegin
import chat.app.directory.VerifyResult
import chat.app.directory.normalizePhoneInput
import chat.app.theme.ChatMonoStyle
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentField
import chat.app.theme.InstrumentPhoneField
import chat.app.theme.InstrumentStatusChip
import chat.app.theme.LocalChatPalette
import chat.app.theme.Rosette
import chat.app.theme.StatusTone
import chat.engine.BackupBundle
import chat.engine.backupAuthProof
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * T27: phone verification gates the app itself, Signal-style — there is no
 * unverified-but-usable state. This flow runs before `ChatEngine` is ever
 * constructed; `onComplete` is the only way out.
 *
 * Layout per DESIGN.md "Onboarding rhythm": one idea per screen, centered
 * statement type, muted explanation, pill CTA stack pinned to the bottom.
 */
sealed interface OnboardingState {
    data object Welcome : OnboardingState
    /** [held] = `POST /signup` couldn't reach the OTP vendor (503), so no code was sent — same rule as [AwaitingOtp.held], one screen earlier (ET17). */
    data class PhoneEntry(val held: Boolean = false) : OnboardingState
    /**
     * [held] = the directory could not check the code at all (vendor outage,
     * 503 — see [isVerificationUnavailable]); the code is unproven, not wrong.
     *
     * [attempts] = how many times in a row we failed to reach verification
     * (ET3). It carries the chip's escalation, and it is also what makes a
     * repeated outage *visible*: `state = AwaitingOtp(phone, held = true)` twice
     * running assigns an equal data-class value, and Compose skips recomposition
     * on an equal value — so before this, tapping "Try again" into a continuing
     * outage changed nothing on screen and read as a dead button.
     */
    data class AwaitingOtp(
        val phone: String,
        val held: Boolean = false,
        val attempts: Int = 0,
    ) : OnboardingState
    data class ClaimUsername(val sessionToken: String, val phone: String) : OnboardingState

    /** Issue #2: mandatory recovery PIN, set right after the username. */
    data class SetPin(val sessionToken: String, val handle: String, val phone: String) : OnboardingState

    /** Issue #2: one-time display of the 5-word phrase that recovers the PIN. */
    data class ShowPhrase(
        val sessionToken: String,
        val handle: String,
        val phone: String,
        val phrase: String,
    ) : OnboardingState

    /** Issue #3: restore-account flow behind the Welcome CTA. */
    data object RestorePhone : OnboardingState
    data class RestoreOtp(val phone: String) : OnboardingState
    data class RestorePin(val begin: RestoreBegin, val phone: String) : OnboardingState
    data class RestorePhrase(val begin: RestoreBegin, val phone: String) : OnboardingState

    /** Issue #3: phrase-path success forces a fresh PIN before entering the app. */
    data class RestoreNewPin(
        val begin: RestoreBegin,
        val phone: String,
        val bundle: BackupBundle,
        val phrase: String,
    ) : OnboardingState
}

/**
 * Issue #3: everything App needs to finish a restore — build the engine
 * from the bundle with `ChatEngine.newFromBackup`, save the session, and
 * (phrase path, [newPin] != null) rewrap + re-upload before entering.
 */
data class RestoreRequest(
    val bundle: BackupBundle,
    val secret: String,
    val newPin: String?,
    val sessionToken: String,
    val phone: String,
)

/**
 * T27's gate, extracted as one pure decision so it is testable without a UI.
 *
 * **Defense-in-depth as of ET6, not the gate.** The real gate is now server-side:
 * `directory` mints a session only for a code the OTP vendor affirmatively
 * approved, so `verified == false` should never reach this client. This check is
 * deliberately kept (founder decision, 2026-07-16) so a future server regression
 * cannot walk an unverified human into the app. It costs five lines.
 *
 * The `else` branch is therefore unreachable from a correct server. Do not
 * delete it, and do not treat it as the vendor-outage path — that is
 * [isVerificationUnavailable], driven off the 503.
 */
internal fun nextAfterVerify(phone: String, result: VerifyResult): OnboardingState =
    if (result.verified) {
        OnboardingState.ClaimUsername(result.sessionToken, phone)
    } else {
        OnboardingState.AwaitingOtp(phone, held = true)
    }

/**
 * ET11: survives rotation and process death.
 *
 * Without this, `state` was `remember`, so a rotation on the OTP step dropped
 * the user back at Welcome and re-entering the number **burned another real
 * SMS** — a rotation cost money and a vendor quota. `ClaimUsername` is the one
 * that matters most: it holds a live `sessionToken`, and losing it strands a
 * verified account with no username.
 *
 * Hand-written rather than `@Parcelize`: `OnboardingState` lives in `commonMain`
 * and Parcelize is Android-only. A list of primitives is what
 * `rememberSaveable`'s default bundle saver already knows how to store on every
 * target.
 *
 * The tag is positional and must stay in sync with [restore] — a saver that
 * silently mis-restores is worse than one that doesn't save, so an unknown tag
 * throws rather than guessing.
 *
 * **Issue #2/#3 states save their strings; the restore path does not (merge
 * decision, 2026-07-17).** `SetPin`/`ShowPhrase` are the signup path and hold
 * only what `ClaimUsername` already justifies saving, plus the phrase — losing
 * that screen to a rotation would cost the user their one and only look at it.
 * The three post-OTP restore states hold `RestoreBegin`/`BackupBundle` instead:
 * salts, wrap material, and the encrypted identity blob. Those are not going
 * into `savedInstanceState`, which the system may write to disk — so they
 * collapse to `RestorePhone` and the user re-enters their number. That costs one
 * SMS in a rare case, and the ≤10-min restore token has usually expired across a
 * process death anyway, so saving it would mostly restore a dead token.
 */
private val OnboardingStateSaver: Saver<OnboardingState, Any> = listSaver(
    save = { s ->
        when (s) {
            is OnboardingState.Welcome -> listOf("welcome")
            is OnboardingState.PhoneEntry -> listOf("phone", s.held)
            is OnboardingState.AwaitingOtp -> listOf("otp", s.phone, s.held, s.attempts)
            // Deliberately saved, session token included: this is the state whose
            // loss is worst. The token is already in memory on this device and the
            // bundle it lands in is process-private; dropping it to avoid saving a
            // secret would strand a verified user with no way forward.
            is OnboardingState.ClaimUsername -> listOf("claim", s.sessionToken, s.phone)
            is OnboardingState.SetPin -> listOf("setpin", s.sessionToken, s.handle, s.phone)
            is OnboardingState.ShowPhrase ->
                listOf("showphrase", s.sessionToken, s.handle, s.phone, s.phrase)
            is OnboardingState.RestorePhone -> listOf("restorephone")
            is OnboardingState.RestoreOtp -> listOf("restoreotp", s.phone)
            // See the KDoc: these carry crypto material, so the flow restarts at
            // the phone step rather than round-tripping salts and the blob
            // through a system bundle.
            is OnboardingState.RestorePin,
            is OnboardingState.RestorePhrase,
            is OnboardingState.RestoreNewPin,
            -> listOf("restorephone")
        }
    },
    restore = { v ->
        when (val tag = v[0] as String) {
            "welcome" -> OnboardingState.Welcome
            "phone" -> OnboardingState.PhoneEntry(v[1] as Boolean)
            "otp" -> OnboardingState.AwaitingOtp(v[1] as String, v[2] as Boolean, v[3] as Int)
            "claim" -> OnboardingState.ClaimUsername(v[1] as String, v[2] as String)
            "setpin" -> OnboardingState.SetPin(v[1] as String, v[2] as String, v[3] as String)
            "showphrase" -> OnboardingState.ShowPhrase(
                v[1] as String,
                v[2] as String,
                v[3] as String,
                v[4] as String,
            )
            "restorephone" -> OnboardingState.RestorePhone
            "restoreotp" -> OnboardingState.RestoreOtp(v[1] as String)
            else -> error("unknown OnboardingState tag: $tag")
        }
    },
)

/**
 * ET6/ET8: did the directory fail to *check* the code, as opposed to rejecting it?
 *
 * A 503 from `POST /verify` means the OTP vendor never answered, so the code was
 * never checked and the user is not at fault: they get the held treatment (digits
 * stay put, warning chip, "your code is fine"), not the error channel, which reads
 * as blame. A 400 ("code rejected") is the user's to fix and stays an error.
 *
 * ponytail: `accounts_enabled = false` also returns 503 (`ApiError::FeatureDisabled`),
 * so flipping that flag mid-flow would show "Try again" for something retrying can
 * never fix. Not worth a wire-format change today: `/signup` checks the same flag one
 * screen earlier, so a user can only reach here if the flag flipped between the two
 * calls. If that stops being true, give the API a machine-readable error code and
 * match on it here instead of on the status.
 */
internal fun isVerificationUnavailable(e: DirectoryException): Boolean = e.status == 503

/**
 * ET6/ET8: where a failed `POST /verify` leaves the flow.
 *
 * [OnboardingState.AwaitingOtp.held] is true **iff** the directory never checked
 * the code, so any response we *did* get retires the hold — the chip's "your code
 * is fine" can never outlive the 503 that earned it. Without that, an outage
 * followed by a rejected code (the likely pair: codes expire while the vendor is
 * down) renders "your code is fine" directly above "code rejected", which is the
 * false-copy class ET8 exists to delete.
 *
 * Extracted rather than inlined in the catch block so the *live* path is testable
 * without a Compose harness. [nextAfterVerify]'s gate is defense-in-depth and is
 * unreachable from a correct server, so leaving this inline meant the dead branch
 * had tests and the reachable one had none.
 */
internal fun nextAfterVerifyError(
    current: OnboardingState,
    phone: String,
    e: DirectoryException,
): OnboardingState {
    val unavailable = isVerificationUnavailable(e)
    val prior = (current as? OnboardingState.AwaitingOtp)?.attempts ?: 0
    return OnboardingState.AwaitingOtp(
        phone,
        held = unavailable,
        // ET3: counts outages, not taps — a checked answer ends the streak
        // whatever it says, because the hold is over either way.
        attempts = if (unavailable) prior + 1 else 0,
    )
}

/**
 * ET3: the held chip, escalating.
 *
 * The first outage is reassurance ("this isn't you"). A third is information the
 * user can act on: it is not clearing, so stop tapping and come back later. Left
 * flat, the screen says the same calm thing forever while the user retries into
 * a wall — and gives no reason to stop, which is the other half of ET1's
 * throttle: the server stops counting the guesses, this stops asking for them.
 */
internal fun heldChipText(attempts: Int): String =
    if (attempts >= 3) "Still can't reach verification — tried $attempts times"
    else "Can't reach verification — your code is fine"

/**
 * ET17: where a failed `POST /signup` leaves the flow.
 *
 * `/signup` calls the same vendor `/verify` does, so it owes the same treatment:
 * a 503 means we never reached verification and no code was sent — not the
 * user's fault. Which screen holds depends on where they were, and both are
 * reachable: from [OnboardingState.PhoneEntry] on first submit, and from
 * [OnboardingState.AwaitingOtp] via Resend, which ET13 keeps live during a hold
 * precisely so a user whose code expired mid-outage can ask for another.
 *
 * Any other failure returns [current] untouched — the state is fine, the error
 * channel explains it.
 *
 * **Issue #3's restore path shares [sendCode] and is deliberately excluded.**
 * Its states carry no `held` flag, and — more importantly — falling through to
 * the `PhoneEntry` default would answer "we couldn't send your restore code" by
 * moving the user into *signup*. Losing the flow you asked for is a worse
 * outcome than the outage that caused it, so a restore holds its own screen and
 * takes the error channel instead of the chip.
 */
internal fun nextAfterSendError(
    current: OnboardingState,
    phone: String,
    e: DirectoryException,
): OnboardingState = when {
    !isVerificationUnavailable(e) -> current
    current is OnboardingState.AwaitingOtp -> OnboardingState.AwaitingOtp(phone, held = true)
    current is OnboardingState.RestorePhone || current is OnboardingState.RestoreOtp -> current
    else -> OnboardingState.PhoneEntry(held = true)
}

/**
 * [enroll] runs the recovery enrollment (engine `backupEnroll` + bundle
 * upload) and returns the 5-word phrase; it gets the session token and
 * claimed handle because it builds the persistent engine (issue #1) before
 * the first backup upload. Null skips the PIN/phrase steps (tests only —
 * App.kt always passes it; issue #2 acceptance 1 makes the steps mandatory).
 */
@Composable
fun OnboardingFlow(
    client: DirectoryClient,
    enroll: (suspend (sessionToken: String, handle: String, pin: String) -> String)? = null,
    /**
     * Issue #3: finishes a restore (engine + session). Null keeps the
     * Welcome CTA on its "not available yet" stub (tests only).
     */
    restore: (suspend (RestoreRequest) -> Unit)? = null,
    onComplete: (sessionToken: String, handle: String, phone: String) -> Unit,
) {
    val palette = LocalChatPalette.current
    // ET11: the step survives rotation; `error` and `loading` deliberately do
    // not. Both describe a request that died with the old Activity — restoring
    // a spinner for a coroutine nobody is waiting on, or an error about a call
    // that is no longer in flight, would be a lie the user can't dismiss.
    var state by rememberSaveable(stateSaver = OnboardingStateSaver) {
        mutableStateOf<OnboardingState>(OnboardingState.Welcome)
    }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    // ET10: the request the current step started, so leaving can cancel it.
    var inFlight by remember { mutableStateOf<Job?>(null) }

    /**
     * ET10: every transition cancels whatever the step you're leaving started.
     *
     * `rememberCoroutineScope()` is scoped to `OnboardingFlow`, not to the step,
     * so a launched request outlived the screen that started it: tap Verify, tap
     * "Change number", and the resolved call wrote `ClaimUsername` with the
     * captured (now abandoned) phone — teleporting the user forward and
     * persisting a number they had just backed out of.
     *
     * Cancelling rather than gating `BackRow` on `loading`, which is what ET10
     * literally asked for: the vendor call can take the full 10s timeout, and a
     * back affordance that stops working for ten seconds is the dead end
     * BackRow was added to remove. Cancellation makes the escape both always
     * available and safe, which the gate alone would not — a request already in
     * flight when the gate closed would still land.
     *
     * Clears the error for the same reason it always did: a message about the
     * step you left ("Restore isn't available yet") should not glow under the
     * step you're on.
     */
    fun goTo(next: OnboardingState) {
        inFlight?.cancel()
        inFlight = null
        loading = false
        error = null
        state = next
    }

    fun sendCode(rawPhone: String, next: (phone: String) -> OnboardingState) {
        loading = true; error = null
        val phone = normalizePhoneInput(rawPhone)
        inFlight = scope.launch {
            try {
                client.signup(phone)
                // A code really was sent, so any prior hold is over: every
                // caller's [next] builds a fresh state, defaulting `held` false.
                state = next(phone)
            } catch (e: DirectoryException) {
                // ET17: same rule as /verify — a 503 is our vendor failing, not the
                // user, so the chip carries it and the error channel (which reads as
                // blame) stays empty.
                state = nextAfterSendError(state, phone, e)
                // Keyed on whether the *resulting* state can actually show a hold,
                // not on the status alone: the restore path has no chip, so blanking
                // its error would leave a 503 with no state change and no message —
                // a tap that visibly does nothing.
                val heldByState = (state as? OnboardingState.PhoneEntry)?.held == true ||
                    (state as? OnboardingState.AwaitingOtp)?.held == true
                error = if (heldByState) null else e.message
            } finally {
                // ET10: only the request that still owns the screen may clear the
                // spinner. A cancelled one doesn't — `goTo` already reset it, and a
                // newer request may have set it again.
                if (isActive) loading = false
            }
        }
    }

    /**
     * Issue #3: derive the auth proof (Argon2, seconds) and redeem it for
     * the bundle — off the UI thread; [then] runs there too, so it can do
     * the equally heavy `newFromBackup`.
     */
    fun fetchBundle(begin: RestoreBegin, secret: String, method: String, then: suspend (BackupBundle) -> Unit) {
        loading = true; error = null
        scope.launch {
            try {
                withContext(Dispatchers.Default) {
                    val salt = if (method == "pin") begin.saltA else begin.saltPa
                    val bundle = client.restoreComplete(
                        begin.restoreToken, method, backupAuthProof(secret, salt),
                    )
                    then(bundle)
                }
            } catch (e: Exception) {
                // DirectoryException carries the server's message verbatim
                // (remaining attempts, lockout wait) — show it as-is.
                error = e.message ?: "Restore failed."
            } finally {
                loading = false
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize().background(palette.bg).padding(horizontal = 24.dp, vertical = 28.dp)) {
        Box(Modifier.weight(1f)) {
            when (val s = state) {
                is OnboardingState.Welcome -> WelcomeStep(
                    onContinue = { goTo(OnboardingState.PhoneEntry()) },
                    onRestore = {
                        // ET10: `goTo` rather than a bare assignment — it cancels
                        // whatever the last step left in flight and clears `error`.
                        if (restore == null) {
                            error = "Restore isn't available yet."
                        } else {
                            goTo(OnboardingState.RestorePhone)
                        }
                    },
                )
                is OnboardingState.PhoneEntry -> PhoneEntryStep(
                    loading = loading,
                    held = s.held,
                    onBack = { goTo(OnboardingState.Welcome) },
                    onSubmit = { raw -> sendCode(raw) { OnboardingState.AwaitingOtp(it) } },
                )
                is OnboardingState.AwaitingOtp -> OtpStep(
                    phone = s.phone,
                    loading = loading,
                    held = s.held,
                    attempts = s.attempts,
                    onBack = { goTo(OnboardingState.PhoneEntry()) },
                    onResend = { sendCode(s.phone) { OnboardingState.AwaitingOtp(it) } },
                    // ET3: a new answer is being proposed, so the hold no longer
                    // describes it — the chip says "your code is fine" about a code
                    // that no longer exists. Only fires when something is actually
                    // held, so ordinary typing doesn't churn state.
                    onCodeEdited = { if (s.held) state = s.copy(held = false) },
                ) { code ->
                    loading = true; error = null
                    inFlight = scope.launch {
                        try {
                            // T27: `verified`, not just `sessionToken` — see nextAfterVerify.
                            state = nextAfterVerify(s.phone, client.verify(s.phone, code))
                        } catch (e: DirectoryException) {
                            // ET6/ET8: a vendor outage is a 503 and arrives here, not as
                            // `verified == false`. It is not the user's fault, so it holds
                            // rather than blaming them through the error channel.
                            state = nextAfterVerifyError(state, s.phone, e)
                            // The held chip explains a 503 on its own; anything else is a
                            // real answer the user needs to read.
                            error = if (isVerificationUnavailable(e)) null else e.message
                        } finally {
                            if (isActive) loading = false
                        }
                    }
                }
                is OnboardingState.ClaimUsername -> UsernameStep(loading) { nickname ->
                    loading = true; error = null
                    scope.launch {
                        try {
                            val handle = client.claimUsername(s.sessionToken, nickname)
                            if (enroll == null) {
                                onComplete(s.sessionToken, handle, s.phone)
                            } else {
                                state = OnboardingState.SetPin(s.sessionToken, handle, s.phone)
                            }
                        } catch (e: DirectoryException) {
                            error = e.message
                        } finally {
                            loading = false
                        }
                    }
                }
                is OnboardingState.SetPin -> PinSetStep(loading) { pin ->
                    loading = true; error = null
                    scope.launch {
                        try {
                            val phrase = checkNotNull(enroll)(s.sessionToken, s.handle, pin)
                            state = OnboardingState.ShowPhrase(s.sessionToken, s.handle, s.phone, phrase)
                        } catch (e: Exception) {
                            // DirectoryException or the engine's own error —
                            // either way the step stays put and says why.
                            error = e.message ?: "Couldn't set up recovery."
                        } finally {
                            loading = false
                        }
                    }
                }
                is OnboardingState.ShowPhrase -> RecoveryPhraseStep(s.phrase) {
                    onComplete(s.sessionToken, s.handle, s.phone)
                }
                // held/attempts are signup's ET3/ET17 outage treatment; the restore
                // path surfaces an outage through `error` instead (see
                // [nextAfterSendError]), so it opts out rather than faking a chip.
                is OnboardingState.RestorePhone -> PhoneEntryStep(
                    loading = loading,
                    held = false,
                    onBack = { goTo(OnboardingState.Welcome) },
                    onSubmit = { raw -> sendCode(raw) { OnboardingState.RestoreOtp(it) } },
                )
                is OnboardingState.RestoreOtp -> OtpStep(
                    phone = s.phone,
                    loading = loading,
                    held = false,
                    attempts = 0,
                    onBack = { goTo(OnboardingState.RestorePhone) },
                    onResend = { sendCode(s.phone) { OnboardingState.RestoreOtp(it) } },
                    onCodeEdited = {},
                ) { code ->
                    loading = true; error = null
                    scope.launch {
                        try {
                            val begin = client.restoreBegin(s.phone, code)
                            state = OnboardingState.RestorePin(begin, s.phone)
                        } catch (e: DirectoryException) {
                            error = e.message
                        } finally {
                            loading = false
                        }
                    }
                }
                is OnboardingState.RestorePin -> RestorePinStep(
                    loading = loading,
                    onForgot = { state = OnboardingState.RestorePhrase(s.begin, s.phone) },
                ) { pin ->
                    fetchBundle(s.begin, pin, "pin") { bundle ->
                        checkNotNull(restore)(
                            RestoreRequest(bundle, pin, null, s.begin.sessionToken, s.phone),
                        )
                    }
                }
                is OnboardingState.RestorePhrase -> RestorePhraseStep(loading) { phrase ->
                    fetchBundle(s.begin, phrase, "phrase") { bundle ->
                        state = OnboardingState.RestoreNewPin(s.begin, s.phone, bundle, phrase)
                    }
                }
                is OnboardingState.RestoreNewPin -> PinSetStep(loading) { newPin ->
                    loading = true; error = null
                    scope.launch {
                        try {
                            withContext(Dispatchers.Default) {
                                checkNotNull(restore)(
                                    RestoreRequest(s.bundle, s.phrase, newPin, s.begin.sessionToken, s.phone),
                                )
                            }
                        } catch (e: Exception) {
                            error = e.message ?: "Restore failed."
                        } finally {
                            loading = false
                        }
                    }
                }
            }
        }
        error?.let {
            Spacer(Modifier.height(12.dp))
            Text(
                it,
                color = palette.error,
                style = MaterialTheme.typography.labelMedium,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** Centered statement headline + muted explanation, per the onboarding rhythm. */
@Composable
private fun StepHeader(headline: String, body: String) {
    val palette = LocalChatPalette.current
    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
        Text(
            headline,
            style = MaterialTheme.typography.headlineSmall,
            color = palette.ink,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            body,
            style = MaterialTheme.typography.bodyLarge,
            color = palette.muted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 6.dp),
        )
    }
}

@Composable
private fun WelcomeStep(onContinue: () -> Unit, onRestore: () -> Unit) {
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.weight(1f))
        Rosette(seed = "quiet-room-welcome", size = 116.dp)
        Spacer(Modifier.height(30.dp))
        StepHeader(
            headline = "Speak freely.\nNo one else can read it.",
            body = "Every message is end-to-end encrypted. Relays only ever see ciphertext — not names, not content, not who talks to whom.",
        )
        Spacer(Modifier.weight(1f))
        InstrumentButton("Get started", onClick = onContinue)
        Spacer(Modifier.height(12.dp))
        InstrumentButton("Restore account", onClick = onRestore, primary = false)
    }
}

@Composable
private fun PhoneEntryStep(
    loading: Boolean,
    held: Boolean,
    onBack: () -> Unit,
    onSubmit: (phone: String) -> Unit,
) {
    // ET11: rotation must not empty the field the user is typing into.
    var countryCode by rememberSaveable { mutableStateOf("") }
    var number by rememberSaveable { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        BackRow("Back", onBack)
        StepHeader(
            headline = "Your phone number",
            body = "Used once to prove you're a person, then hashed. It's hidden by default and never shown to anyone.",
        )
        Spacer(Modifier.height(28.dp))
        InstrumentPhoneField(
            countryCode = countryCode,
            onCountryCodeChange = { countryCode = it },
            number = number,
            onNumberChange = { number = it },
        )
        if (held) {
            Spacer(Modifier.height(20.dp))
            // ET17: `/signup` couldn't reach the vendor, so no code was sent. Same
            // treatment as the OTP step's hold, and for the same reason — it is our
            // outage, not their mistake. Deliberately claims nothing about what was
            // stored: `POST /signup` only reaches the vendor *after* it has written
            // the peppered hash (api.rs), so "nothing was saved" would be false here
            // exactly as it was on the OTP step (ET8). The number stays in the field
            // so retrying costs one tap.
            InstrumentStatusChip("Can't reach verification — try again in a moment", tone = StatusTone.Warning)
        }
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Sending…" else if (held) "Try again" else "Next",
            onClick = { onSubmit((countryCode.ifBlank { "+420" }) + number) },
            enabled = !loading && number.isNotBlank(),
            loading = loading,
        )
    }
}

private const val OTP_LENGTH = 6

@Composable
private fun OtpStep(
    phone: String,
    loading: Boolean,
    held: Boolean,
    attempts: Int,
    onBack: () -> Unit,
    onResend: () -> Unit,
    onCodeEdited: () -> Unit,
    onSubmit: (code: String) -> Unit,
) {
    val palette = LocalChatPalette.current
    var code by rememberSaveable { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        BackRow("Change number", onBack)
        StepHeader(
            headline = "Enter the code",
            body = "We sent a $OTP_LENGTH-digit code to $phone.",
        )
        Spacer(Modifier.height(28.dp))
        // Held: the digits stay put and dim — they were accepted-looking, we
        // just can't confirm them. Clearing them would imply they were wrong.
        OtpCells(
            code = code,
            onChange = {
                code = it
                onCodeEdited()
            },
            dimmed = held,
        )
        if (held) {
            Spacer(Modifier.height(20.dp))
            // ET8 (founder decision, 2026-07-16): no reassurance claim here. The line
            // that used to sit here ("Your number isn't registered until we confirm
            // it, so nothing has been saved") was false on every count: `POST /signup`
            // durably wrote a peppered Argon2id hash one screen ago (store.rs
            // `create_pending_user`). The honest rewrite was rejected too, for two
            // reasons: it cannot promise erasure (that needs a session, and ET6
            // deliberately mints none here), and it answers "should I hand over my
            // number" one screen after the number was handed over. If that promise is
            // worth making, it belongs on PhoneEntry, where the user can still act on
            // it. Do not restore a claim here without tracing every clause to a line
            // in directory/src/{api.rs,store.rs}.
            InstrumentStatusChip(heldChipText(attempts), tone = StatusTone.Warning)
        }
        // ET13 (founder decision, 2026-07-16): resend is an always-live link, so the
        // chip sits *above* it rather than replacing it. The chip is a status line,
        // not a control — swapping the two made a vendor outage hide the one action
        // that helps, because OTP codes expire on the vendor's clock while we are
        // down. "Try again" then resubmits a dying code and the only route to a fresh
        // one was a button labelled "Use a different number".
        Spacer(Modifier.height(if (held) 12.dp else 20.dp))
        Text(
            "Resend code",
            style = MaterialTheme.typography.labelLarge,
            color = palette.accent,
            modifier = Modifier.clickable(enabled = !loading, onClick = onResend).padding(4.dp),
        )
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Verifying…" else if (held) "Try again" else "Verify",
            onClick = { onSubmit(code) },
            enabled = !loading && code.length == OTP_LENGTH,
            loading = loading,
        )
        if (held) {
            Spacer(Modifier.height(12.dp))
            InstrumentButton("Use a different number", onClick = onBack, primary = false)
        }
    }
}

/** Back affordance for every post-Welcome step — without it, a mistyped number is a dead end. */
@Composable
private fun BackRow(label: String, onBack: () -> Unit) {
    val palette = LocalChatPalette.current
    Row(modifier = Modifier.fillMaxWidth().height(34.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(
            // ponytail: text glyph matches the rest of the app's placeholder icons;
            // swaps for Lucide chevron-left with the others (see TODO 13).
            "‹  $label",
            style = MaterialTheme.typography.labelMedium,
            color = palette.muted,
            modifier = Modifier.clickable(onClick = onBack).padding(vertical = 6.dp, horizontal = 4.dp),
        )
    }
}

/**
 * Six mono cells (DESIGN.md: OTP digits are a crypto fact → Plex Mono,
 * 12dp cells, active cell ringed in accent). One invisible text field
 * overlays the row and owns focus/IME.
 */
@Composable
private fun OtpCells(code: String, onChange: (String) -> Unit, dimmed: Boolean = false) {
    val palette = LocalChatPalette.current
    Box {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            repeat(OTP_LENGTH) { i ->
                val active = i == code.length && !dimmed
                Box(
                    modifier = Modifier
                        .width(40.dp)
                        .height(50.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(palette.surface)
                        .then(if (active) Modifier.border(2.dp, palette.accent, RoundedCornerShape(12.dp)) else Modifier),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        code.getOrNull(i)?.toString() ?: "",
                        style = ChatMonoStyle.copy(fontSize = 20.sp),
                        color = if (dimmed) palette.muted else palette.ink,
                    )
                }
            }
        }
        BasicTextField(
            value = code,
            onValueChange = { onChange(it.filter(Char::isDigit).take(OTP_LENGTH)) },
            singleLine = true,
            textStyle = TextStyle(color = Color.Transparent, fontSize = 1.sp),
            cursorBrush = SolidColor(Color.Transparent),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
            modifier = Modifier.matchParentSize(),
        )
    }
}

@Composable
private fun UsernameStep(loading: Boolean, onSubmit: (nickname: String) -> Unit) {
    var nickname by rememberSaveable { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        // ET5: the one post-Welcome step with no `BackRow`, deliberately — the
        // asymmetry is the point, not an oversight.
        //
        // By here `/verify` has already minted a real session and the account
        // exists, verified, server-side. "Back" would return to a PhoneEntry
        // whose only action is `POST /signup`, which sends a second SMS for a
        // number that is already verified — and would strand the live token this
        // step is holding, since nothing else in the flow can reach it again.
        // There is nothing to go back *to*: the irreversible thing already
        // happened one screen ago.
        //
        // The real escape from a wrong number is `DELETE /account`, which needs
        // this token and belongs in settings, not in a back button. If this ever
        // grows a back affordance it has to erase the account first — otherwise
        // it silently abandons a verified account holding the user's number.
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = "Claim your username",
            body = "How people find you — if you let them. Search is off by default; you can turn it on later.",
        )
        Spacer(Modifier.height(28.dp))
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Rosette(seed = nickname.ifBlank { "…" }, size = 44.dp)
            Spacer(Modifier.width(16.dp))
            InstrumentField(
                value = nickname,
                onValueChange = { nickname = it },
                placeholder = "mira",
                modifier = Modifier.weight(1f),
            )
        }
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Claiming…" else "Continue",
            onClick = { onSubmit(nickname) },
            enabled = !loading && nickname.isNotBlank(),
            loading = loading,
        )
    }
}

private val PIN_RANGE = 4..6

/**
 * Issue #2: mandatory recovery PIN — enter then confirm, 4-6 digits, mono
 * cells like the OTP (a crypto fact per DESIGN.md). The copy says what the
 * PIN is FOR: account recovery, not an app lock.
 */
@Composable
private fun PinSetStep(loading: Boolean, onSubmit: (pin: String) -> Unit) {
    val palette = LocalChatPalette.current
    var pin by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    var confirming by remember { mutableStateOf(false) }
    var mismatch by remember { mutableStateOf(false) }
    val current = if (confirming) confirm else pin
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = if (confirming) "Confirm your PIN" else "Set your PIN",
            body = "4-6 digits. It's how you recover your account on a new phone — not an app lock.",
        )
        Spacer(Modifier.height(28.dp))
        OtpCells(
            code = current,
            onChange = {
                if (confirming) confirm = it else pin = it
                mismatch = false
            },
        )
        if (mismatch) {
            Spacer(Modifier.height(12.dp))
            Text(
                "PINs don't match — try again.",
                color = palette.error,
                style = MaterialTheme.typography.labelMedium,
            )
        }
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = when {
                loading -> "Securing…"
                confirming -> "Confirm PIN"
                else -> "Continue"
            },
            onClick = {
                when {
                    !confirming -> confirming = true
                    confirm == pin -> onSubmit(pin)
                    else -> { mismatch = true; confirm = "" }
                }
            },
            enabled = !loading && current.length in PIN_RANGE,
            loading = loading,
        )
    }
}

/**
 * Issue #3: PIN entry during restore. Failures show the server's message
 * verbatim (remaining attempts, lockout wait) via the flow-level error
 * line; the phrase fallback link is always available.
 */
@Composable
private fun RestorePinStep(loading: Boolean, onForgot: () -> Unit, onSubmit: (pin: String) -> Unit) {
    val palette = LocalChatPalette.current
    var pin by remember { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = "Enter your PIN",
            body = "The 4-6 digit PIN you set when you created your account.",
        )
        Spacer(Modifier.height(28.dp))
        OtpCells(code = pin, onChange = { pin = it })
        Spacer(Modifier.height(20.dp))
        Text(
            "Forgot PIN? Use recovery phrase",
            style = MaterialTheme.typography.labelLarge,
            color = palette.accent,
            modifier = Modifier.clickable(enabled = !loading, onClick = onForgot).padding(4.dp),
        )
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Checking…" else "Restore",
            onClick = { onSubmit(pin) },
            enabled = !loading && pin.length in PIN_RANGE,
            loading = loading,
        )
    }
}

/**
 * Issue #3: the phrase fallback — one lowercase field, five words,
 * normalized before derivation (see engine `backupAuthProof`).
 */
@Composable
private fun RestorePhraseStep(loading: Boolean, onSubmit: (phrase: String) -> Unit) {
    var phrase by remember { mutableStateOf("") }
    val wordCount = phrase.trim().split(Regex("\\s+")).count { it.isNotBlank() }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = "Your recovery phrase",
            body = "The five words you wrote down. Order matters; capitals and extra spaces don't.",
        )
        Spacer(Modifier.height(28.dp))
        InstrumentField(
            value = phrase,
            onValueChange = { phrase = it },
            placeholder = "five words with spaces",
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Checking…" else "Restore",
            onClick = { onSubmit(phrase) },
            enabled = !loading && wordCount == 5,
            loading = loading,
        )
    }
}

/**
 * Issue #2: the 5 words that recover the PIN, on a tap-to-reveal 16dp card
 * (DESIGN.md), shown exactly once. A single checkbox gates the CTA — no
 * forced re-entry.
 */
@Composable
private fun RecoveryPhraseStep(phrase: String, onDone: () -> Unit) {
    val palette = LocalChatPalette.current
    var revealed by remember { mutableStateOf(false) }
    var written by remember { mutableStateOf(false) }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = "Your recovery phrase",
            body = "If you forget your PIN, these five words are the only way to get back in. Write them down.",
        )
        Spacer(Modifier.height(28.dp))
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(palette.surface)
                .clickable(enabled = !revealed) { revealed = true }
                .padding(vertical = 24.dp),
            contentAlignment = Alignment.Center,
        ) {
            if (!revealed) {
                Text("Tap to reveal", style = MaterialTheme.typography.labelLarge, color = palette.muted)
            } else {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    phrase.split(" ").forEach { word ->
                        Text(word, style = ChatMonoStyle.copy(fontSize = 20.sp), color = palette.ink)
                    }
                }
            }
        }
        Spacer(Modifier.height(20.dp))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.clickable(enabled = revealed) { written = !written },
        ) {
            Checkbox(checked = written, onCheckedChange = { written = it }, enabled = revealed)
            Text(
                "I wrote them down",
                style = MaterialTheme.typography.bodyLarge,
                color = if (revealed) palette.ink else palette.muted,
            )
        }
        Spacer(Modifier.weight(1f))
        InstrumentButton("Continue", onClick = onDone, enabled = written)
    }
}
