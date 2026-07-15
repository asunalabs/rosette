package chat.app.chatlist

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.app.theme.ChatListRow
import chat.app.theme.LocalChatPalette
import chat.app.theme.LucideIcons
import chat.engine.ChatEngine
import chat.engine.Conversation

/** T27's chat surface — Signal-anatomy chat list (row height, hairline dividers), Rosette avatars instead of photos. */
@Composable
fun ChatListScreen(
    engine: ChatEngine,
    onOpenConversation: (Conversation) -> Unit,
    modifier: Modifier = Modifier,
    /** Issue #4: settings entry point lives in this top bar. */
    onOpenSettings: () -> Unit = {},
) {
    val palette = LocalChatPalette.current
    val conversations = remember { engine.conversations() }

    Column(modifier = modifier.fillMaxSize().background(palette.bg)) {
        Box(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 16.dp), contentAlignment = Alignment.CenterStart) {
            Text(
                "Chats",
                style = MaterialTheme.typography.labelLarge.copy(fontSize = 20.sp, fontWeight = FontWeight.Bold),
                color = palette.ink,
            )
            Icon(
                LucideIcons.Settings,
                contentDescription = "Settings",
                tint = palette.muted,
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .clickable(onClick = onOpenSettings)
                    .padding(4.dp)
                    .size(22.dp),
            )
        }
        if (conversations.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
                Text(
                    "No conversations yet — find people to start one.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = palette.muted,
                    textAlign = TextAlign.Center,
                )
            }
        } else {
            LazyColumn(modifier = Modifier.fillMaxSize()) {
                items(conversations, key = { it.id }) { c ->
                    ChatListRow(
                        displayName = c.displayName,
                        lastMessage = c.lastMessage,
                        unread = c.unread.toInt(),
                        verified = c.verified,
                        seed = c.id,
                        onClick = { onOpenConversation(c) },
                    )
                }
            }
        }
    }
}
