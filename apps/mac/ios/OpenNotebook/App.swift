import SwiftUI

@main
struct OpenNotebookApp: App {
    var body: some Scene {
        WindowGroup {
            RuneView().ignoresSafeArea()
        }
    }
}

/// The Rust app, drawn by Rune inside `RuneViewController`.
struct RuneView: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> RuneViewController { RuneViewController() }
    func updateUIViewController(_ vc: RuneViewController, context: Context) {}
}
