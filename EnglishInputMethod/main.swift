import Cocoa
import InputMethodKit

private let connectionName = "io.github.rdj.englishinput.inputmethod"

let server = IMKServer(name: connectionName, bundleIdentifier: Bundle.main.bundleIdentifier)
_ = server
NSApplication.shared.run()
