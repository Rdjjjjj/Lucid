import Foundation

public protocol APIKeyStore: Sendable {
    func read() throws -> String?
}

public protocol CorrectionService: Sendable {
    func correct(_ request: CorrectionRequest) async throws -> CorrectionResult
}

public struct HTTPCorrectionService: CorrectionService {
    private let configuration: AIConfiguration
    private let apiKeyStore: any APIKeyStore
    private let session: URLSession

    public init(configuration: AIConfiguration, apiKeyStore: any APIKeyStore, session: URLSession = .shared) {
        self.configuration = configuration
        self.apiKeyStore = apiKeyStore
        self.session = session
    }

    public func correct(_ request: CorrectionRequest) async throws -> CorrectionResult {
        try configuration.validate()
        guard let key = try apiKeyStore.read(), !key.isEmpty else { throw AIClientError.missingAPIKey }

        var urlRequest = URLRequest(url: endpointURL())
        urlRequest.httpMethod = "POST"
        urlRequest.timeoutInterval = configuration.requestTimeout
        urlRequest.setValue("application/json", forHTTPHeaderField: "Content-Type")
        urlRequest.setValue("application/json", forHTTPHeaderField: "Accept")
        switch configuration.apiProtocol {
        case .openAICompatible:
            urlRequest.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        case .anthropicCompatible:
            urlRequest.setValue(key, forHTTPHeaderField: "x-api-key")
            urlRequest.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        }
        urlRequest.httpBody = try requestBody(sentence: request.sentence)

        do {
            let (data, response) = try await session.data(for: urlRequest)
            guard let http = response as? HTTPURLResponse else { throw AIClientError.invalidResponse }
            guard (200..<300).contains(http.statusCode) else {
                throw AIClientError.serviceFailure(statusCode: http.statusCode)
            }
            return try decodeResult(from: data)
        } catch let error as AIClientError {
            throw error
        } catch {
            throw AIClientError.transportFailure("无法连接 AI 服务，请检查网络和中转站设置后重试。")
        }
    }

    private func endpointURL() -> URL {
        var url = configuration.baseURL
        let path = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let hasVersionSuffix = path == "v1" || path.hasSuffix("/v1")
        let suffix: String
        switch configuration.apiProtocol {
        case .openAICompatible: suffix = hasVersionSuffix ? "chat/completions" : "v1/chat/completions"
        case .anthropicCompatible: suffix = hasVersionSuffix ? "messages" : "v1/messages"
        }
        url.append(path: suffix)
        return url
    }

    private func requestBody(sentence: String) throws -> Data {
        let instruction = "You are a minimal English sentence corrector for a Chinese learner. Infer any pinyin-placeholder word from the sentence context. Correct only the pinyin placeholder and clear grammar errors. Preserve the user's meaning, tone, and wording. Do not translate the whole sentence or add ideas. Return only a JSON object with keys corrected_text (string), changes (array of objects with original, replacement, reason), and confidence (high, medium, or low)."
        let payload: [String: Any]
        switch configuration.apiProtocol {
        case .openAICompatible:
            payload = [
                "model": configuration.model,
                "temperature": 0,
                "messages": [
                    ["role": "system", "content": instruction],
                    ["role": "user", "content": sentence],
                ],
            ]
        case .anthropicCompatible:
            payload = [
                "model": configuration.model,
                "max_tokens": 512,
                "temperature": 0,
                "system": instruction,
                "messages": [["role": "user", "content": sentence]],
            ]
        }
        return try JSONSerialization.data(withJSONObject: payload)
    }

    private func decodeResult(from data: Data) throws -> CorrectionResult {
        guard let outer = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AIClientError.invalidResponse
        }
        let content: String?
        switch configuration.apiProtocol {
        case .openAICompatible:
            let choices = outer["choices"] as? [[String: Any]]
            let message = choices?.first?["message"] as? [String: Any]
            content = message?["content"] as? String
        case .anthropicCompatible:
            let blocks = outer["content"] as? [[String: Any]]
            content = blocks?.compactMap { $0["text"] as? String }.joined()
        }
        guard let content,
              let contentData = normalizedJSONData(from: content),
              let object = try? JSONSerialization.jsonObject(with: contentData) as? [String: Any],
              let correctedText = object["corrected_text"] as? String,
              !correctedText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AIClientError.invalidResponse
        }
        let changes = (object["changes"] as? [[String: Any]] ?? []).compactMap { item -> TextChange? in
            guard let original = item["original"] as? String,
                  let replacement = item["replacement"] as? String else { return nil }
            return TextChange(original: original, replacement: replacement, reason: item["reason"] as? String)
        }
        let confidenceValue = object["confidence"] as? String ?? "medium"
        let confidence = CorrectionResult.Confidence(rawValue: confidenceValue) ?? .medium
        return CorrectionResult(correctedText: correctedText, changes: changes, confidence: confidence)
    }

    private func normalizedJSONData(from content: String) -> Data? {
        var text = content.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("```"), let firstNewline = text.firstIndex(of: "\n"), text.hasSuffix("```") {
            text = String(text[text.index(after: firstNewline)..<text.index(text.endIndex, offsetBy: -3)])
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }
        return text.data(using: .utf8)
    }
}
