package chat.app.session

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import java.util.prefs.Preferences

private class DesktopSessionStore : SessionStore {
    private val prefs = Preferences.userRoot().node("chat/app/session")

    override fun load(): Session? {
        val token = prefs.get("session_token", null) ?: return null
        val handle = prefs.get("handle", null) ?: return null
        val phone = prefs.get("phone", null) ?: return null
        return Session(token, handle, phone)
    }

    override fun save(session: Session) {
        prefs.put("session_token", session.sessionToken)
        prefs.put("handle", session.handle)
        prefs.put("phone", session.phone)
    }

    override fun clear() {
        prefs.clear()
    }
}

@Composable
actual fun rememberSessionStore(): SessionStore = remember { DesktopSessionStore() }
