package chat.app.storage

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.convert
import kotlinx.cinterop.usePinned
import platform.Foundation.NSApplicationSupportDirectory
import platform.Foundation.NSFileManager
import platform.Foundation.NSSearchPathForDirectoriesInDomains
import platform.Foundation.NSTemporaryDirectory
import platform.Foundation.NSUserDefaults
import platform.Foundation.NSUserDomainMask
import platform.Security.SecRandomCopyBytes
import platform.Security.kSecRandomDefault

// ponytail: the SQLCipher key lives in NSUserDefaults, NOT the Keychain — a
// deliberate placeholder so the framework links and the app runs for sideload
// dogfooding (iOS-11). This is NOT at-rest secure: the raw DB key sits in the
// app's plist in plaintext, so the local encrypted DB is only as protected as
// the device sandbox. Fine on your own test device; MUST be replaced before any
// non-dogfood distribution. Upgrade path is iOS-6: Keychain via SecItemAdd/
// SecItemCopyMatching with kSecAttrAccessibleAfterFirstUnlock (the contract in
// DbKeyStore.kt: released when the device is unlocked, no unlock screen).
private const val KEY_DB_LITERAL = "db_key_literal"

@OptIn(ExperimentalForeignApi::class)
private fun randomRawKeyLiteral(): String {
    val bytes = ByteArray(32)
    bytes.usePinned { pinned ->
        SecRandomCopyBytes(kSecRandomDefault, 32.convert(), pinned.addressOf(0))
    }
    return rawKeyLiteral(bytes) // shared commonMain helper → x'<64 hex>'
}

private fun appSupportChatDir(): String {
    val base = (
        NSSearchPathForDirectoriesInDomains(NSApplicationSupportDirectory, NSUserDomainMask, true)
            .firstOrNull() as? String
        ) ?: NSTemporaryDirectory()
    val dir = "$base/chat"
    NSFileManager.defaultManager.createDirectoryAtPath(dir, true, null, null)
    return dir
}

@Composable
actual fun rememberDbConfig(): DbConfig = remember {
    val defaults = NSUserDefaults.standardUserDefaults
    val keyLiteral = defaults.stringForKey(KEY_DB_LITERAL)
        ?: randomRawKeyLiteral().also { defaults.setObject(it, forKey = KEY_DB_LITERAL) }
    DbConfig(dbPath = "${appSupportChatDir()}/chat.db", dbKey = keyLiteral)
}

actual fun deleteDbFile(path: String) {
    NSFileManager.defaultManager.removeItemAtPath(path, null)
}
