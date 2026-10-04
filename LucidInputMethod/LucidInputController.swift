import AppKit
import LucidCore
import InputMethodKit
import os.log

/// Some IMK client proxies implement setSelectedRange: even though it is not
/// part of IMKTextInput. Calling it through an ObjC protocol preserves NSRange
/// its value ABI; NSObject.perform(_:with:) would pass an NSValue object instead.
@objc private protocol LucidSelectionSetting {
    func setSelectedRange(_ range: NSRange)
}

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
    private var pendingSelection: NSRange?
    private var currentContext: ClientContext?
    private var sentenceOrigin: Int = 0
    private var trackedOrigin: Int = 0
    private var isApplyingReplacement = false
    /// The current sentence is still a marked composition, not committed text.
    private var composing = false
    private var rewriteCache: [String: String] = [:]
    private var lastObservedKey = ""
    private var lastObservedKeyCode = Int.min
    private var lastObservedSource = ""
    private var lastObservedAt = Date.distantPast

    private let defaults: UserDefaults
    private let keyStore: DefaultsAPIKeyStore
    private let logger = Logger(subsystem: "io.github.rdj.inputmethod.lucid", category: "input")
    private let notFoundRange = NSRange(location: NSNotFound, length: NSNotFound)
    private let finishedPauseInterval: TimeInterval = 2.0

    public override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        let suiteName = Bundle.main.object(forInfoDictionaryKey: "LucidAppGroup") as? String
            ?? "group.io.github.rdj.lucid"
        defaults = UserDefaults(suiteName: suiteName) ?? .standard
        keyStore = DefaultsAPIKeyStore(defaults: defaults)
        super.init(server: server, delegate: delegate, client: inputClient)
    }

    public override func inputText(_ string: String?, key keyCode: Int, modifiers flags: Int, client sender: Any?) -> Bool {
        observeKey(string, keyCode: keyCode, modifiers: flags, client: sender, source: "inputText", insert: true)
    }

    public override func inputText(_ string: String?, client sender: Any?) -> Bool {
        observeKey(string, keyCode: -1, modifiers: 0, client: sender, source: "inputText", insert: true)
    }

    /// Some editor and WebView hosts deliver key events through IMK's
    /// `handleEvent:client:` entry point instead of `inputText:`. The Objective-C
    /// shim installs that selector at runtime and forwards it here so those hosts
    /// get the same pass-through and sentence tracking behavior.
    @objc(lucidHandle:client:)
    public func lucidHandle(_ event: NSEvent?, client sender: Any?) -> Bool {
        guard let event, event.type == .keyDown else { return false }
        let characters = event.characters ?? ""
        guard characters.isEmpty == false else { return false }
        return observeKey(
            characters,
            keyCode: Int(event.keyCode),
            modifiers: Int(event.modifierFlags.rawValue),
            client: sender,
            source: "handleEvent",
            insert: true
        )
    }

    /// Records a key for sentence detection. Printable keys are never consumed:
    /// the host inserts them. Only Escape is consumed, and only to dismiss the panel.
    private func observeKey(
        _ string: String?,
        keyCode: Int,
        modifiers flags: Int,
        client sender: Any?,
        source: String,
        insert: Bool
    ) -> Bool {
        let host = (sender as? IMKTextInput)?.bundleIdentifier() ?? "unknown"
        logger.notice("key source=\(source, privacy: .public) host=\(host, privacy: .public) keyCode=\(keyCode, privacy: .public) chars=\(string?.count ?? -1, privacy: .public)")
        guard let string else { return false }

        // A few hosts send the same physical key through both IMK entry points.
        // Insert and track it only once, otherwise a sentence like `nihao.` can
        // be duplicated or its terminator can be consumed by the second callback.
        let now = Date()
        if source != lastObservedSource,
           string == lastObservedKey,
           keyCode == lastObservedKeyCode,
           now.timeIntervalSince(lastObservedAt) < 0.08 {
            logger.debug("ignored duplicate key callback source=\(source, privacy: .public)")
            return insert
        }
        lastObservedKey = string
        lastObservedKeyCode = keyCode
        lastObservedSource = source
        lastObservedAt = now
        guard let textClient = sender as? IMKTextInput else {
            logger.error("input callback received a non-text client source=\(source, privacy: .public)")
            // Let the host insert the character instead of swallowing the key.
            return false
        }
        let context = ClientContext(client: textClient)
        currentContext = context

        if keyCode == 53 || string == "\u{1b}" {
            if suggestion?.replaceableText != nil {
                keepOriginal()
            } else {
                clearSuggestion()
            }
            return false
        }

        if isReturnKey(keyCode: keyCode, string: string) {
            pauseWorkItem?.cancel()
            tracker.reset()
            return false
        }

        if keyCode == 51 || string == "\u{8}" {
            pauseWorkItem?.cancel()
            composing = false
            if tracker.pendingText.isEmpty == false {
                tracker.deleteBackward()
                trackedOrigin = max(0, trackedOrigin - 1)
                schedulePauseFlush(for: context)
            }
            return false
        }

        guard string.isEmpty == false else { return false }
        let commandOrControl = Int(NSEvent.ModifierFlags.command.rawValue | NSEvent.ModifierFlags.control.rawValue)
        if flags & commandOrControl != 0 { return false }
        guard isInsertableText(string) else { return false }

        if tracker.pendingText.isEmpty {
            sentenceOrigin = trackedOrigin
        }
        if suggestion != nil { clearSuggestion() }
        pauseWorkItem?.cancel()
        // A new character means the previous asynchronous rewrite is stale.
        // In particular, pressing Enter or switching fields must not make an old
        // response replace text in the next sentence.
        requestID &+= 1

        if insert {
            textClient.insertText(string, replacementRange: notFoundRange)
        }
        composing = false
        trackedOrigin += (string as NSString).length
        pendingSelection = NSRange(location: trackedOrigin, length: 0)
        let completed = tracker.append(string)
        for sentence in completed {
            requestCorrection(for: sentence, context: context)
        }
        schedulePauseFlush(for: context)
        return insert
    }

    /// Updates the uncommitted composition. An empty draft clears it.
    private func refreshComposition(_ client: IMKTextInput) {
        let draft = tracker.pendingText
        composing = draft.isEmpty == false
        client.setMarkedText(
            draft,
            selectionRange: NSRange(location: (draft as NSString).length, length: 0),
            replacementRange: notFoundRange
        )
    }

    /// Selects the committed original and types over that selection. Sublime
    /// ignores replacement ranges but replaces the active selection, which is
    /// what its own status bar means by "6 characters selected".
    private func replaceSelected(_ original: String, with replacement: String, client: IMKTextInput) -> Bool {
        guard let range = pendingRange, range.length == (original as NSString).length else { return false }
        let target = client as AnyObject
        let select = NSSelectorFromString("setSelectedRange:")
        guard target.responds(to: select) else { return false }
        let value = NSValue(range: range)
        _ = target.perform(select, with: value)
        let selected = client.selectedRange()
        guard selected.location == range.location, selected.length == range.length else { return false }
        client.insertText(replacement, replacementRange: NSRange(location: NSNotFound, length: 0))
        let written = client.attributedSubstring(from: NSRange(location: range.location, length: (replacement as NSString).length))?.string
        return written == replacement
    }

    /// Turns the marked draft into committed text. This is the only insert that
    /// hosts such as Sublime Text reliably treat as replacing the composition
    /// rather than appending after it.
    private func commitComposition(_ client: IMKTextInput, text: String) {
        guard composing else { return }
        client.insertText(text, replacementRange: notFoundRange)
        composing = false
    }

    public override func activateServer(_ sender: Any!) {
        // A fresh client must not inherit the previous field's sentence, but it
        // also must not cancel a rewrite that has already been requested. The
        // request ID is the only thing that keeps that result from being dropped.
        pauseWorkItem?.cancel()
        suggestion?.orderOut(nil)
        suggestion = nil
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        pendingSelection = nil
        sentenceOrigin = 0
        trackedOrigin = 0
        composing = false
        super.activateServer(sender)
    }

    public override func didCommand(by selector: Selector!, client sender: Any!) -> Bool {
        guard let selector else { return false }
        if selector == #selector(NSResponder.insertNewline(_:)) {
            pauseWorkItem?.cancel()
            // Enter commits the line in many hosts immediately after the sentence
            // callback. Do not invalidate an in-flight correction here; doing so
            // made the final sentence silently stop converting when the user
            // pressed Return.
            tracker.reset()
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
        guard isApplyingReplacement == false else { return }
        // WeChat asks IMK to commit as soon as a sentence ends. That is not an
        // app switch. Keep the suggestion; only clear the unfinished draft.
        pauseWorkItem?.cancel()
        tracker.reset()
        super.commitComposition(sender)
    }

    public override func deactivateServer(_ sender: Any!) {
        guard isApplyingReplacement == false else { return }
        // InputMethodKit deactivates the controller when a host commits a line or
        // the focus briefly moves to the suggestion panel. Reset only the live
        // tracker; invalidateSession() would bump requestID and discard the
        // correction that is still on the way.
        pauseWorkItem?.cancel()
        tracker.reset()
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
                // Consume the paused sentence. Leaving it in the tracker makes the
                // next key join the old sentence and can suppress the next rewrite.
                guard let sentence = self.tracker.flushOnPause(),
                      SentenceCompletion.looksFinished(sentence.text) else { return }
                self.requestCorrection(for: sentence, context: context)
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + finishedPauseInterval, execute: pauseWorkItem!)
    }

    private func recoveredSentence(_ sentence: CompletedSentence, client: IMKTextInput) -> CompletedSentence {
        let selected = client.selectedRange()
        guard selected.location != NSNotFound, selected.location >= 0 else { return sentence }
        // Read only up to the caret. A full-document read stalls some clients.
        let lookbehind = min(selected.location, 1024)
        let start = selected.location - lookbehind
        let document = client.attributedSubstring(from: NSRange(location: start, length: lookbehind))?.string
        let absoluteSelection = NSRange(location: lookbehind, length: 0)
        if let recovered = CommittedTextReplacement.sentenceOnCurrentLine(
            document: document,
            selected: absoluteSelection
        ) {
            let absolute = NSRange(location: start + recovered.range.location, length: recovered.range.length)
            pendingRange = absolute
            logger.info("recovered current line utf16=\(recovered.text.utf16.count, privacy: .public)")
            return CompletedSentence(text: recovered.text, utf16Range: absolute, version: sentence.version)
        }
        guard let recovered = CommittedTextReplacement.sentenceBeforeCursor(
            document: document,
            selected: absoluteSelection,
            tracked: sentence.text
        ) else {
            return sentence
        }
        let absolute = NSRange(location: start + recovered.range.location, length: recovered.range.length)
        pendingRange = absolute
        logger.info("recovered pasted context utf16=\(recovered.text.utf16.count, privacy: .public)")
        return CompletedSentence(text: recovered.text, utf16Range: absolute, version: sentence.version)
    }

    private func requestCorrection(for sentence: CompletedSentence, context: ClientContext) {
        // Use the keystrokes we just saw. Reading the document here stalls Codex
        // and WeChat, and a stalled key callback is exactly "the keyboard is dead".
        let original = sentence.text
        rememberOriginal(original, range: sentence.utf16Range, client: context.client)

        if let cached = rewriteCache[original], cached != original {
            handleResult(
                CorrectionResult(correctedText: cached),
                original: original,
                context: context
            )
            return
        }

        requestID &+= 1
        let currentRequestID = requestID

        guard let configuration = try? loadConfiguration() else {
            showStatus("还没配置 AI。请打开 Lucid 填写服务地址和 Key，获取模型列表后选择一个模型。", context: context)
            return
        }
        guard let apiKey = AISettingsRepository(defaults: defaults, keyStore: keyStore).sharedAPIKey() else {
            showStatus("读取不了 API Key，原文已保留。请打开 Lucid，重新填写 Key 并保存。", context: context)
            return
        }

        showStatus("正在改写成英文…", context: context)
        let service = HTTPCorrectionService(configuration: configuration, apiKeyStore: InMemoryAPIKeyStore(apiKey))
        let started = Date()
        logger.info("correcting length=\(original.count, privacy: .public) — waiting for user choice")

        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result = try await service.correct(CorrectionRequest(sentence: original))
                guard self.requestID == currentRequestID else {
                    self.logger.error("dropped correction after \(Date().timeIntervalSince(started), privacy: .public)s because a newer request replaced it")
                    return
                }
                // Do not also require the sentence tracker version. activateServer
                // resets that version when focus moves, which previously discarded a
                // successful rewrite before its panel could appear.
                if result.correctedText != original {
                    self.rewriteCache[original] = result.correctedText
                }
                self.handleResult(result, original: original, context: context)
            } catch {
                guard self.requestID == currentRequestID else { return }
                self.showStatus("改写失败：\(error.localizedDescription)", context: context)
                self.logger.error("correction failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    private func rememberOriginal(_ original: String, range: NSRange, client: IMKTextInput) {
        pendingOriginal = original
        pendingSelection = nil
        let length = (original as NSString).length
        // `recoveredSentence` reads the range straight out of the document, which
        // is exact. Only fall back to the session-relative origin when the host
        // gave us nothing usable, because that origin drifts when focus moves.
        let location = range.location != NSNotFound ? range.location : max(0, sentenceOrigin)
        pendingRange = NSRange(location: location, length: length)
        sentenceOrigin = location + length
        logger.info("remembered typed utf16=\(length, privacy: .public) loc=\(location, privacy: .public)")
    }

    private func handleResult(
        _ result: CorrectionResult,
        original: String,
        context: ClientContext
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
        // Do not mark the committed sentence here. Sublime treats a ranged
        // setMarkedText as another insertion, which is why the original appeared
        // twice before the button was even clicked.
        composing = false
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
        // Do not call selectedRange() here. Sublime Text answers that query by
        // waiting on the same key-event turn that is showing this panel, so the
        // correction task never gets to run and the status panel stays forever.
        // The caret was already tracked while the characters were inserted.
        let panel = CorrectionSuggestionPanel(
            title: title,
            displayText: text,
            originalText: originalText,
            replaceableText: replaceableText,
            cancelsRequestOnDismiss: replaceableText != nil,
            onReplace: { [weak self] in
                _ = self?.commitSuggestionIfNeeded()
            },
            onKeep: { [weak self] in
                self?.keepOriginal()
            }
        )
        suggestion = panel
        panel.setFrameOrigin(panelOrigin(width: panel.frame.width, height: panel.frame.height))
        panel.orderFrontRegardless()
        logger.info("showing suggestion panel chars=\(text.count, privacy: .public)")
    }

    private func commitSuggestionIfNeeded() -> Bool {
        guard let panel = suggestion, let replacement = panel.replaceableText, let context = currentContext else {
            return false
        }
        let original = panel.originalText ?? pendingOriginal ?? ""
        guard original.isEmpty == false else { return false }

        isApplyingReplacement = true
        defer { isApplyingReplacement = false }
        // Do not try to select the old sentence and blindly call insertText first.
        // WeChat reports/accepts the selection inconsistently: it can ignore the
        // selected range and append the English after `nihao.`. The old helper
        // returned false after that append, so the fallback ran with a duplicate
        // already in the document. Use the verified replacement pipeline as the
        // only write path; it deletes from the tracked sentence end and verifies
        // the result before reporting success.
        guard replaceOriginal(original, with: replacement, client: context.client) else {
            return false
        }
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        pendingSelection = nil
        sentenceOrigin = 0
        trackedOrigin = 0
        clearSuggestion()
        return true
    }

    /// Replaces already-committed text entirely through the input-method
    /// connection — no Accessibility grant, no synthetic key events.
    ///
    /// The one mechanism every host that can type Chinese supports is the
    /// marked-text overlay: `setMarkedText(_:selectionRange:replacementRange:)`
    /// with a valid range makes the host swap that range for the (underlined)
    /// composition, and `insertText(_:)` then commits the English over it.
    /// WeChat and DingTalk ignore `replacementRange` for ordinary inserts but
    /// cannot ignore it here, because their own Pinyin composition depends on it.
    ///
    /// A host that still ignores the range must never corrupt the document, so
    /// the range is verified by reading it back first, and the overlay is only
    /// committed once the host reports the composition parked exactly over it.
    private func replaceOriginal(_ original: String, with replacement: String, client: IMKTextInput) -> Bool {
        let originalUTF16Length = (original as NSString).length
        guard originalUTF16Length > 0, replacement.isEmpty == false else { return false }
        let context = ClientContext(client: client)

        let liveSelection = client.selectedRange()
        // Clicking the suggestion panel moves focus out of editors such as
        // Sublime Text. Their input client then reports a caret at 0 even though
        // the sentence is still where the user left it. Keep the caret captured
        // when the suggestion appeared.
        let selection = usableSelection(liveSelection) ?? pendingSelection ?? liveSelection
        let storedRange = pendingRange?.length == originalUTF16Length ? pendingRange : nil
        let readText: (NSRange) -> String? = { [client] range in client.attributedSubstring(from: range)?.string }

        // Prefer a range we can prove holds the original. The caret-derived range
        // is the common case; the backwards search covers hosts that report the
        // caret next to (not exactly at) the end of the sentence, which would
        // otherwise make strict verification reject a perfectly good rewrite.
        let caretRange = CommittedTextReplacement.rangeBehindCaret(
            originalUTF16Length: originalUTF16Length,
            selected: selection
        )
        let searchedRange = CommittedTextReplacement.rangeSearchingBackwards(
            original: original,
            selected: selection,
            readText: readText
        )
        let candidates = [caretRange, searchedRange, storedRange].compactMap { $0 }
        // A byte-exact match is the norm. Failing that, accept a range whose text
        // differs only by letter case: TextEdit, Notes and most web views
        // auto-capitalise the sentence after we commit it (`nihao?` becomes
        // `Nihao?`), and refusing on that difference is why a perfectly valid
        // rewrite used to report "原文已经被修改".
        let strictRange = candidates.first { readText($0) == original }
        let verifiedRange = strictRange ?? candidates.first {
            CommittedTextReplacement.matchesLoosely(readText($0), original: original)
        }
        // Commit against the text that is actually in the document, so a host
        // that changed the case under us is still replaced as a whole.
        let documentOriginal = verifiedRange.flatMap(readText) ?? original
        // A host that answers no reads cannot be verified at all. Remember that,
        // because it is exactly the host kind that only accepts a blind rewrite.
        let hostIsReadable = hostAnswersReads(client: client, selected: selection)
        let host = client.bundleIdentifier() ?? "unknown"

        logger.notice("replace host=\(host, privacy: .public) caret=\(selection.location, privacy: .public) readable=\(hostIsReadable, privacy: .public) verified=\(verifiedRange != nil, privacy: .public) exact=\(strictRange != nil, privacy: .public)")

        // When the host *can* be read and the original is not there, the document
        // already changed under us. Refuse instead of overwriting the wrong text.
        if hostIsReadable, verifiedRange == nil {
            logger.error("original no longer matches behind the caret; not replacing host=\(host, privacy: .public) caret=\(selection.location, privacy: .public)")
            showStatus("原文已经被修改，所以没有替换。", context: context)
            return false
        }

        // The range we are allowed to overwrite: a proven one when the host can
        // be read, otherwise the caret-derived guess (the only thing left).
        let fallbackRange = caretRange ?? (storedRange?.length == originalUTF16Length ? storedRange : nil)
        guard let range = verifiedRange ?? fallbackRange,
              range.location != NSNotFound,
              range.length == originalUTF16Length,
              readText(range).map({ $0 == documentOriginal }) ?? true else {
            logger.error("no range to overwrite host=\(host, privacy: .public) caret=\(selection.location, privacy: .public)")
            showStatus(replacementFailureMessage(host: host, readable: hostIsReadable), context: context)
            return false
        }

        let useCapturedCaretForDeletion = host.lowercased().contains("wechat") || host.lowercased().contains("tencent")
        let replacementClient = replacementClient(
            host: host,
            client: client,
            readText: readText,
            useCapturedCaretForDeletion: useCapturedCaretForDeletion
        )
        // WeChat/Tencent proxies ignore every replacement range. Never try a
        // ranged insert or marked-text fallback there: if the delete command
        // cannot be verified, leave the original untouched instead of producing
        // the familiar `nihao.Hello.` duplicate.
        let outcome = useCapturedCaretForDeletion
            ? CommittedTextReplacement.replaceRangeByDeletion(
                original: documentOriginal,
                replacement: replacement,
                range: range,
                client: replacementClient
            )
            : CommittedTextReplacement.replaceRange(
                original: documentOriginal,
                replacement: replacement,
                range: range,
                client: replacementClient
            )
        switch outcome {
        case .replaced:
            logger.notice("replaced host=\(host, privacy: .public) loc=\(range.location, privacy: .public)")
            return true
        case .stale:
            showStatus("原文已经被修改，所以没有替换。", context: context)
            return false
        case .unverified:
            showStatus(replacementFailureMessage(host: host, readable: hostIsReadable), context: context)
            return false
        }
    }

    /// Bridges a live `IMKTextInput` connection to the pure commit logic so the
    /// same mechanism can also be exercised in unit tests.
    private func replacementClient(
        host: String,
        client: IMKTextInput,
        readText: @escaping (NSRange) -> String?,
        useCapturedCaretForDeletion: Bool
    ) -> CommittedTextReplacement.Client {
        // Chromium text proxies used by WeChat often return NSNotFound after a
        // non-activating panel is clicked, even though they still accept the
        // editing commands. The replacement algorithm needs a caret that moves
        // after each delete; returning NSNotFound made the deletion path abort
        // immediately and fall through to insertText(_:replacementRange:), which
        // WeChat ignores and therefore appended `Hello.` after `nihao.`.
        var virtualSelection = pendingSelection
            ?? pendingRange.map { NSRange(location: $0.location + $0.length, length: 0) }
            ?? NSRange(location: NSNotFound, length: NSNotFound)

        return CommittedTextReplacement.Client(
            readText: readText,
            selectedRange: { [client] in
                if useCapturedCaretForDeletion {
                    return virtualSelection
                }
                return client.selectedRange()
            },
            setMarkedText: { [client] text, selectionRange, replacementRange in
                client.setMarkedText(
                    text,
                    selectionRange: selectionRange,
                    replacementRange: replacementRange
                )
            },
            markedRange: { [client] in client.markedRange() },
            insertText: { [client] text, replacementRange in
                client.insertText(text, replacementRange: replacementRange)
            },
            deleteBackward: { [client] in
                let caret = virtualSelection.location == NSNotFound ? nil : virtualSelection.location
                self.deleteBackward(client: client, capturedCaret: caret)
                if useCapturedCaretForDeletion, let caret, caret > 0 {
                    virtualSelection = NSRange(location: caret - 1, length: 0)
                }
            },
            setSelection: { [client] range in
                self.moveCaret(to: range.location, client: client)
                if useCapturedCaretForDeletion {
                    virtualSelection = NSRange(location: range.location, length: range.length)
                }
            },
            trace: { [logger] message in
                logger.notice("replace \(message, privacy: .public) host=\(host, privacy: .public)")
            }
        )
    }

    /// A caret the host still reports after the suggestion panel took focus.
    /// Location 0 with no selection is the stale value Sublime returns once the
    /// editor is no longer first responder; it must not override the caret we
    /// captured while the user was still in the sentence.
    private func usableSelection(_ selection: NSRange) -> NSRange? {
        guard selection.location != NSNotFound, selection.location >= 0 else { return nil }
        if selection.location == 0, selection.length == 0 { return nil }
        return selection
    }

    /// Moves the caret without inserting. Prefer the text client's direct
    /// selection API: WeChat keeps the IMK connection alive after the suggestion
    /// panel takes focus, but its responder movement commands are not guaranteed
    /// to move the proxy caret. Falling back to moveLeft:/moveRight: keeps this
    /// compatible with editors that expose only those commands.
    @discardableResult
    private func setSelectedRange(_ range: NSRange, client: IMKTextInput) -> Bool {
        guard range.location != NSNotFound, range.location >= 0 else { return false }
        let target = client as AnyObject
        let selector = NSSelectorFromString("setSelectedRange:")
        guard target.responds(to: selector) else { return false }
        if let setter = target as? LucidSelectionSetting {
            setter.setSelectedRange(range)
        } else {
            // Keep the old fallback for unusual proxies that expose the selector
            // without being bridgeable to the private protocol.
            _ = target.perform(selector, with: NSValue(range: range))
        }
        let selected = client.selectedRange()
        let moved = selected.location == range.location && selected.length == range.length
        logger.notice("setSelectedRange requested=\(range.location, privacy: .public):\(range.length, privacy: .public) actual=\(selected.location, privacy: .public):\(selected.length, privacy: .public) moved=\(moved, privacy: .public)")
        return moved
    }

    private func moveCaret(to location: Int, client: IMKTextInput) {
        guard location != NSNotFound, location >= 0 else { return }
        let desired = NSRange(location: location, length: 0)
        if setSelectedRange(desired, client: client) { return }

        let target = client as AnyObject
        let moveRight = NSSelectorFromString("moveRight:")
        let moveLeft = NSSelectorFromString("moveLeft:")
        guard target.responds(to: moveRight), target.responds(to: moveLeft) else { return }
        let current = client.selectedRange().location
        guard current != NSNotFound else { return }
        let steps = location - current
        let selector = steps >= 0 ? moveRight : moveLeft
        for _ in 0..<abs(steps) {
            _ = target.perform(selector, with: nil)
            if client.selectedRange().location == location { break }
        }
    }

    /// One backwards delete through the host's own editing command. Hosts that do
    /// not expose `deleteBackward:` get an empty ranged insert one character
    /// behind the caret, which some AppKit clients treat as a delete.
    private func deleteBackward(client: IMKTextInput, capturedCaret: Int? = nil) {
        let target = client as AnyObject
        let selector = NSSelectorFromString("deleteBackward:")
        if target.responds(to: selector) {
            _ = target.perform(selector, with: nil)
            return
        }
        // Some proxies do not expose deleteBackward:, but still honour a
        // one-character replacement range. Prefer the caret captured before the
        // suggestion panel took focus; selectedRange() may now be NSNotFound.
        let caret = capturedCaret ?? client.selectedRange().location
        guard caret != NSNotFound, caret > 0 else { return }
        client.insertText("", replacementRange: NSRange(location: caret - 1, length: 1))
    }

    private func replacementFailureMessage(host: String, readable: Bool) -> String {
        if readable {
            return "没能替换原文。这个应用不接受输入法的范围替换，原文已保留。"
        }
        // Chat clients expose no readable text, so this is the honest answer:
        // the rewrite failed, nothing was inserted, and the original is intact.
        return "这个输入框没能完成替换，原文已保留。请把光标放到句尾再点一次。"
    }

    private func keepOriginal() {
        requestID &+= 1
        pauseWorkItem?.cancel()
        tracker.reset()
        pendingOriginal = nil
        pendingRange = nil
        pendingSelection = nil
        clearSuggestion()
    }

    private func updateOrigin(afterReplacing replacement: String, client: IMKTextInput) {
        let cursor = client.selectedRange().location
        sentenceOrigin = cursor == NSNotFound ? sentenceOrigin + (replacement as NSString).length : cursor
    }

    private func substring(from range: NSRange, client: IMKTextInput) -> String? {
        guard range.location != NSNotFound, range.length > 0 else { return nil }
        return client.attributedSubstring(from: range)?.string
    }

    /// True when the host answers *any* substring read around the caret. Used to
    /// decide whether a rewrite failure is "the text changed" (refuse) or "this
    /// host exposes nothing" (only a caret-blind rewrite can ever work).
    ///
    /// Several ranges are attempted because hosts disagree at the document
    /// edges: a read that runs one character past the end returns nil on some,
    /// while others clamp it, so a single probe is not conclusive.
    private func hostAnswersReads(client: IMKTextInput, selected: NSRange) -> Bool {
        guard selected.location != NSNotFound, selected.location >= 0 else { return false }
        let lookbehind = min(selected.location, 32)
        let start = selected.location - lookbehind
        let probes = [lookbehind + 8, lookbehind, min(lookbehind, 1)]
            .filter { $0 > 0 }
            .map { NSRange(location: start, length: $0) }
            + [NSRange(location: selected.location, length: 1)]
        return probes.contains { client.attributedSubstring(from: $0) != nil }
    }

    private func panelOrigin(width: CGFloat, height: CGFloat) -> NSPoint {
        // Do not ask the host for the caret rectangle while handling its key
        // event: Sublime Text deadlocks on that query. Place the panel under
        // the front editor window instead, which is where the sentence was typed.
        let panelWidth = width
        let screens = NSScreen.screens
        let mouse = NSEvent.mouseLocation
        let screen = screens.first { $0.frame.contains(mouse) } ?? NSScreen.main ?? screens.first
        let visibleFrame = screen?.visibleFrame ?? NSRect(x: 80, y: 80, width: 800, height: 600)
        let inset: CGFloat = 28
        // Keep the panel in the screen corner, away from the caret and the
        // editor's top-left text. The previous position covered the sentence.
        return NSPoint(
            x: visibleFrame.maxX - panelWidth - inset,
            y: visibleFrame.maxY - height - inset
        )
    }

    private func loadConfiguration() throws -> AIConfiguration {
        let repository = AISettingsRepository(defaults: defaults, keyStore: keyStore)
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
        pendingSelection = nil
        sentenceOrigin = 0
        trackedOrigin = 0
        composing = false
        currentContext = nil
        isApplyingReplacement = false
    }

    private func isReturnKey(keyCode: Int, string: String) -> Bool {
        keyCode == 36 || keyCode == 76 || string == "\n" || string == "\r"
    }

    /// Letters, digits, punctuation, and whitespace. Excludes controls and
    /// private-use function-key characters that IMK sometimes delivers as text.
    private func isInsertableText(_ string: String) -> Bool {
        guard string.isEmpty == false else { return false }
        return string.unicodeScalars.allSatisfy { scalar in
            if scalar.value < 0x20 || scalar.value == 0x7F { return false }
            if scalar.value >= 0xF700 && scalar.value <= 0xF8FF { return false }
            return true
        }
    }
}
