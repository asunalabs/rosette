package chat.app.session

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext

private const val PREFS_NAME = "chat_session"
private const val KEY_TOKEN = "session_token"
private const val KEY_HANDLE = "handle"
private const val KEY_PHONE = "phone"

private class AndroidSessionStore(private val context: Context) : SessionStore {
    private val prefs get() = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    override fun load(): Session? {
        val token = prefs.getString(KEY_TOKEN, null) ?: return null
        val handle = prefs.getString(KEY_HANDLE, null) ?: return null
        val phone = prefs.getString(KEY_PHONE, null) ?: return null
        return Session(token, handle, phone)
    }

    override fun save(session: Session) {
        prefs.edit()
            .putString(KEY_TOKEN, session.sessionToken)
            .putString(KEY_HANDLE, session.handle)
            .putString(KEY_PHONE, session.phone)
            .apply()
    }

    override fun clear() {
        prefs.edit().clear().apply()
    }
}

@Composable
actual fun rememberSessionStore(): SessionStore {
    val context = LocalContext.current
    return remember(context) { AndroidSessionStore(context) }
}
