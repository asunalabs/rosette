package chat.app.settings

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
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
import chat.app.session.Session
import chat.app.theme.ChatMonoStyle
import chat.app.theme.HairlineDivider
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentField
import chat.app.theme.LocalChatPalette
import chat.engine.ChatEngine
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class Stage { VerifyPin, VerifyPhrase, NewPin, RetryUpload, Done }

/**
 * Issue #4: Change PIN. Gate with the current PIN (verified locally against
 * the kv bundle — no server call, no attempt counting) or the recovery
 * phrase, then set a new PIN twice, rewrap, and re-upload. An upload
 * failure keeps a retry state — the local kv is already rewrapped, so
 * silence would mean divergence.
 */
@Composable
fun ChangePinScreen(engine: ChatEngine, client: DirectoryClient, session: Session, onBack: () -> Unit) {
    val palette = LocalChatPalette.current
    var stage by remember { mutableStateOf(Stage.VerifyPin) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val enrolled = remember {
        runCatching { engine.backupBundleCurrent() != null }.getOrDefault(false)
    }

    fun verify(secret: String, wrongMessage: String) {
        loading = true; error = null
        scope.launch {
            val ok = try {
                withContext(Dispatchers.Default) { engine.backupVerifySecret(secret) }
            } catch (e: Exception) {
                error = e.message; false
            }
            loading = false
            if (ok) stage = Stage.NewPin else if (error == null) error = wrongMessage
        }
    }

    fun applyNewPin(newPin: String) {
        loading = true; error = null
        scope.launch {
            try {
                val bundle = withContext(Dispatchers.Default) { engine.backupRewrapPin(newPin) }
                try {
                    client.putBackup(session.sessionToken, bundle)
                    stage = Stage.Done
                } catch (_: DirectoryException) {
                    // Local kv is already rewrapped; only the upload lagged.
                    stage = Stage.RetryUpload
                }
            } catch (e: Exception) {
                error = e.message ?: "Couldn't change the PIN."
            } finally {
                loading = false
            }
        }
    }

    fun retryUpload() {
        loading = true; error = null
        scope.launch {
            try {
                val bundle = withContext(Dispatchers.Default) { engine.backupBundleCurrent() }
                if (bundle != null) {
                    client.putBackup(session.sessionToken, bundle)
                    stage = Stage.Done
                }
            } catch (e: DirectoryException) {
                error = e.message
            } finally {
                loading = false
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize().background(palette.bg)) {
        Row(
            modifier = Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.clickable(onClick = onBack).padding(8.dp)) {
                Text("←", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
            }
            Spacer(Modifier.width(4.dp))
            Text("Change PIN", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
        }
        HairlineDivider()

        Column(
            modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp, vertical = 28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (!enrolled) {
                Spacer(Modifier.weight(1f))
                Text(
                    "Recovery isn't set up on this device yet.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = palette.muted,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.weight(1f))
            } else when (stage) {
                Stage.VerifyPin -> {
                    var pin by remember { mutableStateOf("") }
                    StageHeader("Enter your current PIN", "Proves it's you before anything changes.")
                    Spacer(Modifier.height(28.dp))
                    PinDigits(pin) { pin = it; error = null }
                    Spacer(Modifier.height(20.dp))
                    Text(
                        "Forgot PIN? Use recovery phrase",
                        style = MaterialTheme.typography.labelLarge,
                        color = palette.accent,
                        modifier = Modifier
                            .clickable(enabled = !loading) { stage = Stage.VerifyPhrase; error = null }
                            .padding(4.dp),
                    )
                    Spacer(Modifier.weight(1f))
                    InstrumentButton(
                        text = if (loading) "Checking…" else "Continue",
                        onClick = { verify(pin, "Wrong PIN.") },
                        enabled = !loading && pin.length in 4..6,
                        loading = loading,
                    )
                }
                Stage.VerifyPhrase -> {
                    var phrase by remember { mutableStateOf("") }
                    StageHeader(
                        "Enter your recovery phrase",
                        "The five words you wrote down. Capitals and extra spaces don't matter.",
                    )
                    Spacer(Modifier.height(28.dp))
                    InstrumentField(
                        value = phrase,
                        onValueChange = { phrase = it; error = null },
                        placeholder = "five words with spaces",
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.weight(1f))
                    InstrumentButton(
                        text = if (loading) "Checking…" else "Continue",
                        onClick = { verify(phrase, "That phrase doesn't match.") },
                        enabled = !loading && phrase.isNotBlank(),
                        loading = loading,
                    )
                }
                Stage.NewPin -> {
                    var pin by remember { mutableStateOf("") }
                    var confirm by remember { mutableStateOf("") }
                    var confirming by remember { mutableStateOf(false) }
                    val current = if (confirming) confirm else pin
                    StageHeader(
                        if (confirming) "Confirm your new PIN" else "Set your new PIN",
                        "4-6 digits. Your recovery phrase stays the same.",
                    )
                    Spacer(Modifier.height(28.dp))
                    PinDigits(current) {
                        if (confirming) confirm = it else pin = it
                        error = null
                    }
                    Spacer(Modifier.weight(1f))
                    InstrumentButton(
                        text = when {
                            loading -> "Securing…"
                            confirming -> "Change PIN"
                            else -> "Continue"
                        },
                        onClick = {
                            when {
                                !confirming -> confirming = true
                                confirm == pin -> applyNewPin(pin)
                                else -> { error = "PINs don't match — try again."; confirm = "" }
                            }
                        },
                        enabled = !loading && current.length in 4..6,
                        loading = loading,
                    )
                }
                Stage.RetryUpload -> {
                    Spacer(Modifier.weight(1f))
                    Text(
                        "Your PIN changed on this device, but the backup upload failed. Retry so the new PIN works for restore.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = palette.ink,
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.weight(1f))
                    InstrumentButton(
                        text = if (loading) "Uploading…" else "Retry upload",
                        onClick = ::retryUpload,
                        enabled = !loading,
                        loading = loading,
                    )
                }
                Stage.Done -> {
                    Spacer(Modifier.weight(1f))
                    Text(
                        "PIN changed.",
                        style = MaterialTheme.typography.headlineSmall,
                        color = palette.ink,
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.weight(1f))
                    InstrumentButton("Done", onClick = onBack)
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
}

@Composable
private fun StageHeader(headline: String, body: String) {
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
        )
    }
}

// ponytail: near-copy of Onboarding's private OtpCells — duplicated to keep
// this branch off Onboarding.kt's hot regions while the country-picker work
// (#5) rides there. Extract to theme/ when a third caller appears.
@Composable
private fun PinDigits(code: String, onChange: (String) -> Unit) {
    val palette = LocalChatPalette.current
    Box {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            repeat(6) { i ->
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
            onValueChange = { onChange(it.filter(Char::isDigit).take(6)) },
            singleLine = true,
            textStyle = TextStyle(color = Color.Transparent, fontSize = 1.sp),
            cursorBrush = SolidColor(Color.Transparent),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
            modifier = Modifier.matchParentSize(),
        )
    }
}
