import Foundation

public enum AIProtocol: String, Codable, CaseIterable, Sendable {
    case openAICompatible
    case anthropicCompatible
}

public struct AIConfiguration: Codable, Equatable, Sendable {
    public var apiProtocol: AIProtocol
    public var baseURL: URL
    public var model: String
    public var requestTimeout: TimeInterval

    public init(apiProtocol: AIProtocol, baseURL: URL, model: String, requestTimeout: TimeInterval = 20) {
        self.apiProtocol = apiProtocol
        self.baseURL = baseURL
        self.model = model
        self.requestTimeout = requestTimeout
    }

    public func validate() throws {
        guard let scheme = baseURL.scheme?.lowercased(), scheme == "https" || isLocalDevelopmentURL else {
            throw AIClientError.invalidConfiguration("中转站地址必须使用 HTTPS（本机开发地址除外）")
        }
        guard baseURL.host != nil else {
            throw AIClientError.invalidConfiguration("中转站地址缺少主机名")
        }
        guard !model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AIClientError.invalidConfiguration("请填写模型名称")
        }
        guard requestTimeout >= 1 && requestTimeout <= 120 else {
            throw AIClientError.invalidConfiguration("请求超时时间必须在 1 到 120 秒之间")
        }
    }

    private var isLocalDevelopmentURL: Bool {
        guard baseURL.scheme?.lowercased() == "http", let host = baseURL.host?.lowercased() else { return false }
        return host == "localhost" || host == "127.0.0.1" || host == "::1"
    }
}

public struct CorrectionRequest: Sendable {
    public let sentence: String

    public init(sentence: String) {
        self.sentence = sentence
    }
}

public struct TextChange: Codable, Equatable, Sendable {
    public let original: String
    public let replacement: String
    public let reason: String?

    public init(original: String, replacement: String, reason: String? = nil) {
        self.original = original
        self.replacement = replacement
        self.reason = reason
    }
}

public struct CorrectionResult: Codable, Equatable, Sendable {
    public let correctedText: String
    public let changes: [TextChange]
    public let confidence: Confidence

    enum CodingKeys: String, CodingKey {
        case correctedText = "corrected_text"
        case changes, confidence
    }

    public enum Confidence: String, Codable, Sendable {
        case high, medium, low
    }

    public init(correctedText: String, changes: [TextChange] = [], confidence: Confidence = .medium) {
        self.correctedText = correctedText
        self.changes = changes
        self.confidence = confidence
    }
}

public enum AIClientError: LocalizedError, Equatable, Sendable {
    case invalidConfiguration(String)
    case missingAPIKey
    case invalidResponse
    case serviceFailure(statusCode: Int)
    case transportFailure(String)

    public var errorDescription: String? {
        switch self {
        case .invalidConfiguration(let message): message
        case .missingAPIKey: "请先在设置中配置 API Key。"
        case .invalidResponse: "AI 服务返回了无法识别的结果。"
        case .serviceFailure(let statusCode): "AI 服务请求失败（HTTP \(statusCode)）。"
        case .transportFailure(let message): message
        }
    }
}
