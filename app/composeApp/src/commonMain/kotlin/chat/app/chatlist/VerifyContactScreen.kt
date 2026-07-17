package chat.app.chatlist

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.app.theme.ChatMonoStyle
import chat.app.theme.HairlineDivider
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentStatusChip
import chat.app.theme.LocalChatPalette
import chat.app.theme.Rosette
import chat.app.theme.StatusTone
import chat.engine.ChatEngine
import chat.engine.Conversation
import chat.engine.EngineException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * DT6 (D4): the verify-contact ceremony. Shows the safety number both peers
 * read off `engine.securityCode` — bound to the MLS signature keys, so an
 * active MITM shows different digits here on each device. The user compares it
 * out of band, then marks the contact verified (which lights the accent ✓ and
 * the Rosette's verified band everywhere the contact appears).
 */
@Composable
fun VerifyContactScreen(
    engine: ChatEngine,
    conversation: Conversation,
    verified: Boolean,
    onBack: () -> Unit,
    onMarkVerified: () -> Unit,
) {
    val palette = LocalChatPalette.current
    var code by remember(conversation.id) { mutableStateOf<String?>(null) }
    var loadFailed by remember(conversation.id) { mutableStateOf(false) }
    LaunchedEffect(conversation.id) {
        try {
            code = withContext(Dispatchers.Default) { engine.securityCode(conversation.id) }
        } catch (_: EngineException) {
            // Not paired yet, or the engine isn't live — no number to compare.
            loadFailed = true
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
            Text("Verify contact", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
        }
        HairlineDivider()

        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.height(16.dp))
            Rosette(seed = conversation.id, verified = verified, size = 64.dp)
            Spacer(Modifier.height(16.dp))
            Text(
                conversation.displayName,
                style = MaterialTheme.typography.headlineSmall,
                color = palette.ink,
            )
            Spacer(Modifier.height(16.dp))
            Text(
                "Compare these numbers with ${conversation.displayName} in person, or over another channel you trust. " +
                    "If they match on both devices, this conversation is end-to-end secure and no one is in the middle.",
                style = MaterialTheme.typography.bodyMedium,
                color = palette.muted,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.height(28.dp))
            when {
                code != null -> Text(
                    code!!,
                    style = ChatMonoStyle.copy(fontSize = 20.sp),
                    color = palette.ink,
                    textAlign = TextAlign.Center,
                )
                loadFailed -> Text(
                    "Couldn't read the safety number — this contact may not be fully paired yet.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.muted,
                    textAlign = TextAlign.Center,
                )
                else -> Text("…", style = MaterialTheme.typography.headlineSmall, color = palette.muted)
            }
            Spacer(Modifier.weight(1f))
            if (verified) {
                InstrumentStatusChip("Verified", tone = StatusTone.Positive)
            } else {
                InstrumentButton(
                    text = "Mark as verified",
                    onClick = onMarkVerified,
                    enabled = code != null,
                )
            }
        }
    }
}
