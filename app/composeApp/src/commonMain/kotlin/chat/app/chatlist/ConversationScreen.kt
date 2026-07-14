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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import chat.app.theme.HairlineDivider
import chat.app.theme.InstrumentField
import chat.app.theme.LocalChatPalette
import chat.app.theme.MessageBubble
import chat.app.theme.Rosette
import chat.engine.ChatEngine
import chat.engine.Conversation

/**
 * The bubble-thread screen (the pasted Signal reference's conversation view).
 * `mine`/grouping only, no read-receipt/typing chrome — that's a separate,
 * undesigned surface, not something to improvise here.
 */
@Composable
fun ConversationScreen(engine: ChatEngine, conversation: Conversation, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val palette = LocalChatPalette.current
    var messages by remember(conversation.id) { mutableStateOf(engine.messages(conversation.id)) }
    var draft by remember { mutableStateOf("") }

    Column(modifier = modifier.fillMaxSize().background(palette.bg)) {
        Row(
            modifier = Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.clickable(onClick = onBack).padding(8.dp)) {
                Text("←", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
            }
            Spacer(Modifier.width(4.dp))
            Rosette(seed = conversation.id, verified = conversation.verified, size = 32.dp)
            Spacer(Modifier.width(8.dp))
            Text(conversation.displayName, style = MaterialTheme.typography.labelLarge, color = palette.ink)
        }
        HairlineDivider()

        LazyColumn(
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
            reverseLayout = true,
        ) {
            items(messages.reversed(), key = { it.id }) { m ->
                Box(Modifier.padding(vertical = 4.dp)) {
                    MessageBubble(body = m.body, mine = m.mine)
                }
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            InstrumentField(
                value = draft,
                onValueChange = { draft = it },
                placeholder = "Message",
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(12.dp))
            val canSend = draft.isNotBlank()
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .background(if (canSend) palette.accent else palette.surface2)
                    .clickable(enabled = canSend) {
                        engine.send(conversation.id, draft)
                        draft = ""
                        messages = engine.messages(conversation.id)
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text("↑", style = MaterialTheme.typography.labelLarge, color = if (canSend) palette.onAccent else palette.muted)
            }
        }
    }
}
