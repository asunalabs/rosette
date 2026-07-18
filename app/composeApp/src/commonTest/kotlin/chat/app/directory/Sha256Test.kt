package chat.app.directory

import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * Shared SHA-256 vectors. Runs on every target, so it guards the JVM actuals
 * (MessageDigest) AND the pure-Kotlin iOS actual against the same oracle — the
 * directory's cross-platform hash bucketing (T3/T17) breaks silently if they
 * diverge. Vectors are the canonical FIPS 180-4 / NIST values.
 */
class Sha256Test {
    @Test
    fun emptyString() {
        assertEquals(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            sha256Hex(""),
        )
    }

    @Test
    fun abc() {
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            sha256Hex("abc"),
        )
    }

    @Test
    fun multiBlock() {
        // 56 bytes forces a second padding block (the length no longer fits the
        // first) — the boundary a single-block impl would get wrong.
        assertEquals(
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            sha256Hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        )
    }
}
