package chat.app.storage

import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermission
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

class DbKeyStoreTest {
    @Test
    fun rawKeyLiteralIsSqlcipherRawKeySyntax() {
        val literal = rawKeyLiteral(ByteArray(32) { it.toByte() })
        assertTrue(Regex("^x'[0-9a-f]{64}'$").matches(literal), "bad literal: $literal")
    }

    @Test
    fun keyfileFallbackRoundTripsAndIsOwnerOnly() {
        val dir = Files.createTempDirectory("dbkey").toFile()
        val keyfile = dir.resolve("db.key")
        val first = keyfileDbKeyHex(keyfile)
        assertTrue(Regex("^[0-9a-f]{64}$").matches(first), "bad key hex: $first")
        assertEquals(first, keyfileDbKeyHex(keyfile), "second call must return the same key")
        val perms = Files.getPosixFilePermissions(keyfile.toPath())
        assertEquals(
            setOf(PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE),
            perms,
            "keyfile must be 0600",
        )
    }

    @Test
    fun freshKeyfilesGetDistinctKeys() {
        val a = keyfileDbKeyHex(Files.createTempDirectory("dbkey-a").toFile().resolve("db.key"))
        val b = keyfileDbKeyHex(Files.createTempDirectory("dbkey-b").toFile().resolve("db.key"))
        assertNotEquals(a, b)
    }
}
