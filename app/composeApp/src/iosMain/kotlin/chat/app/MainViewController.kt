package chat.app

import androidx.compose.ui.window.ComposeUIViewController
import platform.UIKit.UIViewController

// The iOS entry point: the iosApp Xcode host (iOS-8) embeds this view
// controller. App() already owns nav, theme, and the onboarding gate, so
// there is nothing platform-specific to wire here.
fun MainViewController(): UIViewController = ComposeUIViewController { App() }
