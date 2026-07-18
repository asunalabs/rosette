package chat.app.directory

// iOS has no java.security.MessageDigest and Kotlin/Native bundles no
// CommonCrypto binding (and CryptoKit is Swift-only, unreachable from K/N), so
// SHA-256 is implemented here in pure Kotlin. It MUST stay byte-identical to the
// JVM actuals: the directory's k-anonymity prefix bucketing (T3/T17) compares
// these hashes across platforms, so a mismatch would make an iOS user's phone/
// username search silently miss. Sha256Test's shared vectors guard that.
//
// ponytail: textbook FIPS 180-4 SHA-256, no dependency. The alternative (a
// CommonCrypto cinterop def) is more moving parts for a hash we can write once.

private val K = uintArrayOf(
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u, 0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
)

private infix fun UInt.rotr(bits: Int): UInt = (this shr bits) or (this shl (32 - bits))

private val HEX = "0123456789abcdef"

actual fun sha256Hex(input: String): String {
    val bytes = input.encodeToByteArray()
    val bitLen = bytes.size.toLong() * 8
    // 0x80 terminator + zero pad + 8-byte big-endian bit length, to a 64-byte
    // multiple.
    val padded = ByteArray(((bytes.size + 8) / 64 + 1) * 64)
    bytes.copyInto(padded)
    padded[bytes.size] = 0x80.toByte()
    for (i in 0 until 8) {
        padded[padded.size - 1 - i] = (bitLen ushr (8 * i)).toByte()
    }

    val h = uintArrayOf(
        0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
        0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u,
    )
    val w = UIntArray(64)
    var off = 0
    while (off < padded.size) {
        for (i in 0 until 16) {
            w[i] = ((padded[off + 4 * i].toUInt() and 0xffu) shl 24) or
                ((padded[off + 4 * i + 1].toUInt() and 0xffu) shl 16) or
                ((padded[off + 4 * i + 2].toUInt() and 0xffu) shl 8) or
                (padded[off + 4 * i + 3].toUInt() and 0xffu)
        }
        for (i in 16 until 64) {
            val s0 = (w[i - 15] rotr 7) xor (w[i - 15] rotr 18) xor (w[i - 15] shr 3)
            val s1 = (w[i - 2] rotr 17) xor (w[i - 2] rotr 19) xor (w[i - 2] shr 10)
            w[i] = w[i - 16] + s0 + w[i - 7] + s1
        }
        var a = h[0]; var b = h[1]; var c = h[2]; var d = h[3]
        var e = h[4]; var f = h[5]; var g = h[6]; var hh = h[7]
        for (i in 0 until 64) {
            val s1 = (e rotr 6) xor (e rotr 11) xor (e rotr 25)
            val ch = (e and f) xor (e.inv() and g)
            val t1 = hh + s1 + ch + K[i] + w[i]
            val s0 = (a rotr 2) xor (a rotr 13) xor (a rotr 22)
            val maj = (a and b) xor (a and c) xor (b and c)
            val t2 = s0 + maj
            hh = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d
        h[4] += e; h[5] += f; h[6] += g; h[7] += hh
        off += 64
    }

    val sb = StringBuilder(64)
    for (x in h) {
        for (shift in intArrayOf(24, 16, 8, 0)) {
            val byte = ((x shr shift) and 0xffu).toInt()
            sb.append(HEX[byte ushr 4])
            sb.append(HEX[byte and 0xf])
        }
    }
    return sb.toString()
}
