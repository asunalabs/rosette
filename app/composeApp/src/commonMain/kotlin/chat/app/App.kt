package chat.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.unit.dp
import chat.app.chatlist.ChatListScreen
import chat.app.chatlist.ConversationScreen
import chat.app.directory.DirectoryClient
import chat.app.directory.DirectoryException
import chat.app.onboarding.OnboardingFlow
import chat.app.session.Session
import chat.app.session.rememberSessionStore
import chat.app.pairing.FindPeopleScreen
import chat.app.storage.DbConfig
import chat.app.storage.deleteDbFile
import chat.app.storage.rememberDbConfig
import chat.app.theme.ChatTheme
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentTabBar
import chat.app.theme.LocalChatPalette
import chat.engine.ChatEngine
import chat.engine.Conversation
import chat.engine.EngineException

/**
 * T27: phone verification gates the app itself — no unverified-but-usable
 * state. `session` is null until onboarding (signup -> verify -> claim
 * username) completes; only then can `EngineScreen` construct `ChatEngine`.
 * A completed session is persisted (SessionStore) so onboarding doesn't
 * re-run on every app start.
 */
@Composable
fun App() {
    ChatTheme {
        val store = rememberSessionStore()
        var session by remember { mutableStateOf(store.load()) }
        val current = session
        val client = remember { DirectoryClient() }
        val db = rememberDbConfig()
        if (current == null) {
            OnboardingFlow(client) { token, claimedHandle, phone ->
                val newSession = Session(token, claimedHandle, phone)
                store.save(newSession)
                session = newSession
            }
        } else {
            EngineScreen(client = client, session = current, db = db)
        }
    }
}

private enum class Tab { Chats, FindPeople }

@Composable
private fun EngineScreen(client: DirectoryClient, session: Session, db: DbConfig) {
    val palette = LocalChatPalette.current
    // Issue #1: SQLCipher-persistent engine — identity, pairing, and history
    // survive restarts. remember {} — one engine per composition, as the
    // contract prescribes one per app start.
    fun open(): ChatEngine? = try {
        ChatEngine.newPersistent(session.handle, db.dbPath, db.dbKey)
    } catch (_: EngineException) {
        // BadKey/StorageFailed: the key store lost the key but the DB file
        // survived (e.g. device-to-device data copy). Unreadable forever —
        // offer the reset path, never crash, never silent fresh state.
        null
    }
    var engineOrNull by remember { mutableStateOf(open()) }
    val engine = engineOrNull ?: run {
        ResetLocalDataScreen(onReset = {
            deleteDbFile(db.dbPath)
            engineOrNull = open()
        })
        return
    }
    var tab by remember { mutableStateOf(Tab.Chats) }
    var openConversation by remember { mutableStateOf<Conversation?>(null) }

    // T25: best-effort — publish a fresh one-time pairing bootstrap so a
    // directory search hit can find and pair with this device. Silently
    // skipped offline/on failure: nothing here blocks the chat UI on it.
    LaunchedEffect(session.sessionToken) {
        val link = engine.createContactLink()
        if (link.isNotEmpty()) {
            try {
                client.publishPairingBootstrap(session.sessionToken, link)
            } catch (_: DirectoryException) {
                // best-effort; the user can still pair via QR/link directly.
            }
        }
    }

    val conversation = openConversation
    if (conversation != null) {
        ConversationScreen(engine = engine, conversation = conversation, onBack = { openConversation = null })
        return
    }

    Column(modifier = Modifier.fillMaxSize().background(palette.bg)) {
        Column(modifier = Modifier.weight(1f)) {
            when (tab) {
                Tab.Chats -> ChatListScreen(engine = engine, onOpenConversation = { openConversation = it })
                Tab.FindPeople -> FindPeopleScreen(client, session, engine, onBack = { tab = Tab.Chats })
            }
        }
        InstrumentTabBar(
            tabs = listOf("Chats", "Find people"),
            selected = tab.ordinal,
            onSelect = { tab = Tab.entries[it] },
        )
    }
}

/** Issue #1 failure path: key store wiped but the DB file survived. */
@Composable
private fun ResetLocalDataScreen(onReset: () -> Unit) {
    val palette = LocalChatPalette.current
    Column(
        modifier = Modifier.fillMaxSize().background(palette.bg).padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            "This device can't read its local data",
            style = MaterialTheme.typography.headlineSmall,
            color = palette.ink,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            "The key protecting this device's messages is gone, so the data " +
                "stored here can't be opened. Reset to start fresh — your " +
                "account itself is not affected.",
            style = MaterialTheme.typography.bodyMedium,
            color = palette.muted,
        )
        Spacer(Modifier.height(24.dp))
        InstrumentButton("Reset local data", onClick = onReset)
    }
}
