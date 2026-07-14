package chat.app.directory

import java.security.MessageDigest

actual fun sha256Hex(input: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(input.encodeToByteArray())
    return digest.joinToString("") { "%02x".format(it) }
}
