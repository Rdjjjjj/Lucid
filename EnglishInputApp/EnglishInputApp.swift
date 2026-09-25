import SwiftUI

@main
struct EnglishInputApp: App {
    var body: some Scene {
        WindowGroup("English Input") {
            SettingsView()
        }
        .windowResizability(.contentSize)
    }
}
