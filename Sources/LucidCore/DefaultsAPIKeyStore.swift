import Foundation

/// Stores the API key in the shared app defaults, never in the login keychain.
/// Keychain access from an ad-hoc input method raises a password dialog or fails
/// with errSecCSBadObjectFormat (-67049).
public struct DefaultsAPIKeyStore: APIKeyStore, Sendable {
    private let defaults: UserDefaults
    private let key: String

    public init(defaults: UserDefaults, key: String = "ai.apiKey.v1") {
        self.defaults = defaults
        self.key = key
    }

    public func read() throws -> String? {
        let value = defaults.string(forKey: key)
        guard let value, value.isEmpty == false else { return nil }
        return value
    }

    public func save(_ value: String) throws {
        defaults.set(value, forKey: key)
    }

    public func delete() throws {
        defaults.removeObject(forKey: key)
    }
}

/// Key already read before the request. Correction must not look it up again.
public struct InMemoryAPIKeyStore: APIKeyStore, Sendable {
    private let apiKey: String

    public init(_ apiKey: String) {
        self.apiKey = apiKey
    }

    public func read() throws -> String? {
        apiKey
    }
}
