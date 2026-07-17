package chat.app.pairing

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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import chat.app.directory.DirectoryClient
import chat.app.directory.DirectoryException
import chat.app.directory.isSessionExpired
import chat.app.directory.SearchResult
import chat.app.directory.hashPrefix
import chat.app.directory.normalizePhoneInput
import chat.app.directory.phoneSearchHash
import chat.app.session.Session
import chat.app.theme.HairlineDivider
import chat.app.theme.InstrumentButton
import chat.app.theme.InstrumentField
import chat.app.theme.InstrumentSegments
import chat.app.theme.InstrumentToggle
import chat.app.theme.LocalChatPalette
import chat.engine.ChatEngine
import chat.engine.EngineException
import kotlinx.coroutines.launch

private enum class LookupMode { Username, Phone }

/**
 * T25's client half: username lookup (OQ10's default) or opt-in phone
 * lookup, each resolving to a user_id that [DirectoryClient.requestPairingBootstrap]
 * can turn into a real `ChatEngine.pairWithLink` call.
 */
@Composable
fun FindPeopleScreen(
    client: DirectoryClient,
    session: Session,
    engine: ChatEngine,
    onBack: () -> Unit,
    onSessionExpired: () -> Unit,
) {
    val palette = LocalChatPalette.current
    var mode by remember { mutableStateOf(LookupMode.Username) }
    var status by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun pairWith(userId: Long, label: String) {
        loading = true; status = null
        scope.launch {
            try {
                val link = client.requestPairingBootstrap(session.sessionToken, userId)
                if (link == null) {
                    status = "$label hasn't published a pairing link right now — ask them to open the app."
                } else {
                    engine.pairWithLink(link)
                    status = "Paired with $label."
                }
            } catch (e: DirectoryException) {
                // A dead token is not a status line — it needs onboarding, not
                // a message the user can only stare at.
                if (e.isSessionExpired()) onSessionExpired() else status = e.message
            } catch (e: EngineException) {
                status = "Pairing failed: ${e.message}"
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
            Text("Find people", style = MaterialTheme.typography.headlineSmall, color = palette.ink)
        }
        HairlineDivider()

        Column(modifier = Modifier.fillMaxSize().padding(24.dp)) {
            InstrumentSegments(
                labels = listOf("Username", "Phone number"),
                selected = mode.ordinal,
                onSelect = { mode = LookupMode.entries[it]; status = null },
            )
            Spacer(Modifier.height(24.dp))
            when (mode) {
                LookupMode.Username -> UsernameLookup(loading) { nickname, discriminator ->
                    loading = true; status = null
                    scope.launch {
                        try {
                            val userId = client.lookupUsername(session.sessionToken, nickname, discriminator)
                            status = if (userId == null) "No one has that handle." else null
                            userId?.let { pairWith(it, "$nickname#$discriminator") }
                        } catch (e: DirectoryException) {
                            if (e.isSessionExpired()) onSessionExpired() else status = e.message
                        } finally {
                            loading = false
                        }
                    }
                }
                LookupMode.Phone -> PhoneLookup(
                    client = client,
                    session = session,
                    loading = loading,
                    setLoading = { loading = it },
                    setStatus = { status = it },
                    onSessionExpired = onSessionExpired,
                    onFound = ::pairWith,
                )
            }
            status?.let {
                Spacer(Modifier.height(16.dp))
                Text(it, style = MaterialTheme.typography.labelMedium, color = palette.muted)
            }
        }
    }
}

@Composable
private fun UsernameLookup(loading: Boolean, onSubmit: (nickname: String, discriminator: Int) -> Unit) {
    var handle by remember { mutableStateOf("") }
    val parts = handle.split("#")
    val discriminator = parts.getOrNull(1)?.toIntOrNull()
    val nickname = parts.getOrNull(0)?.takeIf { it.isNotBlank() }
    InstrumentField(
        value = handle,
        onValueChange = { handle = it },
        label = "Handle",
        placeholder = "mira#07",
    )
    Spacer(Modifier.height(24.dp))
    InstrumentButton(
        text = if (loading) "Looking up…" else "Find & pair",
        onClick = { onSubmit(nickname!!, discriminator!!) },
        enabled = !loading && nickname != null && discriminator != null,
    )
}

@Composable
private fun PhoneLookup(
    client: DirectoryClient,
    session: Session,
    loading: Boolean,
    setLoading: (Boolean) -> Unit,
    setStatus: (String?) -> Unit,
    onSessionExpired: () -> Unit,
    onFound: (Long, String) -> Unit,
) {
    val palette = LocalChatPalette.current
    var searchable by remember { mutableStateOf<Boolean?>(null) }
    var query by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Text(
            "Let people who have my number find me",
            style = MaterialTheme.typography.bodyLarge,
            color = palette.ink,
            modifier = Modifier.weight(1f),
        )
        InstrumentToggle(
            checked = searchable == true,
            onCheckedChange = { on ->
                scope.launch {
                    try {
                        val hash = if (on) phoneSearchHash(session.phone) else null
                        client.setSearchable(session.sessionToken, on, hash)
                        searchable = on
                    } catch (e: DirectoryException) {
                        if (e.isSessionExpired()) onSessionExpired() else setStatus(e.message)
                    }
                }
            },
        )
    }
    Spacer(Modifier.height(32.dp))
    InstrumentField(
        value = query,
        onValueChange = { query = it },
        label = "Their phone number",
        placeholder = "+1 555 0100",
    )
    Spacer(Modifier.height(24.dp))
    InstrumentButton(
        text = if (loading) "Searching…" else "Find & pair",
        onClick = {
            setLoading(true); setStatus(null)
            scope.launch {
                try {
                    val targetHash = phoneSearchHash(query)
                    val bucket = client.search(session.sessionToken, hashPrefix(targetHash))
                    val match: SearchResult? = bucket.firstOrNull { it.searchHash == targetHash }
                    if (match == null) {
                        setStatus("Not found — they may not be registered, or haven't opted in to phone search.")
                    } else {
                        onFound(match.userId, match.handle)
                    }
                } catch (e: DirectoryException) {
                    if (e.isSessionExpired()) onSessionExpired() else setStatus(e.message)
                } finally {
                    setLoading(false)
                }
            }
        },
        enabled = !loading && normalizePhoneInput(query).length >= 9,
    )
}
