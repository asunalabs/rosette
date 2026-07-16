package chat.app.directory

import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking

/**
 * ET7/ARCH-6. Ktor leaves `expectSuccess` false by default, so before this
 * fix nothing was thrown on 4xx/5xx: `DirectoryClient.call`'s handlers were
 * dead code and an error envelope reached `.body()`, crashing on the missing
 * fields. These are error-path tests because the error paths are what shipped
 * broken — the happy path always worked.
 *
 * The stub is the JDK's own `HttpServer` against a real socket: `DirectoryClient`
 * already takes a `baseUrl`, so no engine-injection seam has to exist in
 * production code just to be testable. `desktopTest` rather than `commonTest`
 * because `com.sun.net.httpserver` is JVM-only.
 */
class DirectoryClientErrorTest {
    private fun withStub(status: Int, body: String, block: (String) -> Unit) {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/") { exchange ->
            val bytes = body.toByteArray()
            exchange.responseHeaders.add("Content-Type", "application/json")
            exchange.sendResponseHeaders(status, bytes.size.toLong())
            exchange.responseBody.use { it.write(bytes) }
        }
        server.start()
        try {
            block("http://127.0.0.1:${server.address.port}")
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun wrongOtpSurfacesTheServerMessageInsteadOfCrashing() =
        withStub(400, """{"error":"code rejected"}""") { base ->
            val e = assertFailsWith<DirectoryException> {
                runBlocking { DirectoryClient(base).verify("+15551234567", "999999") }
            }
            assertEquals("code rejected", e.message)
            assertEquals(400, e.status)
        }

    /**
     * ET6 mints no session on a vendor timeout; its 503 must reach the app as an
     * error carrying the status. The status is load-bearing, not decoration:
     * `isVerificationUnavailable` reads it to decide whether the user gets the
     * held screen or gets blamed for a code they typed correctly (ET8).
     */
    @Test
    fun vendorUnavailable503SurfacesAsAnErrorCarryingItsStatus() =
        withStub(503, """{"error":"verification temporarily unavailable"}""") { base ->
            val e = assertFailsWith<DirectoryException> {
                runBlocking { DirectoryClient(base).verify("+15551234567", "000000") }
            }
            assertEquals("verification temporarily unavailable", e.message)
            assertEquals(503, e.status)
        }

    /** A signup 400 must surface, not silently advance the user to "Enter the code" for an SMS that was never sent. */
    @Test
    fun signupErrorSurfacesInsteadOfClaimingAnSmsWasSent() =
        withStub(400, """{"error":"phone number is in cooldown after a recent deletion"}""") { base ->
            val e = assertFailsWith<DirectoryException> {
                runBlocking { DirectoryClient(base).signup("+15551234567") }
            }
            assertEquals("phone number is in cooldown after a recent deletion", e.message)
        }

    /** A taken username is a 409, not a crash. */
    @Test
    fun takenUsernameSurfacesAsAnError() =
        withStub(409, """{"error":"nickname unavailable"}""") { base ->
            assertFailsWith<DirectoryException> {
                runBlocking { DirectoryClient(base).claimUsername("tok", "mira") }
            }
        }

    /**
     * The `ignoreUnknownKeys` half: a field the installed client has never
     * heard of must not brick it. T27's own attestation-token work adds one.
     */
    @Test
    fun anUnknownResponseFieldDoesNotBrickTheClient() =
        withStub(
            200,
            """{"user_id":7,"session_token":"tok","verified":true,"attestation_token":"added-later"}""",
        ) { base ->
            val res = runBlocking { DirectoryClient(base).verify("+15551234567", "000000") }
            assertEquals(7L, res.userId)
            assertEquals("tok", res.sessionToken)
            assertTrue(res.verified)
        }
}
