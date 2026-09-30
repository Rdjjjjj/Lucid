import Carbon
import Cocoa
import InputMethodKit

private let bundleID = "io.github.rdj.inputmethod.lucid"
private let connectionName = "io.github.rdj.inputmethod.lucid_Connection"
private let modeID = "io.github.rdj.inputmethod.lucid.english"

private func sourceID(_ src: TISInputSource) -> String {
    guard let raw = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { return "" }
    return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
}

private func isEnabled(_ src: TISInputSource) -> Bool {
    guard let raw = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsEnabled) else { return false }
    return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
}

private func lucidSources() -> [TISInputSource] {
    let list = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] ?? []
    return list.filter { sourceID($0).hasPrefix(bundleID) }
}

private func registerBundle() {
    _ = TISRegisterInputSource(Bundle.main.bundleURL as CFURL)
}

private func disableLucid() {
    for source in lucidSources() {
        _ = TISDisableInputSource(source)
    }
}

/// 先启用父源，父源真正启用后再启用子模式。
/// 父源启用失败时保持子模式关闭，否则系统设置的「添加」列表会把 Lucid 藏起来。
private func enableLucid() {
    registerBundle()
    let sources = lucidSources()
    let parents = sources.filter { sourceID($0) == bundleID }
    let children = sources.filter { sourceID($0) == modeID }
    for parent in parents {
        _ = TISEnableInputSource(parent)
    }
    let parentReady = parents.contains { isEnabled($0) }
    if parentReady {
        for child in children {
            _ = TISEnableInputSource(child)
        }
    } else {
        for child in children {
            _ = TISDisableInputSource(child)
        }
    }
}

private func selectLucid() {
    if let child = lucidSources().first(where: { sourceID($0) == modeID }), isEnabled(child) {
        _ = TISSelectInputSource(child)
    }
}

let args = Set(CommandLine.arguments.dropFirst())
if args.contains("--deactivate") {
    disableLucid()
    exit(EXIT_SUCCESS)
}
if args.contains("--install") {
    registerBundle()
    enableLucid()
    selectLucid()
    exit(EXIT_SUCCESS)
}
if args.contains("--enable") {
    enableLucid()
    exit(EXIT_SUCCESS)
}
if args.contains("--select") {
    selectLucid()
    exit(EXIT_SUCCESS)
}

// 正常被 imklaunchagent 拉起时只提供输入法服务，不要反复 TISEnable。
registerBundle()
let server = IMKServer(name: connectionName, bundleIdentifier: Bundle.main.bundleIdentifier)
_ = server
NSApplication.shared.run()
