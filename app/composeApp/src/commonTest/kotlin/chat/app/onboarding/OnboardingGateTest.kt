package chat.app.onboarding

import chat.app.directory.DirectoryException
import chat.app.directory.VerifyResult
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

/**
 * T27's gate: "there is no unverified-but-usable app state."
 *
 * This regressed silently once already — `POST /verify` used to mint a real
 * `session_token` even on a `Degraded` (vendor-outage) outcome, and the flow
 * read only `sessionToken`, so an outage walked an unverified user into the
 * app and then persisted the session. ET6 closed that at the server; these
 * tests fail if either half returns.
 */
class OnboardingGateTest {

    private val phone = "+420777123456"

    @Test
    fun a_degraded_verification_never_opens_the_app() {
        // ET6 means a correct server can no longer produce this. That is the point:
        // this asserts the client would still refuse if a regression did.
        val result = VerifyResult(userId = 1L, sessionToken = "a-real-usable-token", verified = false)

        val next = nextAfterVerify(phone, result)

        assertIs<OnboardingState.AwaitingOtp>(next, "verified=false must not reach ClaimUsername")
        assertTrue(next.held, "the OTP step must show the held state, not silently re-prompt")
        assertEquals(phone, next.phone)
    }

    @Test
    fun a_verified_result_advances_to_the_username_claim() {
        val result = VerifyResult(userId = 1L, sessionToken = "tok", verified = true)

        val next = nextAfterVerify(phone, result)

        assertIs<OnboardingState.ClaimUsername>(next)
        assertEquals("tok", next.sessionToken)
        assertEquals(phone, next.phone)
    }

    /** The token being real is exactly what made the old bug invisible — presence of a token proves nothing. */
    @Test
    fun a_usable_token_does_not_by_itself_open_the_app() {
        val next = nextAfterVerify(phone, VerifyResult(1L, "indistinguishable-from-a-good-token", verified = false))

        assertIs<OnboardingState.AwaitingOtp>(next)
    }

    /**
     * ET6/ET8: after ET6 the vendor outage arrives as a 503, so this — not
     * `verified == false` — is the live path to the held screen. If this
     * mapping breaks, the held UI silently becomes unreachable and an outage
     * blames the user for a code they typed correctly.
     */
    @Test
    fun a_503_is_the_vendor_outage_path_to_the_held_screen() {
        val e = DirectoryException("verification temporarily unavailable", status = 503)

        assertTrue(isVerificationUnavailable(e), "a 503 must hold, not blame the user")
    }

    @Test
    fun a_rejected_code_is_the_users_to_fix_and_must_not_hold() {
        val e = DirectoryException("code rejected", status = 400)

        assertFalse(isVerificationUnavailable(e), "a wrong code is an error, not a hold")
    }

    /** A transport failure has no response, so it cannot be proven to be an outage — it must not claim "your code is fine". */
    @Test
    fun an_exception_with_no_status_does_not_hold() {
        assertFalse(isVerificationUnavailable(DirectoryException("connection refused")))
    }

    /**
     * The held chip says "your code is fine". A 400 says it isn't. The hold
     * must not survive a response that actually checked the code, or the two
     * render together — the exact false-copy ET8 deleted, re-introduced by a
     * latch. Reachable in the obvious way: codes expire during an outage, so
     * held -> "Try again" -> "code rejected" is the common exit from a hold.
     */
    @Test
    fun a_checked_answer_retires_the_hold() {
        val next = nextAfterVerifyError(phone, DirectoryException("code rejected", status = 400))

        assertIs<OnboardingState.AwaitingOtp>(next)
        assertFalse(next.held, "a 400 means the code WAS checked — the hold must clear")
        assertEquals(phone, next.phone)
    }

    @Test
    fun an_unchecked_code_holds() {
        val next = nextAfterVerifyError(phone, DirectoryException("verification temporarily unavailable", status = 503))

        assertIs<OnboardingState.AwaitingOtp>(next)
        assertTrue(next.held, "a 503 means nobody checked the code — hold, don't blame")
    }
}
