import AppKit
import LucidCore
import InputMethodKit
import os.log

/// Passes text through immediately, then rewrites a finished sentence into English.
@objc(LucidInputController)
public final class LucidInputController: IMKInputController, @unchecked Sendable {
    private final class ClientContext: @unchecked Sendable {
        let client: IMKTextInput

        init(client: IMKTextInput) {
            self.client = client
        }
    }

    private var tracker = SentenceTracker()
    private var requestID: UInt64 = 0
    private var pauseWorkItem: DispatchWorkItem?
    private var suggestion: CorrectionSuggestionPanel?
    private var pendingOriginal: String?
    private var pendingRange: NSRange?
    private var currentContext: ClientContext?
    private var sentenceOrigin: Int = 0
    private var isApplyingReplacement = false
    private var rewriteCache: [String: String] = [:]

    private let defaults: UserDefaults
    private let keyStore: KeychainAPIKeyStore
    private let logger = Logger(subsystem: "io.github.rdj.inputmethod.lucid", category: "input")
    private let notFoundRange = NSRange(location: NSNotFound, length: NSNotFound)
    private let finishedPauseInterval: TimeInterval = 2.0

    public override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        let suiteName = Bundle.main.object(forInfoDictionaryKey: "LucidAppGroup") as? String
            ?? "group.io.github.rdj.lucid"
        defaults = UserDefaults(suiteName: suiteName) ?? .standard
        let accessGroup = Bundle.main.object(forInfoDictionaryKey: "LucidKeychainAccessGroup") as? String
        keyStore = KeychainAPIKeyStore(accessGroup: accessGroup)
        super.init(server: server, delegate: delegate, client: inputClient)
    }

    public override func inputText(_ string: String?, key keyCode: Int, modifiers flags: Int, client sender: Any?) -> Bool {
        guard let string, let textClient = sender as? IMKTextInput else { return false }
        let context = ClientContext(client: textClient)
        currentContext = context

        if isReturnKey(keyCode: keyCode, string: string) {
            // Return is for the client (send / newline), not a sentence-end signal.
            pauseWorkItem?.cancel()
            requestID &+= 1
            if suggestion != nil { clearSuggestion() }
            tracker.reset()
            pendingOriginal = nil
            pendingRange = nil
            return false
        }

        if keyCode == 51 || string == "\u{8}" {
            requestID &+= 1
            if suggestion != nil { clearSuggestion() }
            pauseWorkItem?.cancel()
            if tracker.pendingText.isEmpty == false {
                tracker.deleteBackward()
                schedulePauseFlush(for: context)
            }
            return false
        }

        guard !string.isEmpty else { return false }

        if tracker.pendingText.isEmpty {
            let location = textClient.selectedRange().location
            sentenceOrigin = location == NSNotFound ? 0 : location
        }
        // Any new edit makes an in-flight result stale. The user remains in control;
        // a late response must not bring a panel back into the current sentence.
        requestID &+= 1
        if suggestion != nil { clearSuggestion() }
        textClient.insertText(string, replacementRange: notFoundRange)
        pauseWorkItem?.cancel()

        let completed = tracker.append(string)
        for sentence in completed {
            requestCorrection(for: sentence, context: context)
        }
        schedulePauseFlush(for: context)
        return true
    }

    public override func didCommand(by selector: Selector!, client sender: Any!) -> Bool {
        guard let selector else { return false }
        if selector == #selector(NSResponder.insertNewline(_:)) {
            pauseWorkItem?.cancel()
            requestID &+= 1
            if suggestion != nil { clearSuggestion() }
            tracker.reset()
            pendingOriginal = nil
            pendingRange = nil
            return false
        }
        if selector == #selector(NSResponder.deleteBackward(_:)) {
            requestID &+= 1
            if suggestion != nil { clearSuggestion() }
            pauseWorkItem?.cancel()
            if tracker.pendingText.isEmpty == false {
                tracker.deleteBackward()
                if let textClient = sender as? IMKTextInput {
                    schedulePauseFlush(for: ClientContext(client: textClient))
                }
            }
            return false
        }
        return false
    }

    public override func commitComposition(_ sender: Any!) {
        if isApplyingReplacement || suggestion?.replaceableText != nil {
            return
        }
        invalidateSession()
        super.commitComposition(sender)
    }

    public override func deactivateServer(_ sender: Any!) {
        if isApplyingReplacement || suggestion?.replaceableText != nil {
            return
        }
        invalidateSession()
        super.deactivateServer(sender)
    }

    private var hasPendingSentence: Bool {
        tracker.pendingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
    }

    private func schedulePauseFlush(for context: ClientContext) {
        guard hasPendingSentence else { return }
        let snapshot = tracker.pendingText
        guard SentenceCompletion.looksFinished(snapshot) else { return }
        let expectedVersion = tracker.version
        pauseWorkItem = DispatchWorkItem { [weak self] in
            guard let self else { return }
            Task { @MainActor in
                guard self.tracker.version == expectedVersion else { return }
                let text = self.tracker.pendingText.trimmingCharacters(in: .whitespacesAndNewlines)
                guard text.isEmpty == false, SentenceCompletion.looksFinished(text) else { return }
                let sentence = CompletedSentence(
                    text: text,
                    utf16Range: NSRange(location: 0, length: (text as NSString).length),
                    version: expectedVersion
                )
                self.requestCorrection(for: sentence, context: context)
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + finishedPauseInterval, execute: pauseWorkItem!)
    }

    private func requestCorrection(for sentence: CompletedSentence, context: ClientContext) {
        let original = sentence.text
        rememberOriginal(original, client: context.client)

        if let cached = rewriteCache[original], cached != original {
            handleResult(
                CorrectionResult(correctedText: cached),
                original: original,
                context: context
            )
            return
        }

        showStatus("正在改写成英文…", context: context)
        requestID &+= 1
        let currentRequestID = requestID
        let expectedVersion = sentence.version

        guard let configuration = try? loadConfiguration() else {
            showStatus("还没配置 AI。请打开 Lucid 填写服务地址和 Key，获取模型列表后选择一个模型。", context: context)
            return
        }

        let service = HTTPCorrectionService(configuration: configuration, apiKeyStore: keyStore)
        logger.info("correcting length=\(original.count, privacy: .public) — waiting for user choice")

        Task { [weak self] in
            do {
                let result = try await service.correct(CorrectionRequest(sentence: original))
                await MainActor.run {
                    guard let self, self.requestID == currentRequestID else { return }
                    // A result is only useful while the user has not continued editing.
                    // Never overwrite text automatically; the panel is the only commit path.
                    if self.tracker.version != expectedVersion { return }
                    if result.correctedText != original {
                        self.rewriteCache[original] = result.correctedText
                    }
                    self.handleResult(result, original: original, context: context)
                }
            } catch {
                await MainActor.run {
                    guard let self, self.requestID == currentRequestID else { return }
                    self.showStatus("改写失败：\(error.localizedDescription)", context: context)
                    self.logger.error("correction failed: \(error.localizedDescription, privacy: .public)")
                }
            }
        }
    }

    private func rememberOriginal(_ original: String, client: IMKTextInput) {
        pendingOriginal = original
        pendingRange = CommittedTextReplacement.range(
            originalUTF16Length: (original as NSString).length,
            selected: client.selectedRange(),
            origin: sentenceOrigin
        )
        logger.info("remembered original utf16=\(original.utf16.count, privacy: .public) loc=\(self.pendingRange?.location ?? -1, privacy: .public)")
    }

    private func handleResult(
        _ result: CorrectionResult,
        original: String,
        context: ClientContext,
    ) {
        let rewritten = result.correctedText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard rewritten.isEmpty == false else {
            showStatus("模型没有返回英文句子，原文已保留。", context: context)
            return
        }
        if rewritten == original {
            clearSuggestion()
            return
        }
        pendingOriginal = original
        // Preview only. The original text stays in the client until the user explicitly
        // clicks “使用英文” (or the equivalent action). This is intentionally the sole
        // path that calls replaceOriginal(_:with:client:).
        presentPanel(text: rewritten, originalText: original, replaceableText: rewritten, context: context, title: "英文建议（原文未修改）")
    }

    private func showStatus(_ text: String, context: ClientContext) {
        presentPanel(text: text, originalText: nil, replaceableText: nil, context: context, title: "Lucid")
    }

    private func presentPanel(
        text: String,
        originalText: String?,
        replaceableText: String?,
        context: ClientContext,
        title: String
    ) {
        suggestion?.orderOut(nil)
        currentContext = context
        if let originalText, originalText.isEmpty == false {
            pendingOriginal = originalText
        }
        let panel = CorrectionSuggestionPanel(
            title: title,
            displayText: text,
            originalText: originalText,
            replaceableText: replaceableText,
            onReplace: { [weak self] in
                _ = self?.commitSuggestionIfNeeded()
            },
            onKeep: { [weak self] in
                self?.keepOriginal()
            }
        )
        suggestion = panel
        panel.setFrameOrigin(panelOrigin(near: context.client, height: panel.frame.height))
        panel.orderFrontRegardless()
    }

    private func commitSuggestionIfNeeded() -> Bool {
        guard let panel = suggestion, let replacement = panel.replaceableText, let context = currentContext else {
            return false
        }
        let original = panel.originalText ?? pendingOriginal ?? ""
        guard original.isEmpty == false else { return false }

        // The suggestion is a preview of a specific piece of text. If the client
        // changed it while the panel was visible, never replace whatever happens
        // to be under the cursor now.
        if let range = pendingRange,
           let visible = substring(from: range, client: context.client),
           visible != original {
            logger.info("discarding suggestion because the original text changed")
            keepOriginal()
            return false
        }

        isApplyingReplacement = true
        replaceOriginal(original, with: replacement, client: context.client)
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        isApplyingReplacement = false
        clearSuggestion()
        return true
    }

    private func keepOriginal() {
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        clearSuggestion()
    }

    private func replaceOriginal(_ original: String, with replacement: String, client: IMKTextInput) {
        let originalLength = (original as NSString).length
        guard originalLength > 0 else {
            client.insertText(replacement, replacementRange: notFoundRange)
            updateOrigin(afterReplacing: replacement, client: client)
            return
        }

        let marked = client.markedRange()
        if marked.location != NSNotFound, marked.length > 0 {
            client.insertText(replacement, replacementRange: notFoundRange)
            updateOrigin(afterReplacing: replacement, client: client)
            return
        }

        let range = pendingRange ?? CommittedTextReplacement.range(
            originalUTF16Length: originalLength,
            selected: client.selectedRange(),
            origin: sentenceOrigin
        )

        client.setMarkedText(
            original as NSString,
            selectionRange: NSRange(location: originalLength, length: 0),
            replacementRange: range
        )
        let wrapped = client.markedRange()
        if wrapped.location != NSNotFound, wrapped.length > 0 {
            client.insertText(replacement, replacementRange: notFoundRange)
            updateOrigin(afterReplacing: replacement, client: client)
            logger.info("replaced via marked overlay")
            return
        }

        // Never insert first. Chat clients often ignore replacementRange and would produce "shenmeWhat?".
        deleteOriginal(original, preferredRange: range, client: client)
        client.insertText(replacement, replacementRange: notFoundRange)
        updateOrigin(afterReplacing: replacement, client: client)
        logger.info("replaced after deleting original")
    }

    private func deleteOriginal(_ original: String, preferredRange: NSRange, client: IMKTextInput) {
        let originalLength = (original as NSString).length

        if preferredRange.location != NSNotFound, preferredRange.length == originalLength {
            if let existing = substring(from: preferredRange, client: client), existing == original {
                client.insertText("", replacementRange: preferredRange)
                return
            }
            client.insertText("", replacementRange: preferredRange)
            if client.selectedRange().location == preferredRange.location {
                return
            }
        }

        let selected = client.selectedRange()
        if selected.location != NSNotFound, selected.location >= originalLength {
            let behindCursor = NSRange(location: selected.location - originalLength, length: originalLength)
            client.insertText("", replacementRange: behindCursor)
            if client.selectedRange().location == behindCursor.location {
                return
            }
            _ = deleteBackward(utf16Length: originalLength, client: client)
            return
        }

        _ = deleteBackward(utf16Length: originalLength, client: client)
    }

    @discardableResult
    private func deleteBackward(utf16Length: Int, client: IMKTextInput) -> Bool {
        guard utf16Length > 0 else { return true }
        let target = client as AnyObject
        let selector = NSSelectorFromString("deleteBackward:")
        if target.responds(to: selector) {
            for _ in 0..<utf16Length {
                _ = target.perform(selector, with: nil)
            }
            return true
        }
        var remaining = utf16Length
        var deleted = false
        while remaining > 0 {
            let selected = client.selectedRange()
            guard selected.location != NSNotFound, selected.location > 0 else { break }
            client.insertText("", replacementRange: NSRange(location: selected.location - 1, length: 1))
            remaining -= 1
            deleted = true
        }
        return deleted && remaining == 0
    }

    private func updateOrigin(afterReplacing replacement: String, client: IMKTextInput) {
        let cursor = client.selectedRange().location
        sentenceOrigin = cursor == NSNotFound ? sentenceOrigin + (replacement as NSString).length : cursor
    }

    private func substring(from range: NSRange, client: IMKTextInput) -> String? {
        guard range.location != NSNotFound, range.length > 0 else { return nil }
        return client.attributedSubstring(from: range)?.string
    }

    private func panelOrigin(near client: IMKTextInput, height: CGFloat) -> NSPoint {
        // IMKTextInput returns the candidate line rectangle in screen coordinates.
        // There is no inline composition in Lucid (text is committed immediately),
        // so the index must be zero. Passing the document's selectedRange location
        // makes some clients return a bogus rectangle at the screen's top-left.
        var lineRect = NSRect.zero
        _ = client.attributes(forCharacterIndex: 0, lineHeightRectangle: &lineRect)

        let screens = NSScreen.screens
        let fallbackScreen = NSScreen.main ?? screens.first
        let screen = screens.first(where: { screen in
            lineRect.width > 0 && lineRect.height > 0 && screen.visibleFrame.intersects(lineRect)
        }) ?? fallbackScreen
        let visibleFrame = screen?.visibleFrame ?? NSRect(x: 80, y: 80, width: 800, height: 600)
        let panelWidth = max(suggestion?.frame.width ?? 460, 320)
        let horizontalInset: CGFloat = 12
        let verticalInset: CGFloat = 12
        let minX = visibleFrame.minX + horizontalInset
        let maxX = max(minX, visibleFrame.maxX - panelWidth - horizontalInset)

        guard lineRect.width > 0,
              lineRect.height > 0,
              lineRect.origin.x.isFinite,
              lineRect.origin.y.isFinite,
              let screen,
              screen.visibleFrame.intersects(lineRect)
        else {
            return NSPoint(
                x: visibleFrame.midX - panelWidth / 2,
                y: visibleFrame.midY - height / 2
            )
        }

        var origin = NSPoint(
            x: min(max(lineRect.minX, minX), maxX),
            y: lineRect.minY - height - 10
        )
        if origin.y < visibleFrame.minY + verticalInset {
            origin.y = min(visibleFrame.maxY - height - verticalInset, lineRect.maxY + 10)
        }
        origin.y = min(max(origin.y, visibleFrame.minY + verticalInset), visibleFrame.maxY - height - verticalInset)
        return origin
    }

    private func loadConfiguration() throws -> AIConfiguration {
        let repository = AISettingsRepository(defaults: defaults, keychain: keyStore)
        guard let configuration = try repository.loadConfiguration() else {
            throw AIClientError.invalidConfiguration("尚未配置 AI 服务")
        }
        return configuration
    }

    private func clearSuggestion() {
        suggestion?.orderOut(nil)
        suggestion = nil
    }

    private func invalidateSession() {
        pauseWorkItem?.cancel()
        requestID &+= 1
        suggestion?.orderOut(nil)
        suggestion = nil
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        sentenceOrigin = 0
        currentContext = nil
        isApplyingReplacement = false
    }

    private func isReturnKey(keyCode: Int, string: String) -> Bool {
        keyCode == 36 || keyCode == 76 || string == "\n" || string == "\r"
    }
}
