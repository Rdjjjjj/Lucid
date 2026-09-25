import Combine
import Foundation
import EnglishInputCore

@MainActor
final class SettingsModel: ObservableObject {
    @Published var apiProtocol: AIProtocol = .openAICompatible
    @Published var baseURL = ""
    @Published var model = ""
    @Published var apiKey = ""
    @Published private(set) var keyConfigured = false
    @Published private(set) var statusMessage = ""
    @Published private(set) var isTesting = false

    private let repository: AISettingsRepository
    private let keyStore: KeychainAPIKeyStore

    init() {
        let suiteName = Bundle.main.object(forInfoDictionaryKey: "EnglishInputAppGroup") as? String
            ?? "group.io.github.rdj.englishinput"
        let defaults = UserDefaults(suiteName: suiteName) ?? .standard
        let keychainGroup = Bundle.main.object(forInfoDictionaryKey: "EnglishInputKeychainAccessGroup") as? String
        keyStore = KeychainAPIKeyStore(accessGroup: keychainGroup)
        repository = AISettingsRepository(defaults: defaults, keychain: keyStore)
        keyConfigured = repository.hasAPIKey()

        if let config = try? repository.loadConfiguration() {
            apiProtocol = config.apiProtocol
            baseURL = config.baseURL.absoluteString
            model = config.model
        }
    }

    func save() {
        guard let url = URL(string: baseURL.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            statusMessage = "请输入有效的中转站地址。"
            return
        }
        let configuration = AIConfiguration(apiProtocol: apiProtocol, baseURL: url, model: model)
        do {
            try repository.saveConfiguration(configuration)
            if !apiKey.isEmpty {
                try repository.saveAPIKey(apiKey)
                apiKey = ""
                keyConfigured = true
            }
            statusMessage = "设置已保存。"
        } catch {
            statusMessage = error.localizedDescription
        }
    }

    func testConnection() {
        guard let url = URL(string: baseURL.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            statusMessage = "请输入有效的中转站地址。"
            return
        }
        let configuration = AIConfiguration(apiProtocol: apiProtocol, baseURL: url, model: model)
        isTesting = true
        statusMessage = "正在测试连接…"
        Task {
            defer { isTesting = false }
            do {
                let service = HTTPCorrectionService(configuration: configuration, apiKeyStore: keyStore)
                let result = try await service.correct(CorrectionRequest(sentence: "I need hepl."))
                statusMessage = "连接成功：\(result.correctedText)"
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }
}
