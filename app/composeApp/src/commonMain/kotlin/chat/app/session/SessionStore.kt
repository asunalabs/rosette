package chat.app.session

import androidx.compose.runtime.Composable

/** [phone] is the E.164 number verified at onboarding — kept locally (same trust level as the session token) so opting into phone search later doesn't require re-entering it. */
data class Session(val sessionToken: String, val handle: String, val phone: String)

// ponytail: plaintext local storage (SharedPreferences / java.util.prefs),
// not EncryptedSharedPreferences/Keychain. Fine for a session token that's
// re-issued on next onboarding if lost; revisit if the token's blast radius
// grows (e.g. it starts guarding more than search/username actions).
interface SessionStore {
    fun load(): Session?
    fun save(session: Session)
    fun clear()
}

@Composable
expect fun rememberSessionStore(): SessionStore
