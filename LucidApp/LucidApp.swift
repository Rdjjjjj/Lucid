import SwiftUI

@main
struct LucidApp: App {
    var body: some Scene {
        WindowGroup("Lucid") {
            SettingsView()
        }
        .windowResizability(.contentSize)
    }
}
