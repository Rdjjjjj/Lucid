import Foundation

public struct AISettingsRepository {
    private let defaults: UserDefaults
    private let keyStore: DefaultsAPIKeyStore
    private let configurationKey = "ai.configuration.v1"

    public init(defaults: UserDefaults, keyStore: DefaultsAPIKeyStore? = nil) {
        self.defaults = defaults
        self.keyStore = keyStore ?? DefaultsAPIKeyStore(defaults: defaults)
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
        try keyStore.save(apiKey)
    }

    public func hasAPIKey() -> Bool {
        sharedAPIKey() != nil
    }

    public func clearAPIKey() throws {
        try keyStore.delete()
    }

    /// Key stored with the app settings. Never touches the keychain.
    public func sharedAPIKey() -> String? {
        try? keyStore.read()
    }
}
