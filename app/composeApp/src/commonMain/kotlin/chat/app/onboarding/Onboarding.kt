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
import chat.app.directory.normalizePhoneInput
import chat.app.theme.ChatMonoStyle
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentField
import chat.app.theme.InstrumentPhoneField
import chat.app.theme.LocalChatPalette
import chat.app.theme.Rosette
import kotlinx.coroutines.launch

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
    data object PhoneEntry : OnboardingState
    data class AwaitingOtp(val phone: String) : OnboardingState
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
}

/**
 * [enroll] runs the recovery enrollment (engine `backupEnroll` + bundle
 * upload) and returns the 5-word phrase. Null skips the PIN/phrase steps —
 * only until #1's persistent engine lands in App.kt; they are mandatory
 * once it does (issue #2 acceptance 1).
 */
@Composable
fun OnboardingFlow(
    client: DirectoryClient,
    enroll: (suspend (pin: String) -> String)? = null,
    onComplete: (sessionToken: String, handle: String, phone: String) -> Unit,
) {
    val palette = LocalChatPalette.current
    var state by remember { mutableStateOf<OnboardingState>(OnboardingState.Welcome) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun sendCode(rawPhone: String) {
        loading = true; error = null
        val phone = normalizePhoneInput(rawPhone)
        scope.launch {
            try {
                client.signup(phone)
                state = OnboardingState.AwaitingOtp(phone)
            } catch (e: DirectoryException) {
                error = e.message
            } finally {
                loading = false
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize().background(palette.bg).padding(horizontal = 24.dp, vertical = 28.dp)) {
        Box(Modifier.weight(1f)) {
            when (val s = state) {
                is OnboardingState.Welcome -> WelcomeStep(
                    onContinue = { state = OnboardingState.PhoneEntry },
                    onRestore = { error = "Restore isn't available yet." },
                )
                is OnboardingState.PhoneEntry -> PhoneEntryStep(loading, ::sendCode)
                is OnboardingState.AwaitingOtp -> OtpStep(
                    phone = s.phone,
                    loading = loading,
                    onResend = { sendCode(s.phone) },
                ) { code ->
                    loading = true; error = null
                    scope.launch {
                        try {
                            val result = client.verify(s.phone, code)
                            state = OnboardingState.ClaimUsername(result.sessionToken, s.phone)
                        } catch (e: DirectoryException) {
                            error = e.message
                        } finally {
                            loading = false
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
                            val phrase = checkNotNull(enroll)(pin)
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
private fun PhoneEntryStep(loading: Boolean, onSubmit: (phone: String) -> Unit) {
    var countryCode by remember { mutableStateOf("") }
    var number by remember { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
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
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Sending…" else "Next",
            onClick = { onSubmit((countryCode.ifBlank { "+420" }) + number) },
            enabled = !loading && number.isNotBlank(),
            loading = loading,
        )
    }
}

private const val OTP_LENGTH = 6

@Composable
private fun OtpStep(phone: String, loading: Boolean, onResend: () -> Unit, onSubmit: (code: String) -> Unit) {
    val palette = LocalChatPalette.current
    var code by remember { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(34.dp))
        StepHeader(
            headline = "Enter the code",
            body = "We sent a $OTP_LENGTH-digit code to $phone.",
        )
        Spacer(Modifier.height(28.dp))
        OtpCells(code = code, onChange = { code = it })
        Spacer(Modifier.height(20.dp))
        Text(
            "Resend code",
            style = MaterialTheme.typography.labelLarge,
            color = palette.accent,
            modifier = Modifier.clickable(enabled = !loading, onClick = onResend).padding(4.dp),
        )
        Spacer(Modifier.weight(1f))
        InstrumentButton(
            text = if (loading) "Verifying…" else "Verify",
            onClick = { onSubmit(code) },
            enabled = !loading && code.length == OTP_LENGTH,
            loading = loading,
        )
    }
}

/**
 * Six mono cells (DESIGN.md: OTP digits are a crypto fact → Plex Mono,
 * 12dp cells, active cell ringed in accent). One invisible text field
 * overlays the row and owns focus/IME.
 */
@Composable
private fun OtpCells(code: String, onChange: (String) -> Unit) {
    val palette = LocalChatPalette.current
    Box {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            repeat(OTP_LENGTH) { i ->
                val active = i == code.length
                Box(
                    modifier = Modifier
                        .width(40.dp)
                        .height(50.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(palette.surface)
                        .then(if (active) Modifier.border(2.dp, palette.accent, RoundedCornerShape(12.dp)) else Modifier),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(code.getOrNull(i)?.toString() ?: "", style = ChatMonoStyle.copy(fontSize = 20.sp), color = palette.ink)
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
    var nickname by remember { mutableStateOf("") }
    Column(modifier = Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
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
