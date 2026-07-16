package chat.app.onboarding

import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsNodeInteractionsProvider
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasTextExactly
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.runComposeUiTest
import androidx.compose.ui.test.waitUntilDoesNotExist
import androidx.compose.ui.test.waitUntilExactlyOneExists
import chat.app.directory.DirectoryClient
import chat.app.theme.ChatTheme
import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import kotlin.test.Test

/**
 * ET4: the onboarding flows a unit test structurally cannot reach.
 *
 * Every bug this file exists for lived in composable wiring, not in a pure
 * function. CQ-1's held machine, and ET8's `catch` block — which was the *only*
 * path to the held screen in production while every test covered the pure
 * mapping beside it. The review's own summary of the weakness: "the pure
 * function is tested ★★★ and the bugs live in the state machine around it."
 *
 * Driven against a stub JDK `HttpServer`, the same seam `DirectoryClientErrorTest`
 * uses: `DirectoryClient` already takes a `baseUrl`, so the real client and the
 * real composable run. Nothing is faked into production code to make it testable.
 *
 * Nodes are found by role (`hasSetTextAction`) and by the copy the user actually
 * reads, not by test tags — the copy *is* the assertion here (ET8 deleted a line
 * for being false; a tag would not have noticed).
 */
@OptIn(ExperimentalTestApi::class)
class OnboardingFlowUiTest {

    private val heldChip = "Can't reach verification — your code is fine"
    private val escalatedChip = "Still can't reach verification — tried 3 times"

    /** One server, routed per path, so a whole flow can be driven. */
    private fun stub(vararg routes: Pair<String, Pair<Int, String>>, block: (String) -> Unit) {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        routes.forEach { (path, response) ->
            val (status, body) = response
            server.createContext(path) { exchange ->
                val bytes = body.toByteArray()
                exchange.responseHeaders.add("Content-Type", "application/json")
                exchange.sendResponseHeaders(status, bytes.size.toLong())
                exchange.responseBody.use { it.write(bytes) }
            }
        }
        server.start()
        try {
            block("http://127.0.0.1:${server.address.port}")
        } finally {
            server.stop(0)
        }
    }

    private val signupOk = "/signup" to (200 to """{"status":"code_sent"}""")
    private val verifyOutage = "/verify" to (503 to """{"error":"verification temporarily unavailable"}""")
    private val verifyRejected = "/verify" to (400 to """{"error":"code rejected"}""")

    /** The OTP overlay is the only editable field on the OTP step. */
    private fun SemanticsNodeInteractionsProvider.otpField() = onAllNodes(hasSetTextAction())[0]

    private fun ComposeUiTest.start(base: String) {
        setContent {
            ChatTheme {
                OnboardingFlow(client = DirectoryClient(base), onComplete = { _, _, _ -> })
            }
        }
    }

    /** Welcome -> PhoneEntry -> AwaitingOtp, with a code typed and submitted. */
    private fun ComposeUiTest.reachOtpAndSubmit(code: String = "123456") {
        onNodeWithText("Get started").performClick()
        // [0] is the country code, [1] the number.
        onAllNodes(hasSetTextAction())[1].performTextInput("777123456")
        onNodeWithText("Next").performClick()
        waitUntilExactlyOneExists(hasTextExactly("Enter the code"))
        otpField().performTextInput(code)
        onNodeWithText("Verify").performClick()
    }

    /**
     * The headline finding of the ET6/ET7/ET8 review, in one test: a vendor
     * outage must reach the user as the held chip, not through the error
     * channel, which reads as blame for a code they typed correctly.
     *
     * Unreachable from a unit test — `isVerificationUnavailable` is covered
     * three ways in `OnboardingGateTest`, and the `catch` block that calls it had
     * no coverage at all.
     */
    @Test
    fun a503RendersTheHeldChipAndNotTheErrorChannel() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            reachOtpAndSubmit()

            waitUntilExactlyOneExists(hasTextExactly(heldChip))
            onNodeWithText(heldChip).assertIsDisplayed()
            onNodeWithText("Try again").assertIsDisplayed()
        }
    }

    /**
     * ET8: the deleted reassurance was false on every count — `POST /signup`
     * durably wrote a peppered hash one screen earlier. It is deleted, and this
     * is what stops it being helpfully restored.
     */
    @Test
    fun noReassuranceClaimRendersInTheHeldState() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly(heldChip))

            listOf(
                "nothing has been saved",
                "isn't registered",
                "you can erase it",
            ).forEach { claim ->
                onAllNodes(hasText(claim, substring = true)).assertCountEquals(0)
            }
        }
    }

    /** ET13: the chip is a status line, not a control — it must not eat the one action that helps. */
    @Test
    fun resendStaysAvailableWhileHeld() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly(heldChip))

            onNodeWithText("Resend code").assertIsDisplayed()
        }
    }

    /**
     * ET3: a repeated outage used to assign an equal data-class value, so Compose
     * skipped the recomposition and "Try again" looked like a dead button.
     */
    @Test
    fun retryingIntoAContinuingOutageEscalatesTheChip() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly(heldChip))

            onNodeWithText("Try again").performClick()
            onNodeWithText("Try again").performClick()

            waitUntilExactlyOneExists(hasTextExactly(escalatedChip))
            onNodeWithText(escalatedChip).assertIsDisplayed()
        }
    }

    /** ET3: the chip says "your code is fine" — it must not say that about a code the user just changed. */
    @Test
    fun editingTheCodeClearsTheHold() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly(heldChip))

            otpField().performTextReplacement("654321")

            waitUntilDoesNotExist(hasTextExactly(heldChip))
            onNodeWithText("Verify").assertIsDisplayed()
        }
    }

    /**
     * The other half of ET8's latch: a response that *did* check the code must
     * retire the hold, or "your code is fine" renders directly above "code
     * rejected". Codes expire while a vendor is down, so this is the common exit
     * from a hold, not an edge case.
     */
    @Test
    fun aRejectedCodeAfterAnOutageDoesNotLeaveBothMessagesOnScreen() = runComposeUiTest {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        var outage = true
        server.createContext("/signup") { ex ->
            val b = """{"status":"code_sent"}""".toByteArray()
            ex.responseHeaders.add("Content-Type", "application/json")
            ex.sendResponseHeaders(200, b.size.toLong())
            ex.responseBody.use { it.write(b) }
        }
        // Down, then recovered — the vendor comes back and rejects the now-expired code.
        server.createContext("/verify") { ex ->
            val (status, body) =
                if (outage) 503 to """{"error":"verification temporarily unavailable"}"""
                else 400 to """{"error":"code rejected"}"""
            val b = body.toByteArray()
            ex.responseHeaders.add("Content-Type", "application/json")
            ex.sendResponseHeaders(status, b.size.toLong())
            ex.responseBody.use { it.write(b) }
        }
        server.start()
        try {
            start("http://127.0.0.1:${server.address.port}")
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly(heldChip))

            outage = false
            onNodeWithText("Try again").performClick()

            waitUntilExactlyOneExists(hasTextExactly("code rejected"))
            onAllNodes(hasTextExactly(heldChip)).assertCountEquals(0)
        } finally {
            server.stop(0)
        }
    }

    /** ET10/CQ-4: a mistyped number must not be a dead end. */
    @Test
    fun changeNumberReturnsToPhoneEntry() = runComposeUiTest {
        stub(signupOk, verifyOutage) { base ->
            start(base)
            onNodeWithText("Get started").performClick()
            onAllNodes(hasSetTextAction())[1].performTextInput("777123456")
            onNodeWithText("Next").performClick()
            waitUntilExactlyOneExists(hasTextExactly("Enter the code"))

            onNodeWithText("‹  Change number").performClick()

            waitUntilExactlyOneExists(hasTextExactly("Next"))
            onNodeWithText("Next").assertIsDisplayed()
        }
    }

    /** An error about the step you left must not glow under the step you're on. */
    @Test
    fun theErrorChannelClearsOnTransition() = runComposeUiTest {
        stub(signupOk, verifyRejected) { base ->
            start(base)
            reachOtpAndSubmit()
            waitUntilExactlyOneExists(hasTextExactly("code rejected"))

            onNodeWithText("‹  Change number").performClick()

            waitUntilDoesNotExist(hasTextExactly("code rejected"))
        }
    }
}
