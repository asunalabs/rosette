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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import chat.engine.DeliveryState
import chat.engine.EngineException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** DT3: an optimistic outgoing message, shown Pending until the send resolves. */
private data class PendingSend(val id: String, val body: String)

/**
 * The bubble-thread screen (the pasted Signal reference's conversation view).
 * `mine`/grouping only, no read-receipt/typing chrome — that's a separate,
 * undesigned surface, not something to improvise here.
 */
@Composable
fun ConversationScreen(
    engine: ChatEngine,
    conversation: Conversation,
    /** DT2: bumped on every engine event — re-reads the thread. See `rememberEngineRevision`. */
    revision: Int,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalChatPalette.current
    val messages = remember(conversation.id, revision) { engine.messages(conversation.id) }
    var draft by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    // DT3: the engine records a sent message only AFTER the relay round-trip
    // (as Sent or Failed) — there's no optimistic write on its side. So we hold
    // the in-flight message here to draw the Pending bubble, and drop it once
    // the coroutine resolves and the store's real message takes over via
    // `revision`. Its own id space ("pending-N") never collides with the store's
    // "msg-N", so LazyColumn keys stay unique across the swap.
    val pending = remember(conversation.id) { mutableStateListOf<PendingSend>() }
    var nextPendingId by remember(conversation.id) { mutableStateOf(0) }

    fun send(body: String) {
        val id = "pending-${nextPendingId++}"
        pending.add(PendingSend(id, body))
        scope.launch {
            try {
                // engine.send blocks until the relay accepts or gives up; off
                // the UI thread so an unreachable relay never freezes the frame.
                withContext(Dispatchers.Default) { engine.send(conversation.id, body) }
            } catch (_: EngineException) {
                // The engine already recorded the send as DeliveryState.Failed and
                // dispatched ConversationUpdated, so the Failed bubble arrives via
                // `revision`. Nothing to surface here but dropping the optimistic one.
                // ponytail: retry appends a fresh send rather than mutating the
                // failed row in place — the engine has no update-message call.
                // Good enough for the walking skeleton; revisit if double bubbles bite.
            } finally {
                pending.removeAll { it.id == id }
            }
        }
    }

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
            // reverseLayout draws the first item at the bottom, so declare the
            // in-flight sends first (newest, belong below every stored message).
            items(pending.asReversed(), key = { it.id }) { p ->
                Box(Modifier.padding(top = 5.dp)) {
                    MessageBubble(body = p.body, mine = true, pending = true)
                }
            }
            // DT11: newest-first for the reverseLayout. `top` padding sits above
            // each bubble (toward its older neighbor): 5dp within a run of the
            // same sender, 13dp where the sender changes (DESIGN.md:161 grouping).
            val newestFirst = messages.asReversed()
            itemsIndexed(newestFirst, key = { _, m -> m.id }) { i, m ->
                val olderNeighbor = newestFirst.getOrNull(i + 1)
                val gapTop = if (olderNeighbor != null && olderNeighbor.mine == m.mine) 5.dp else 13.dp
                Box(Modifier.padding(top = gapTop)) {
                    MessageBubble(
                        body = m.body,
                        mine = m.mine,
                        time = formatClockTime(m.timestampMs),
                        failed = m.delivery == DeliveryState.FAILED,
                        onRetry = if (m.delivery == DeliveryState.FAILED) {
                            { send(m.body) }
                        } else {
                            null
                        },
                    )
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
                        send(draft)
                        draft = ""
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text("↑", style = MaterialTheme.typography.labelLarge, color = if (canSend) palette.onAccent else palette.muted)
            }
        }
    }
}
