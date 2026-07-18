import SwiftUI
import ComposeApp

// Bridges the Compose UIViewController (MainViewController.kt in iosMain, iOS-7)
// into SwiftUI. `MainViewControllerKt` is the Kotlin-file class Swift sees for
// top-level functions in MainViewController.kt.
struct ContentView: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        MainViewControllerKt.MainViewController()
    }

    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}
