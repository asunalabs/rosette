package chat.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import chat.app.chatlist.ChatListScreen
import chat.app.chatlist.ConversationScreen
import chat.app.directory.BackupUploader
import chat.app.directory.DirectoryClient
import chat.app.directory.DirectoryException
import chat.app.onboarding.OnboardingFlow
import chat.app.pairing.FindPeopleScreen
import chat.app.session.Session
import chat.app.session.rememberSessionStore
import chat.app.settings.ChangePinScreen
import chat.app.settings.SettingsScreen
import chat.app.theme.ChatTheme
import chat.app.theme.InstrumentTabBar
import chat.app.theme.LocalChatPalette
import chat.engine.ChatEngine
import chat.engine.Conversation

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
        if (current == null) {
            // ponytail: enroll/restore = null until #1's persistent engine
            // lands — then enroll does backupEnroll + putBackup (PIN/phrase
            // steps become mandatory, issue #2 acceptance 1) and restore
            // does ChatEngine.newFromBackup + optional backupRewrapPin +
            // putBackup + session save (issue #3).
            OnboardingFlow(client, enroll = null, restore = null) { token, claimedHandle, phone ->
                val newSession = Session(token, claimedHandle, phone)
                store.save(newSession)
                session = newSession
            }
        } else {
            EngineScreen(client = client, session = current)
        }
    }
}

private enum class Tab { Chats, FindPeople }

/** Issue #4: full-screen surfaces above the tab bar. */
private enum class Screen { Main, Settings, ChangePin }

@Composable
private fun EngineScreen(client: DirectoryClient, session: Session) {
    val palette = LocalChatPalette.current
    // remember {} — one engine per composition, as the contract prescribes
    // one per app start.
    val engine = remember { ChatEngine(session.handle) }
    var tab by remember { mutableStateOf(Tab.Chats) }
    var openConversation by remember { mutableStateOf<Conversation?>(null) }
    val scope = rememberCoroutineScope()
    var screen by remember { mutableStateOf(Screen.Main) }
    // Issue #2: one debounced recovery-bundle re-upload per contact change.
    // Inert until backupEnroll has run on a persistent engine.
    val backupUploader = remember { BackupUploader(scope, engine, client, session.sessionToken) }

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

    // Issue #4: settings + Change PIN sit above the tabbed surface.
    when (screen) {
        Screen.Settings -> {
            SettingsScreen(
                session = session,
                onBack = { screen = Screen.Main },
                onChangePin = { screen = Screen.ChangePin },
            )
            return
        }
        Screen.ChangePin -> {
            ChangePinScreen(
                engine = engine,
                client = client,
                session = session,
                onBack = { screen = Screen.Settings },
            )
            return
        }
        Screen.Main -> {}
    }

    Column(modifier = Modifier.fillMaxSize().background(palette.bg)) {
        Column(modifier = Modifier.weight(1f)) {
            when (tab) {
                Tab.Chats -> ChatListScreen(
                    engine = engine,
                    onOpenConversation = { openConversation = it },
                    onOpenSettings = { screen = Screen.Settings },
                )
                Tab.FindPeople -> FindPeopleScreen(
                    client,
                    session,
                    engine,
                    onBack = { tab = Tab.Chats },
                    onContactAdded = { backupUploader.schedule() },
                )
            }
        }
        InstrumentTabBar(
            tabs = listOf("Chats", "Find people"),
            selected = tab.ordinal,
            onSelect = { tab = Tab.entries[it] },
        )
    }
}
