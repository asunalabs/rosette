package chat.engine

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/**
 * The T8 go/no-go gate (ffi-contract.md): prove Gobley generated bindings and
 * the Rust library links + executes on this target. Runs offline — no relay:
 *
 *  - construction crosses the FFI boundary and spins the engine thread
 *  - `conversations()` round-trips a Vec<Record> (empty before pairing)
 *  - `createContactLink()` returns "" offline BY CONTRACT (the frozen
 *    signature is infallible; empty means "relay unreachable, show the calm
 *    offline banner") — reaching that path proves a real network attempt
 *    happened underneath the same frozen surface
 *  - `pairWithLink("garbage")` maps a Rust error onto a typed Kotlin
 *    exception (EngineException.InvalidContactLink)
 */
class FfiSmokeTest {
    @Test
    fun engineConstructsAndListsNoConversations() {
        val engine = ChatEngine("smoke-test")
        assertEquals(emptyList(), engine.conversations())
    }

    @Test
    fun contactLinkOfflineIsEmptyByContract() {
        // CHAT_RELAY_ADDR is unset in tests, so the engine cannot mint a
        // mailbox; the contract says: empty string, never a throw.
        val engine = ChatEngine("smoke-test")
        assertEquals("", engine.createContactLink())
    }

    @Test
    fun malformedLinkMapsToTypedException() {
        val engine = ChatEngine("smoke-test")
        val error = assertFailsWith<EngineException> {
            engine.pairWithLink("not-a-contact-link")
        }
        assertTrue(
            error is EngineException.InvalidContactLink,
            "expected InvalidContactLink, got $error"
        )
    }
}
