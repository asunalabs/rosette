package chat.app.session

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import platform.Foundation.NSUserDefaults

// ponytail: NSUserDefaults, NOT the Keychain — a deliberate placeholder so the
// iOS framework links and the app runs for sideload dogfooding (iOS-11). The
// session token sits in the app's plist in plaintext: acceptable on your own
// test device, NOT for release. Upgrade path is iOS-6 (Keychain via SecItem*).
private const val K_TOKEN = "session_token"
private const val K_HANDLE = "handle"
private const val K_PHONE = "phone"

private class IosSessionStore : SessionStore {
    private val defaults = NSUserDefaults.standardUserDefaults

    override fun load(): Session? {
        val token = defaults.stringForKey(K_TOKEN) ?: return null
        val handle = defaults.stringForKey(K_HANDLE) ?: return null
        val phone = defaults.stringForKey(K_PHONE) ?: return null
        return Session(token, handle, phone)
    }

    override fun save(session: Session) {
        defaults.setObject(session.sessionToken, forKey = K_TOKEN)
        defaults.setObject(session.handle, forKey = K_HANDLE)
        defaults.setObject(session.phone, forKey = K_PHONE)
    }

    override fun clear() {
        listOf(K_TOKEN, K_HANDLE, K_PHONE).forEach { defaults.removeObjectForKey(it) }
    }
}

@Composable
actual fun rememberSessionStore(): SessionStore = remember { IosSessionStore() }
