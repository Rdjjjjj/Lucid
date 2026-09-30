import Foundation

public struct AISettingsRepository {
    private let defaults: UserDefaults
    private let keychain: KeychainAPIKeyStore
    private let configurationKey = "ai.configuration.v1"

    public init(defaults: UserDefaults, keychain: KeychainAPIKeyStore = KeychainAPIKeyStore()) {
        self.defaults = defaults
        self.keychain = keychain
    }

    public func loadConfiguration() throws -> AIConfiguration? {
        guard let data = defaults.data(forKey: configurationKey) else { return nil }
        return try JSONDecoder().decode(AIConfiguration.self, from: data)
    }

    public func saveConfiguration(_ configuration: AIConfiguration) throws {
        try configuration.validate()
        defaults.set(try JSONEncoder().encode(configuration), forKey: configurationKey)
    }

    public func saveAPIKey(_ apiKey: String) throws {
        try keychain.save(apiKey)
    }

    public func hasAPIKey() -> Bool {
        (try? keychain.read())?.isEmpty == false
    }

    public func clearAPIKey() throws {
        try keychain.delete()
    }
}
