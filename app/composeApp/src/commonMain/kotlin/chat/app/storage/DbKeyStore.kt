package chat.app.storage

import androidx.compose.runtime.Composable

/**
 * At-rest encryption config for the local SQLCipher DB (issue #1, Signal
 * model): a random 256-bit key the user never sees, held by the OS key
 * store and released when the device is unlocked. No unlock screen. The
 * recovery PIN (issues #2-#4) never touches this key.
 */
data class DbConfig(
    /** Absolute path of the encrypted DB file. */
    val dbPath: String,
    /**
     * SQLCipher raw-key literal `x'<64 hex chars>'` — raw-key syntax skips
     * SQLCipher's internal PBKDF2, pointless work for a random key.
     */
    val dbKey: String,
)

@Composable
expect fun rememberDbConfig(): DbConfig

/** Reset path (key store wiped but DB survived): drop the unreadable DB. */
expect fun deleteDbFile(path: String)

internal fun rawKeyLiteral(key: ByteArray): String {
    require(key.size == 32) { "DB key must be 32 bytes, got ${key.size}" }
    val hex = key.joinToString("") { b -> (b.toInt() and 0xff).toString(16).padStart(2, '0') }
    return "x'$hex'"
}
