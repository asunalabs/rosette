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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import chat.app.chatlist.ChatListScreen
import chat.app.chatlist.ConversationScreen
import chat.app.directory.BackupUploader
import chat.app.directory.DirectoryClient
import chat.app.directory.DirectoryException
import chat.app.directory.isSessionExpired
import chat.app.onboarding.OnboardingFlow
import chat.app.session.Session
import chat.app.session.rememberSessionStore
import chat.app.pairing.FindPeopleScreen
import chat.app.settings.ChangePinScreen
import chat.app.settings.SettingsScreen
import chat.app.storage.DbConfig
import chat.app.storage.deleteDbFile
import chat.app.storage.rememberDbConfig
import chat.app.theme.ChatTheme
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentTabBar
import chat.app.theme.LocalChatPalette
import chat.engine.ChatEngine
import chat.engine.Conversation
import chat.engine.EngineEvent
import chat.engine.EngineEventListener
import chat.engine.EngineException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

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
        // Enroll/restore build the persistent engine mid-onboarding;
        // EngineScreen picks it up instead of reopening the same SQLCipher
        // file it already holds.
        var pendingEngine by remember { mutableStateOf<ChatEngine?>(null) }
        if (current == null) {
            OnboardingFlow(
                client = client,
                // Issue #2 acceptance 1: enroll is always passed, so the
                // PIN + phrase steps are mandatory on every fresh signup.
                enroll = { token, handle, pin ->
                    withContext(Dispatchers.Default) {
                        // An existing DB here is an orphan from an abandoned
                        // onboarding (a saved session skips this flow):
                        // newPersistent reopens it and backupEnroll simply
                        // overwrites the recovery material.
                        val engine = pendingEngine
                            ?: ChatEngine.newPersistent(handle, db.dbPath, db.dbKey)
                                .also { pendingEngine = it }
                        val enrollment = engine.backupEnroll(pin)
                        client.putBackup(token, enrollment.bundle)
                        enrollment.phrase
                    }
                },
                // Issue #3: runs on Dispatchers.Default via fetchBundle.
                restore = { req ->
                    // Same orphan rule; newFromBackup refuses to overwrite
                    // an existing file, so clear it first.
                    deleteDbFile(db.dbPath)
                    val engine = ChatEngine.newFromBackup(
                        "", db.dbPath, db.dbKey, req.bundle, req.secret,
                    )
                    if (req.newPin != null) {
                        // Phrase path: the forced fresh PIN replaces the old
                        // wrap server-side before the app opens.
                        client.putBackup(req.sessionToken, engine.backupRewrapPin(req.newPin))
                    }
                    pendingEngine = engine
                    val restored = Session(req.sessionToken, engine.displayName(), req.phone)
                    store.save(restored)
                    session = restored
                },
            ) { token, claimedHandle, phone ->
                val newSession = Session(token, claimedHandle, phone)
                store.save(newSession)
                session = newSession
            }
        } else {
            EngineScreen(
                client = client,
                session = current,
                db = db,
                initial = pendingEngine,
                // The only path back to onboarding once a session exists. Without
                // it a 401 is terminal: the branch above keys on `current == null`,
                // so a token the directory has forgotten is never replaced and
                // search/pairing fail for good. `clear()` had no call site at all
                // until this — the recovery existed on the interface and nowhere
                // else.
                onSessionExpired = {
                    store.clear()
                    session = null
                },
            )
        }
    }
}

private enum class Tab { Chats, FindPeople }

/** Issue #4: full-screen surfaces above the tab bar. */
private enum class Screen { Main, Settings, ChangePin }

/**
 * DT2: the engine pushes, the UI pulls. Returns a counter bumped on every
 * engine event; anything reading engine state keys its `remember` on it and
 * re-reads.
 *
 * Registered here, at the engine's own scope, and never inside a screen:
 * `setListener` REPLACES, so a second registration would silently unhook the
 * first. One engine, one listener, fanned out from a single counter.
 *
 * The counter says *that* something changed, not what. Every store mutation
 * already dispatches (inbound, send, pairing, verify), and
 * `conversations()`/`messages()` are a mutex and a clone away — so re-reading
 * keeps the Rust store the one source of truth. Patching event payloads into a
 * Kotlin-side copy would create a second one, free to drift.
 */
@Composable
private fun rememberEngineRevision(engine: ChatEngine): Int {
    var revision by remember(engine) { mutableStateOf(0) }
    DisposableEffect(engine) {
        engine.setListener(object : EngineEventListener {
            // Always the `chat-ffi-dispatch` thread, never the UI thread (FFI
            // contract, review OV8). Single writer, so `++` cannot lose a bump,
            // and Compose state is safe to write from any thread.
            override fun onEvent(event: EngineEvent) {
                revision++
            }
        })
        // `set_listener` has no unset, and the engine outlives this screen
        // (remembered one scope up) — nothing to tear down.
        onDispose {}
    }
    return revision
}

@Composable
private fun EngineScreen(
    client: DirectoryClient,
    session: Session,
    db: DbConfig,
    initial: ChatEngine?,
    onSessionExpired: () -> Unit,
) {
    val palette = LocalChatPalette.current
    // Issue #1: SQLCipher-persistent engine — identity, pairing, and history
    // survive restarts. remember {} — one engine per composition, as the
    // contract prescribes one per app start. [initial] is the engine
    // enroll/restore already built this run, if any.
    fun open(): ChatEngine? = try {
        ChatEngine.newPersistent(session.handle, db.dbPath, db.dbKey)
    } catch (_: EngineException) {
        // BadKey/StorageFailed: the key store lost the key but the DB file
        // survived (e.g. device-to-device data copy). Unreadable forever —
        // offer the reset path, never crash, never silent fresh state.
        null
    }
    var engineOrNull by remember { mutableStateOf(initial ?: open()) }
    val engine = engineOrNull ?: run {
        ResetLocalDataScreen(onReset = {
            deleteDbFile(db.dbPath)
            engineOrNull = open()
        })
        return
    }
    val revision = rememberEngineRevision(engine)
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
            } catch (e: DirectoryException) {
                // Best-effort, with one exception: a 401 is not a failure to
                // shrug off. This runs on every launch, so it is usually the
                // first request a returning user makes — and therefore the first
                // chance to notice the token is dead, before they hit a search
                // that mysteriously returns nothing.
                if (e.isSessionExpired()) onSessionExpired()
                // otherwise the user can still pair via QR/link directly.
            }
        }
    }

    val conversation = openConversation
    if (conversation != null) {
        ConversationScreen(
            engine = engine,
            conversation = conversation,
            revision = revision,
            onBack = { openConversation = null },
        )
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
                    handle = session.handle,
                    revision = revision,
                    onOpenConversation = { openConversation = it },
                    onOpenSettings = { screen = Screen.Settings },
                )
                Tab.FindPeople -> FindPeopleScreen(
                    client = client,
                    session = session,
                    engine = engine,
                    onBack = { tab = Tab.Chats },
                    onContactAdded = { backupUploader.schedule() },
                    onSessionExpired = onSessionExpired,
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
