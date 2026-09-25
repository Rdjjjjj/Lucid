import AppKit
import EnglishInputCore
import InputMethodKit

/// Passes text through immediately, then offers a user-confirmed correction for completed sentences.
@objc(EnglishInputController)
public final class EnglishInputController: IMKInputController, @unchecked Sendable {
    private final class ClientContext: @unchecked Sendable {
        let client: IMKTextInput
        weak var object: NSObject?

        init(client: IMKTextInput, object: NSObject?) {
            self.client = client
            self.object = object
        }
    }

    private var tracker = SentenceTracker()
    private var requestID: UInt64 = 0
    private var pauseWorkItem: DispatchWorkItem?
    private var suggestion: CorrectionSuggestionPanel?
    private weak var suggestionClient: NSObject?
    private var suggestionRange: NSRange?
    private var suggestionVersion: UInt64?

    private let defaults: UserDefaults
    private let keyStore: KeychainAPIKeyStore

    public override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        let suiteName = Bundle.main.object(forInfoDictionaryKey: "EnglishInputAppGroup") as? String
            ?? "group.io.github.rdj.englishinput"
        defaults = UserDefaults(suiteName: suiteName) ?? .standard
        let accessGroup = Bundle.main.object(forInfoDictionaryKey: "EnglishInputKeychainAccessGroup") as? String
        keyStore = KeychainAPIKeyStore(accessGroup: accessGroup)
        super.init(server: server, delegate: delegate, client: inputClient)
    }

    public override func inputText(_ string: String?, key keyCode: Int, modifiers flags: Int, client sender: Any?) -> Bool {
        guard let string, let textClient = sender as? IMKTextInput else { return false }
        let clientObject = sender as? NSObject

        if isReturnKey(keyCode: keyCode, string: string), tracker.pendingText.isEmpty == false {
            pauseWorkItem?.cancel()
            if let completed = tracker.flushOnPause() {
                requestCorrection(for: completed, client: textClient, clientObject: clientObject)
            }
            return true
        }

        if isReturnKey(keyCode: keyCode, string: string), suggestion != nil {
            return true
        }

        guard !string.isEmpty else { return false }
        textClient.insertText(string, replacementRange: NSRange(location: NSNotFound, length: NSNotFound))
        pauseWorkItem?.cancel()

        let completed = tracker.append(string)
        for sentence in completed {
            requestCorrection(for: sentence, client: textClient, clientObject: clientObject)
        }
        schedulePauseFlush(for: textClient, clientObject: clientObject)
        return true
    }

    public override func deactivateServer(_ sender: Any!) {
        invalidateSession()
        super.deactivateServer(sender)
    }

    private func schedulePauseFlush(for client: IMKTextInput, clientObject: NSObject?) {
        guard tracker.pendingText.isEmpty == false else { return }
        let expectedVersion = tracker.version
        let context = ClientContext(client: client, object: clientObject)
        pauseWorkItem = DispatchWorkItem { [weak self, context] in
            guard let self else { return }
            Task { @MainActor in
                guard self.tracker.version == expectedVersion,
                      let completed = self.tracker.flushOnPause() else { return }
                self.requestCorrection(for: completed, context: context)
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.9, execute: pauseWorkItem!)
    }

    private func requestCorrection(for sentence: CompletedSentence, client: IMKTextInput, clientObject: NSObject?) {
        requestCorrection(for: sentence, context: ClientContext(client: client, object: clientObject))
    }

    private func requestCorrection(for sentence: CompletedSentence, context: ClientContext) {
        guard let configuration = try? loadConfiguration() else { return }
        requestID &+= 1
        let currentRequestID = requestID
        let expectedVersion = sentence.version
        let service = HTTPCorrectionService(configuration: configuration, apiKeyStore: keyStore)

        Task { [weak self, context] in
            do {
                let result = try await service.correct(CorrectionRequest(sentence: sentence.text))
                await MainActor.run {
                    guard let self,
                          self.requestID == currentRequestID,
                          self.tracker.version == expectedVersion,
                          self.tracker.pendingText.isEmpty,
                          self.sameClient(context.object),
                          result.correctedText != sentence.text else { return }
                    self.showSuggestion(result: result, sentence: sentence, context: context)
                }
            } catch {
                // Keep the committed text untouched. The next sentence or explicit retry can try again.
            }
        }
    }

    @MainActor
    private func showSuggestion(result: CorrectionResult, sentence: CompletedSentence, context: ClientContext) {
        suggestion?.orderOut(nil)
        suggestionClient = context.object
        suggestionRange = sentence.utf16Range
        suggestionVersion = sentence.version
        let panel = CorrectionSuggestionPanel(
            correctedText: result.correctedText,
            onReplace: { [weak self, context] in
                Task { @MainActor in
                    self?.applyReplacement(result.correctedText, context: context)
                }
            },
            onKeep: { [weak self] in
                Task { @MainActor in self?.clearSuggestion() }
            }
        )
        suggestion = panel

        var lineRect = NSRect.zero
        _ = context.client.attributes(forCharacterIndex: 0, lineHeightRectangle: &lineRect)
        let origin = NSPoint(x: lineRect.minX, y: lineRect.minY - panel.frame.height - 8)
        panel.setFrameOrigin(origin)
        panel.orderFrontRegardless()
    }

    @MainActor
    private func applyReplacement(_ text: String, context: ClientContext) {
        guard sameClient(context.object), tracker.pendingText.isEmpty, suggestionVersion == tracker.version,
              let range = suggestionRange else {
            clearSuggestion()
            return
        }
        context.client.insertText(text, replacementRange: range)
        clearSuggestion()
        tracker.reset()
    }

    private func loadConfiguration() throws -> AIConfiguration {
        let repository = AISettingsRepository(defaults: defaults, keychain: keyStore)
        guard let configuration = try repository.loadConfiguration() else {
            throw AIClientError.invalidConfiguration("尚未配置 AI 服务")
        }
        return configuration
    }

    private func sameClient(_ client: NSObject?) -> Bool {
        guard let client, let suggestionClient else { return false }
        return client === suggestionClient
    }

    @MainActor
    private func clearSuggestion() {
        suggestion?.orderOut(nil)
        suggestion = nil
        suggestionClient = nil
        suggestionRange = nil
        suggestionVersion = nil
    }

    private func invalidateSession() {
        pauseWorkItem?.cancel()
        requestID &+= 1
        Task { @MainActor [weak self] in self?.clearSuggestion() }
        tracker.reset()
    }

    private func isReturnKey(keyCode: Int, string: String) -> Bool {
        keyCode == 36 || keyCode == 76 || string == "\n" || string == "\r"
    }
}
