package chat.engine

import kotlin.test.Test
import kotlin.test.assertTrue

/**
 * DT2: a listener registered from Kotlin actually receives an event across the
 * FFI callback boundary — the half `callback_delivery.rs` can't cover, since
 * that test drives the Rust trait directly and never touches the generated
 * Kotlin `EngineEventListener`.
 *
 * `createContactLink()` offline dispatches `ConnectionStateChanged{online=false}`
 * (the relay is unreachable in tests), so it drives one real event with no
 * relay — the same offline-by-contract path `FfiSmokeTest` already leans on.
 *
 * desktopTest, not commonTest: the wait needs JVM threading, and the FFI dylib
 * only embeds on the desktop host anyway. The dispatch thread delivers
 * asynchronously, so poll briefly rather than assume the callback is synchronous.
 */
class ListenerSmokeTest {
    @Test
    fun listenerReceivesEventAcrossFfi() {
        val engine = ChatEngine("smoke-test")
        val events = mutableListOf<EngineEvent>()
        engine.setListener(object : EngineEventListener {
            override fun onEvent(event: EngineEvent) {
                synchronized(events) { events.add(event) }
            }
        })
        engine.createContactLink()
        var waited = 0
        while (synchronized(events) { events.isEmpty() } && waited < 2000) {
            Thread.sleep(20)
            waited += 20
        }
        assertTrue(
            synchronized(events) { events.any { it is EngineEvent.ConnectionStateChanged } },
            "expected a ConnectionStateChanged event, got ${synchronized(events) { events.toList() }}",
        )
    }
}
