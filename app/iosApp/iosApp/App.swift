import SwiftUI

@main
struct ChatApp: App {
    var body: some Scene {
        WindowGroup {
            // App() already draws its own system-bar insets (safeDrawingPadding,
            // DT16), so the host hands Compose the full screen.
            ContentView().ignoresSafeArea(.all)
        }
    }
}
