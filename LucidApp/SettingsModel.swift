import AppKit
import Carbon
import Combine
import Foundation
import LucidCore

@MainActor
final class SettingsModel: ObservableObject {
    @Published var apiProtocol: AIProtocol = .openAICompatible
    @Published var baseURL = ""
    @Published var model = ""
    @Published var apiKey = ""
    @Published private(set) var keyConfigured = false
    @Published private(set) var statusMessage = ""
    @Published private(set) var isTesting = false
    @Published private(set) var isFetchingModels = false
    @Published private(set) var availableModels: [String] = []

    @Published private(set) var inputMethodStatus = "正在检查输入法状态…"

    func refreshInputMethodStatus() {
        inputMethodStatus = InputMethodEnabler.statusText()
    }

    func enableInputMethod() {
        statusMessage = InputMethodEnabler.enable()
        refreshInputMethodStatus()
    }

    func openKeyboardSettings() {
        InputMethodEnabler.openKeyboardSettings()
    }

    /// 地址或协议变化后，之前获取的模型可能来自另一项服务。
    func clearAvailableModels() {
        availableModels = []
    }

    private let repository: AISettingsRepository
    private let keyStore: DefaultsAPIKeyStore

    init() {
        let suiteName = Bundle.main.object(forInfoDictionaryKey: "LucidAppGroup") as? String
            ?? "group.io.github.rdj.lucid"
        let defaults = UserDefaults(suiteName: suiteName) ?? .standard
        keyStore = DefaultsAPIKeyStore(defaults: defaults)
        repository = AISettingsRepository(defaults: defaults, keyStore: keyStore)
        keyConfigured = repository.sharedAPIKey() != nil

        if let config = try? repository.loadConfiguration() {
            apiProtocol = config.apiProtocol
            baseURL = config.baseURL.absoluteString
            model = config.model
        }
        refreshInputMethodStatus()
    }

    func save() {
        guard let url = makeBaseURL() else {
            statusMessage = "请输入有效的 AI 服务地址。"
            return
        }
        let configuration = makeConfiguration(baseURL: url)
        do {
            try repository.saveConfiguration(configuration)
            if !apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                try persistAPIKey()
            }
            statusMessage = "设置已保存。"
        } catch {
            statusMessage = error.localizedDescription
        }
    }

    func testConnection() {
        guard let url = makeBaseURL() else {
            statusMessage = "请输入有效的 AI 服务地址。"
            return
        }
        let configuration = makeConfiguration(baseURL: url)
        isTesting = true
        statusMessage = "正在测试连接…"
        Task {
            defer { isTesting = false }
            do {
                let service = try await makeService(with: configuration)
                let result = try await service.correct(CorrectionRequest(sentence: "I need hepl."))
                statusMessage = "连接成功：\(result.correctedText)"
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    func fetchModels() {
        guard !isFetchingModels else { return }
        guard let url = makeBaseURL() else {
            statusMessage = "请输入有效的 AI 服务地址。"
            return
        }

        let configuration = makeConfiguration(baseURL: url)
        isFetchingModels = true
        statusMessage = "正在获取模型列表…"
        Task {
            defer { isFetchingModels = false }
            do {
                // 获取模型列表不应要求先填写模型。新 Key 在这里保存后即可直接使用。
                if !apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    try persistAPIKey()
                }
                let service = HTTPCorrectionService(configuration: configuration, apiKeyStore: keyStore)
                let models = try await service.fetchModels()
                let uniqueModels = Array(Set(models.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }))
                    .filter { !$0.isEmpty }
                    .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
                availableModels = uniqueModels

                let currentModel = model.trimmingCharacters(in: .whitespacesAndNewlines)
                if let matchingModel = uniqueModels.first(where: { $0 == currentModel }) {
                    model = matchingModel
                } else {
                    model = uniqueModels.first ?? ""
                }

                if uniqueModels.isEmpty {
                    statusMessage = "服务端没有返回可用模型，请检查服务地址或 API 权限。"
                } else {
                    // 获取模型后把当前服务地址、协议和默认模型一起落盘，
                    // 避免用户重启应用后丢失刚刚填写但尚未点击“保存设置”的配置。
                    try repository.saveConfiguration(makeConfiguration(baseURL: url))
                    statusMessage = "已获取 \(uniqueModels.count) 个模型，请在下方选择。"
                }
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    /// 先把当前页面上的 BaseURL / 模型 / Key 保存下来，再创建请求服务。
    /// 这样「测试连接」和「获取模型列表」无需先单独点一次「保存设置」。
    private func makeService(with configuration: AIConfiguration) async throws -> HTTPCorrectionService {
        try repository.saveConfiguration(configuration)
        if !apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            try persistAPIKey()
        }
        return HTTPCorrectionService(configuration: configuration, apiKeyStore: keyStore)
    }

    private func persistAPIKey() throws {
        let value = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return }
        try repository.saveAPIKey(value)
        apiKey = ""
        keyConfigured = true
    }

    private func makeConfiguration(baseURL: URL) -> AIConfiguration {
        AIConfiguration(
            apiProtocol: apiProtocol,
            baseURL: baseURL,
            model: model.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    private func makeBaseURL() -> URL? {
        URL(string: baseURL.trimmingCharacters(in: .whitespacesAndNewlines))
    }
}


enum InputMethodEnabler {
    static let parentID = "io.github.rdj.inputmethod.lucid"
    static let childID = "io.github.rdj.inputmethod.lucid.english"

    static func statusText() -> String {
        let parent = source(id: parentID)
        let child = source(id: childID)
        let parentEnabled = parent.map { isEnabled($0) } ?? false
        let childEnabled = child.map { isEnabled($0) } ?? false
        if parent == nil && child == nil {
            return "尚未检测到 Lucid 输入法。请确认已完成安装，然后重新登录 macOS。"
        }
        if childEnabled {
            return "Lucid 已启用，可在菜单栏输入法图标里切换。"
        }
        if parentEnabled && !childEnabled {
            return "Lucid 已安装，但还没有启用。请点击“启用 Lucid”，或重新登录 macOS 后再试。"
        }
        return "Lucid 已安装，但尚未启用。请点击“启用 Lucid”，或在系统键盘设置中添加。"
    }

    static func enable() -> String {
        if let url = installedBundleURL() {
            _ = TISRegisterInputSource(url as CFURL)
        }
        guard let parent = source(id: parentID) else {
            return "没有找到 Lucid 输入法。请先完成安装，然后重新登录 macOS。"
        }
        let enableParent = TISEnableInputSource(parent)
        var enableChild: OSStatus = noErr
        var selectChild: OSStatus = noErr
        if let child = source(id: childID) {
            enableChild = TISEnableInputSource(child)
            selectChild = TISSelectInputSource(child)
        }
        if let child = source(id: childID), isEnabled(child) {
            if selectChild == noErr {
                return "Lucid 已启用，可直接在当前输入框使用；也可从菜单栏输入法图标切换。"
            }
            return "Lucid 已启用。请到菜单栏输入法图标里切换。"
        }
        return "系统暂时无法启用 Lucid（状态码：\(enableParent)/\(enableChild)/\(selectChild)）。请重新登录 macOS 后再到系统键盘设置中添加。"
    }

    static func openKeyboardSettings() {
        let urls = [
            "x-apple.systempreferences:com.apple.Keyboard-Settings.extension?TextInput",
            "x-apple.systempreferences:com.apple.Keyboard-Settings.extension"
        ]
        for value in urls {
            if let url = URL(string: value), NSWorkspace.shared.open(url) {
                return
            }
        }
    }

    private static func installedBundleURL() -> URL? {
        let paths = [
            "/Library/Input Methods/LucidInputMethod.app",
            NSHomeDirectory() + "/Library/Input Methods/LucidInputMethod.app"
        ]
        return paths.map { URL(fileURLWithPath: $0) }.first { FileManager.default.fileExists(atPath: $0.path) }
    }

    private static func source(id: String) -> TISInputSource? {
        guard let list = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else {
            return nil
        }
        for src in list {
            if sourceID(src) == id { return src }
        }
        return nil
    }

    private static func sourceID(_ src: TISInputSource) -> String {
        guard let raw = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { return "" }
        return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }

    private static func isEnabled(_ src: TISInputSource) -> Bool {
        guard let raw = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsEnabled) else { return false }
        return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
    }
}
