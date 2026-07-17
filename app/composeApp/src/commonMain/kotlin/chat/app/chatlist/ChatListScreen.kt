package chat.app.chatlist

import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import chat.app.theme.ChatListRow
import chat.app.theme.ChatMonoStyle
import chat.app.theme.InstrumentButton
import chat.app.theme.HairlineDivider
import chat.app.theme.LocalChatPalette
import chat.app.theme.Rosette
import chat.engine.ChatEngine
import chat.engine.Conversation
import kotlinx.coroutines.delay

/** T27's chat surface — Signal-anatomy chat list (row height, hairline dividers), Rosette avatars instead of photos. */
@Composable
fun ChatListScreen(
    engine: ChatEngine,
    /** DT4/D2: the user's own handle — shown (mono, tap-to-copy) in the You menu. */
    handle: String,
    /** DT2: bumped on every engine event — re-reads the store. See `rememberEngineRevision`. */
    revision: Int,
    onOpenConversation: (Conversation) -> Unit,
    modifier: Modifier = Modifier,
    /** D2: the ONLY route to Settings — the You-menu dropdown, not a tab. */
    onOpenSettings: () -> Unit = {},
    /** DT9/DT14: the empty-state primary action opens Find people (the FAB destination). */
    onFindPeople: () -> Unit = {},
) {
    val palette = LocalChatPalette.current
    val conversations = remember(revision) { engine.conversations() }

    Column(modifier = modifier.fillMaxSize().background(palette.bg)) {
        Row(
            modifier = Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            YouMenu(handle = handle, onOpenSettings = onOpenSettings)
            Spacer(Modifier.width(12.dp))
            Text(
                "Chats",
                style = MaterialTheme.typography.labelLarge.copy(fontSize = 20.sp, fontWeight = FontWeight.Bold),
                color = palette.ink,
            )
        }
        if (conversations.isEmpty()) {
            // DT14: statement type + one primary action, per DESIGN.md:47-48 —
            // not a dead muted one-liner (wireframe-v1 bans "no chats yet" text).
            Column(
                modifier = Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "Nobody here yet",
                    style = MaterialTheme.typography.headlineSmall,
                    color = palette.ink,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Find someone by their handle or number, and your first conversation starts here.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = palette.muted,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(24.dp))
                InstrumentButton("Find people", onClick = onFindPeople)
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

/**
 * D2: the chat list's top-left is your own Rosette; tapping it drops a
 * Signal-style menu carrying your handle and the ONLY route to Settings. Built
 * on a raw `Popup`, not Material's `DropdownMenu` — DESIGN.md's hard-NO on
 * Material defaults applies to chrome too.
 */
@Composable
private fun YouMenu(handle: String, onOpenSettings: () -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    val belowRosette = with(LocalDensity.current) { 44.dp.roundToPx() }
    Box {
        Box(Modifier.clip(RoundedCornerShape(50)).clickable { expanded = true }) {
            Rosette(seed = handle, size = 36.dp)
        }
        if (expanded) {
            Popup(
                alignment = Alignment.TopStart,
                offset = IntOffset(0, belowRosette),
                onDismissRequest = { expanded = false },
                properties = PopupProperties(focusable = true),
            ) {
                YouMenuCard(
                    handle = handle,
                    onSettings = {
                        expanded = false
                        onOpenSettings()
                    },
                )
            }
        }
    }
}

@Composable
private fun YouMenuCard(handle: String, onSettings: () -> Unit) {
    val palette = LocalChatPalette.current
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    // Reset the "Copied" confirmation after a beat, so the row goes back to
    // inviting the next copy rather than latching.
    LaunchedEffect(copied) {
        if (copied) {
            delay(1200)
            copied = false
        }
    }
    Column(
        modifier = Modifier
            .width(240.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(palette.surface),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable {
                    clipboard.setText(AnnotatedString(handle))
                    copied = true
                }
                .padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // The one string this product asks people to pass around: mono,
            // because `mira#07` is a crypto fact, not prose (DESIGN.md Typography).
            Text(
                handle,
                style = ChatMonoStyle.copy(fontSize = 18.sp),
                color = palette.ink,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            Text(
                if (copied) "Copied" else "Copy",
                style = MaterialTheme.typography.labelMedium,
                color = if (copied) palette.accent else palette.muted,
            )
        }
        HairlineDivider()
        Text(
            "Settings",
            style = MaterialTheme.typography.bodyLarge,
            color = palette.ink,
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onSettings)
                .padding(horizontal = 16.dp, vertical = 14.dp),
        )
    }
}
