package chat.app.onboarding

import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * DT7: the whole point is that an unknown region yields NO default, not a wrong
 * one — a German locale must never ship `+420`. These pin the mapping and, more
 * importantly, the empty-on-unknown behavior that makes the fix honest.
 */
class DialCodeTest {
    @Test
    fun known_regions_map_to_their_dial_code() {
        assertEquals("+49", dialCodeFor("DE"))
        assertEquals("+33", dialCodeFor("FR"))
        assertEquals("+44", dialCodeFor("GB"))
    }

    @Test
    fun lookup_is_case_insensitive() {
        assertEquals("+49", dialCodeFor("de"))
    }

    @Test
    fun unknown_or_blank_region_yields_empty_not_a_guess() {
        assertEquals("", dialCodeFor("XX"))
        assertEquals("", dialCodeFor(""))
        // The old bug's country must not leak in as a fallback for others.
        assertEquals("", dialCodeFor("ZZ"))
    }
}
