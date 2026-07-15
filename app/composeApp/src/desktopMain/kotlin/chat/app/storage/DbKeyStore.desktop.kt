package chat.app.storage

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import com.github.javakeyring.Keyring
import com.github.javakeyring.PasswordAccessException
import java.io.File
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.security.SecureRandom

private const val SERVICE = "chat"
private const val ACCOUNT = "db-key"

private fun dataDir(): File {
    val home = System.getProperty("user.home")
    val os = System.getProperty("os.name").lowercase()
    val base = when {
        os.contains("mac") -> File(home, "Library/Application Support")
        os.contains("win") -> File(System.getenv("APPDATA") ?: "$home\\AppData\\Roaming")
        else -> File(System.getenv("XDG_DATA_HOME")?.takeIf { it.isNotBlank() } ?: "$home/.local/share")
    }
    return File(base, "chat").apply { mkdirs() }
}

private fun randomKeyHex(): String {
    val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
    return rawKeyLiteral(key).removePrefix("x'").removeSuffix("'")
}

/**
 * Keyfile fallback for hosts without a reachable OS credential store
 * (headless Linux with no Secret Service daemon). 0600, next to the DB.
 * Weaker ceiling than the OS store — the caller logs it once.
 */
internal fun keyfileDbKeyHex(keyfile: File): String {
    if (keyfile.exists()) return keyfile.readText().trim()
    val hex = randomKeyHex()
    try {
        Files.createFile(
            keyfile.toPath(),
            PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")),
        )
    } catch (_: UnsupportedOperationException) {
        keyfile.createNewFile() // non-POSIX (Windows): ACLs already scope to the user profile
    }
    keyfile.writeText(hex)
    return hex
}

private fun getOrCreateDbKeyHex(dir: File): String {
    try {
        val keyring = Keyring.create()
        val existing = try {
            keyring.getPassword(SERVICE, ACCOUNT)
        } catch (_: PasswordAccessException) {
            null // no entry yet
        }
        if (existing != null) return existing
        val hex = randomKeyHex()
        keyring.setPassword(SERVICE, ACCOUNT, hex)
        return hex
    } catch (e: Exception) {
        val keyfile = File(dir, "db.key")
        System.err.println(
            "chat: OS credential store unavailable (${e.message}); " +
                "DB key falls back to 0600 keyfile at $keyfile — " +
                "at-rest protection is then only filesystem permissions.",
        )
        return keyfileDbKeyHex(keyfile)
    }
}

@Composable
actual fun rememberDbConfig(): DbConfig {
    return remember {
        val dir = dataDir()
        DbConfig(
            dbPath = File(dir, "chat.db").absolutePath,
            dbKey = "x'${getOrCreateDbKeyHex(dir)}'",
        )
    }
}

actual fun deleteDbFile(path: String) {
    File(path).delete()
}
