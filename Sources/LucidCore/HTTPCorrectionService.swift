import Foundation

public protocol APIKeyStore: Sendable {
    func read() throws -> String?
}

public protocol CorrectionService: Sendable {
    func correct(_ request: CorrectionRequest) async throws -> CorrectionResult
}

enum CorrectionResponseParser {
    static func sentence(from data: Data) -> String? {
        if let outer = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let text = extractText(fromJSON: outer) {
            return text
        }
        if let raw = String(data: data, encoding: .utf8) {
            return cleanedSentence(raw)
        }
        return nil
    }

    private static let ignoredKeys: Set<String> = [
        "reasoning_content", "reasoning", "thinking", "thought", "reasoning_tokens",
        "id", "object", "created", "model", "usage", "system_fingerprint",
        "finish_reason", "index", "logprobs", "role", "refusal",
    ]

    static func extractText(fromJSON object: [String: Any]) -> String? {
        if let message = object["message"] as? [String: Any], let text = extractText(fromJSON: message) {
            return text
        }
        if let choices = object["choices"] as? [Any] {
            for choice in choices {
                if let text = extractText(fromValue: choice) {
                    return text
                }
            }
        }
        let preferredKeys = ["content", "text", "output_text", "corrected_text", "correctedText", "output"]
        for key in preferredKeys {
            if ignoredKeys.contains(key) { continue }
            if let value = object[key], let text = extractText(fromValue: value) {
                return text
            }
        }
        if let output = object["output"] as? [Any] {
            for item in output {
                if let text = extractText(fromValue: item) {
                    return text
                }
            }
        }
        for (key, value) in object where ignoredKeys.contains(key) == false {
            if preferredKeys.contains(key) { continue }
            if let text = extractText(fromValue: value) {
                return text
            }
        }
        return nil
    }

    static func extractText(fromValue value: Any) -> String? {
        if value is NSNull { return nil }
        if let text = value as? String {
            return cleanedSentence(text)
        }
        if let array = value as? [Any] {
            let joined = array.compactMap { item -> String? in
                if let text = item as? String { return text }
                if let object = item as? [String: Any] {
                    if let text = object["text"] as? String { return text }
                    if let text = object["content"] as? String { return text }
                    return extractText(fromJSON: object)
                }
                return nil
            }.joined()
            return cleanedSentence(joined)
        }
        if let object = value as? [String: Any] {
            return extractText(fromJSON: object)
        }
        return nil
    }

    static func cleanedSentence(_ raw: String) -> String? {
        var text = strippedCodeFence(raw).trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.isEmpty == false else { return nil }

        if text.hasPrefix("{") {
            if let json = extractFirstJSONObject(from: text),
               let data = json.data(using: .utf8),
               let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                let jsonKeys = ["corrected_text", "correctedText", "text", "sentence", "english", "output", "content"]
                for key in jsonKeys {
                    if let value = object[key] as? String, let cleaned = cleanedSentence(value) {
                        return cleaned
                    }
                }
            }
            if let recovered = firstJSONStringValue(in: text) {
                return recovered
            }
            return nil
        }

        if text.hasPrefix("["),
           let data = text.data(using: .utf8),
           let array = try? JSONSerialization.jsonObject(with: data) as? [Any] {
            let joined = array.compactMap { $0 as? String }.joined(separator: " ")
            if let cleaned = cleanedSentence(joined) { return cleaned }
        }

        if (text.hasPrefix("\"") && text.hasSuffix("\"") && text.count >= 2)
            || (text.hasPrefix("'") && text.hasSuffix("'") && text.count >= 2) {
            text = String(text.dropFirst().dropLast())
        }

        text = text.trimmingCharacters(in: CharacterSet(charactersIn: "\"'`"))
        text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.isEmpty == false else { return nil }
        if text.hasPrefix("{") || text.hasPrefix("[") { return nil }
        return text
    }


    static func firstJSONStringValue(in text: String) -> String? {
        let keys = ["corrected_text", "correctedText", "text", "sentence", "english", "output", "content"]
        for key in keys {
            let pattern = "\"\(key)\""
            guard let keyRange = text.range(of: pattern) else { continue }
            let remainder = text[keyRange.upperBound...]
            guard let colon = remainder.firstIndex(of: ":") else { continue }
            var index = remainder.index(after: colon)
            while index < remainder.endIndex, remainder[index].isWhitespace {
                index = remainder.index(after: index)
            }
            guard index < remainder.endIndex, remainder[index] == "\"" else { continue }
            index = remainder.index(after: index)
            var result = ""
            var escaped = false
            while index < remainder.endIndex {
                let character = remainder[index]
                if escaped {
                    result.append(character)
                    escaped = false
                } else if character == "\\" {
                    escaped = true
                } else if character == "\"" {
                    if let cleaned = cleanedSentence(result) { return cleaned }
                    return result.isEmpty ? nil : result
                } else {
                    result.append(character)
                }
                index = remainder.index(after: index)
            }
            if result.isEmpty == false {
                if let cleaned = cleanedSentence(result) { return cleaned }
                return result
            }
        }
        return nil
    }

    static func extractFirstJSONObject(from content: String) -> String? {
        guard let start = content.firstIndex(of: "{") else { return nil }
        var depth = 0
        var inString = false
        var escaped = false
        var end = start
        var index = start
        while index < content.endIndex {
            let character = content[index]
            if inString {
                if escaped {
                    escaped = false
                } else if character == "\\" {
                    escaped = true
                } else if character == "\"" {
                    inString = false
                }
            } else if character == "\"" {
                inString = true
            } else if character == "{" {
                depth += 1
            } else if character == "}" {
                depth -= 1
                if depth == 0 {
                    end = index
                    break
                }
            }
            index = content.index(after: index)
        }
        guard depth == 0 else { return nil }
        return String(content[start...end])
    }

    static func strippedCodeFence(_ content: String) -> String {
        var text = content.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("```") {
            if let firstNewline = text.firstIndex(of: "\n") {
                text = String(text[text.index(after: firstNewline)...])
            }
            if text.hasSuffix("```") {
                text = String(text.dropLast(3))
            }
            text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        return text
    }
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

        var urlRequest = URLRequest(url: endpointURL(pathSuffix: completionPathSuffix()))
        urlRequest.httpMethod = "POST"
        urlRequest.timeoutInterval = configuration.requestTimeout
        urlRequest.setValue("application/json", forHTTPHeaderField: "Content-Type")
        urlRequest.setValue("application/json", forHTTPHeaderField: "Accept")
        applyAuthorization(to: &urlRequest, key: key)
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
            throw AIClientError.transportFailure("无法连接 AI 服务，请检查网络和 AI 服务设置后重试。")
        }
    }

    public func fetchModels() async throws -> [String] {
        // 获取模型列表只需要服务地址和 API Key，模型本身由服务端返回。
        try configuration.validate(requireModel: false)
        guard let key = try apiKeyStore.read(), !key.isEmpty else { throw AIClientError.missingAPIKey }

        var urlRequest = URLRequest(url: endpointURL(pathSuffix: "models"))
        urlRequest.httpMethod = "GET"
        urlRequest.timeoutInterval = configuration.requestTimeout
        urlRequest.setValue("application/json", forHTTPHeaderField: "Accept")
        applyAuthorization(to: &urlRequest, key: key)

        do {
            let (data, response) = try await session.data(for: urlRequest)
            guard let http = response as? HTTPURLResponse else { throw AIClientError.invalidResponse }
            guard (200..<300).contains(http.statusCode) else {
                throw AIClientError.serviceFailure(statusCode: http.statusCode)
            }
            return try decodeModels(from: data)
        } catch let error as AIClientError {
            throw error
        } catch {
            throw AIClientError.transportFailure("无法获取模型列表，请检查网络和 AI 服务设置后重试。")
        }
    }

    private func completionPathSuffix() -> String {
        switch configuration.apiProtocol {
        case .openAICompatible: return "chat/completions"
        case .anthropicCompatible: return "messages"
        }
    }

    private func endpointURL(pathSuffix: String) -> URL {
        var url = configuration.baseURL
        let trimmedPath = url.path.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let hasVersion = trimmedPath == "v1" || trimmedPath.hasSuffix("/v1")
        url.append(path: hasVersion ? pathSuffix : "v1/\(pathSuffix)")
        return url
    }

    private func applyAuthorization(to request: inout URLRequest, key: String) {
        switch configuration.apiProtocol {
        case .openAICompatible:
            request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        case .anthropicCompatible:
            request.setValue(key, forHTTPHeaderField: "x-api-key")
            request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        }
    }

    private func requestBody(sentence: String) throws -> Data {
        let instruction = """
You are helping a Chinese speaker write English on a Mac input method.
The user types English mixed with pinyin placeholders for words they cannot spell.
Rewrite into ONE natural English sentence with the same meaning.
Convert pinyin into the intended English words or proper names, including movie/show titles.
Example: "juemingdushi is this movie good?" -> "Is Breaking Bad a good show?"
Do not add extra ideas, quotes, markdown, JSON, or explanation.
Reply with only the English sentence.
"""
        let payload: [String: Any]
        switch configuration.apiProtocol {
        case .openAICompatible:
            payload = [
                "model": configuration.model,
                "temperature": 0,
                "max_tokens": 1024,
                "thinking": ["type": "disabled"],
                "enable_thinking": false,
                "reasoning": ["effort": "none"],
                "messages": [
                    ["role": "system", "content": instruction],
                    ["role": "user", "content": sentence],
                ],
            ]
        case .anthropicCompatible:
            payload = [
                "model": configuration.model,
                "max_tokens": 1024,
                "temperature": 0,
                "system": instruction,
                "messages": [["role": "user", "content": sentence]],
            ]
        }
        return try JSONSerialization.data(withJSONObject: payload)
    }

    private func decodeResult(from data: Data) throws -> CorrectionResult {
        if let text = CorrectionResponseParser.sentence(from: data) {
            return CorrectionResult(correctedText: text)
        }
        throw AIClientError.invalidResponse
    }

    private func decodeModels(from data: Data) throws -> [String] {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AIClientError.invalidResponse
        }
        if let list = object["data"] as? [[String: Any]] {
            let ids = list.compactMap { $0["id"] as? String }
            if !ids.isEmpty { return ids }
        }
        if let list = object["models"] as? [[String: Any]] {
            let ids = list.compactMap { $0["id"] as? String }
            if !ids.isEmpty { return ids }
        }
        throw AIClientError.invalidResponse
    }
}
