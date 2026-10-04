import Foundation

public struct CompletedSentence: Equatable, Sendable {
    public let text: String
    public let utf16Range: NSRange
    public let version: UInt64

    public init(text: String, utf16Range: NSRange, version: UInt64) {
        self.text = text
        self.utf16Range = utf16Range
        self.version = version
    }
}

/// Tracks only text received during the active input-method session; callers must discard it on focus changes.
public struct SentenceTracker: Sendable {
    public private(set) var pendingText = ""
    public private(set) var version: UInt64 = 0
    private var committedUTF16Length = 0
    private var periodAwaitingLookahead = false

    public init() {}

    /// Records text already inserted into the client and returns sentences ended by explicit punctuation/newlines.
    public mutating func append(_ text: String) -> [CompletedSentence] {
        var completed: [CompletedSentence] = []
        for character in text {
            if periodAwaitingLookahead {
                periodAwaitingLookahead = false
                if character.isNumber {
                    pendingText.append(character)
                    version &+= 1
                    continue
                }
                if let sentence = finishPending() { completed.append(sentence) }
            }

            let previous = pendingText.last
            pendingText.append(character)
            version &+= 1

            if character == ".", previous?.isNumber == true {
                periodAwaitingLookahead = true
                continue
            }
            if isTerminator(character), let sentence = finishPending() {
                completed.append(sentence)
            }
        }
        return completed
    }

    /// Returns the current sentence after an inactivity signal. The caller owns the debounce timer.
    public mutating func flushOnPause() -> CompletedSentence? {
        periodAwaitingLookahead = false
        return finishPending()
    }

    /// Invalidates any outstanding result after a deletion or an externally observed text edit.
    public mutating func invalidatePendingText() {
        pendingText = ""
        periodAwaitingLookahead = false
        version &+= 1
    }

    public mutating func reset() {
        pendingText = ""
        committedUTF16Length = 0
        periodAwaitingLookahead = false
        version &+= 1
    }

    public mutating func deleteBackward() {
        guard !pendingText.isEmpty else { return }
        pendingText.removeLast()
        periodAwaitingLookahead = false
        version &+= 1
    }

    private mutating func finishPending() -> CompletedSentence? {
        let sentence = pendingText.trimmingCharacters(in: .whitespacesAndNewlines)
        defer {
            committedUTF16Length += pendingText.utf16.count
            pendingText = ""
        }
        guard !sentence.isEmpty else { return nil }
        let leadingWhitespace = pendingText.prefix { $0.isWhitespace }
        return CompletedSentence(
            text: sentence,
            utf16Range: NSRange(location: committedUTF16Length + leadingWhitespace.utf16.count, length: sentence.utf16.count),
            version: version
        )
    }

    private func isTerminator(_ character: Character) -> Bool {
        ".!?。？！".contains(character)
    }
}


/// Computes the UTF-16 range of already-committed original text that should be overwritten.
public enum CommittedTextReplacement: Sendable {
    /// Range of text ending at the caret. Unknown carets must not fall back to
    /// document location 0: clients that ignore ranges would then append instead.
    /// A non-empty selection is the whole sentence only when the host selected
    /// exactly that sentence; otherwise the caret is at the selection's end.
    public static func rangeBehindCaret(originalUTF16Length: Int, selected: NSRange) -> NSRange? {
        let length = originalUTF16Length
        guard length > 0, selected.location != NSNotFound, selected.location >= 0 else { return nil }
        if selected.length == length {
            return selected
        }
        let caret = selected.length == 0 ? selected.location : NSMaxRange(selected)
        guard caret >= length else { return nil }
        return NSRange(location: caret - length, length: length)
    }

    /// Last occurrence of `original` at or just before the caret, located inside
    /// a window read *back from the host* rather than assumed from the caret.
    ///
    /// Clients disagree about where the caret sits relative to a finished
    /// sentence (some report it one character behind the terminator, some one
    /// ahead, some never move it after a range delete), so the caret is only a
    /// hint here: the matched text itself is the source of truth. Returns `nil`
    /// when the host gives us no readable text at all, which is the signal that
    /// the caller must fall back to a caret-only rewrite.
    public static func rangeSearchingBackwards(
        original: String,
        selected: NSRange,
        windowLength: Int = 1024,
        readText: (NSRange) -> String?
    ) -> NSRange? {
        let length = original.utf16.count
        guard length > 0,
              selected.location != NSNotFound,
              selected.location >= 0,
              windowLength > 0 else { return nil }
        let lookbehind = min(selected.location, windowLength)
        let start = selected.location - lookbehind
        // Read one character past the caret: some hosts lag the caret behind a
        // just-typed terminator, so the final character lives after the caret.
        // Hosts that refuse an out-of-bounds read get a second try without it.
        let window = readText(NSRange(location: start, length: lookbehind + 1))
            ?? readText(NSRange(location: start, length: lookbehind))
        guard let window else { return nil }
        let nsWindow = window as NSString
        guard nsWindow.length > 0 else { return nil }

        let caret = min(max(selected.location - start, 0), nsWindow.length)
        // Allow a match ending one character past the reported caret, because
        // some hosts lag the caret behind a just-typed terminator.
        let upperBound = min(nsWindow.length, caret + 1)
        guard upperBound >= length else { return nil }
        let found = nsWindow.range(
            of: original,
            options: .backwards,
            range: NSRange(location: 0, length: upperBound)
        )
        guard found.location != NSNotFound else { return nil }
        return NSRange(location: start + found.location, length: found.length)
    }

    public static func range(
        originalUTF16Length: Int,
        selected: NSRange,
        origin: Int
    ) -> NSRange {
        let length = max(0, originalUTF16Length)
        if selected.location != NSNotFound, selected.location >= length {
            return NSRange(location: selected.location - length, length: length)
        }
        if origin != NSNotFound, origin >= 0 {
            return NSRange(location: origin, length: length)
        }
        return NSRange(location: 0, length: length)
    }

    public enum Outcome: Equatable {
        case replaced
        case stale
        case unverified
    }

    /// A minimal view of an `NSTextInputClient`-style host, so the commit logic
    /// can be exercised in tests without a live input method connection.
    public struct Client {
        public var readText: (NSRange) -> String?
        public var selectedRange: () -> NSRange
        public var setMarkedText: ((String, NSRange, NSRange) -> Void)?
        public var markedRange: (() -> NSRange)?
        public var insertText: (String, NSRange) -> Void
        public var deleteBackward: (() -> Void)?
        /// Moves the caret without inserting. Editors that lose focus still
        /// accept this, which is how a click on the suggestion panel is undone.
        public var setSelection: ((NSRange) -> Void)?
        /// Optional diagnostic hook so callers can log which host path worked.
        public var trace: ((String) -> Void)?

        public init(
            readText: @escaping (NSRange) -> String?,
            selectedRange: @escaping () -> NSRange,
            setMarkedText: ((String, NSRange, NSRange) -> Void)? = nil,
            markedRange: (() -> NSRange)? = nil,
            insertText: @escaping (String, NSRange) -> Void,
            deleteBackward: (() -> Void)? = nil,
            setSelection: ((NSRange) -> Void)? = nil,
            trace: ((String) -> Void)? = nil
        ) {
            self.readText = readText
            self.selectedRange = selectedRange
            self.setMarkedText = setMarkedText
            self.markedRange = markedRange
            self.insertText = insertText
            self.deleteBackward = deleteBackward
            self.setSelection = setSelection
            self.trace = trace
        }
    }

    /// True when the host's text is the recorded original modulo letter case.
    ///
    /// Apps that auto-capitalise (TextEdit, Notes, and most web views) turn
    /// `nihao?` into `Nihao?` after the keystroke, so the text we recorded never
    /// matches byte-for-byte even though it is the same sentence at the same
    /// length. Treating that as "the original" keeps the rewrite from refusing
    /// on a difference we are about to overwrite anyway.
    public static func matchesLoosely(_ read: String?, original: String) -> Bool {
        guard let read, read.utf16.count == original.utf16.count else { return false }
        return read.caseInsensitiveCompare(original) == .orderedSame
    }

    /// Picks the candidate range whose text really is `original`. WeChat reports
    /// the caret one past the sentence, so the caret-derived range can be off by
    /// one character; the precisely recovered range has to win in that case.
    public static func preferredRange(
        original: String,
        candidates: [NSRange],
        readText: (NSRange) -> String?
    ) -> NSRange? {
        let usable = candidates.filter {
            $0.location != NSNotFound && $0.location >= 0 && $0.length == original.utf16.count
        }
        if let matching = usable.first(where: { readText($0) == original }) {
            return matching
        }
        return usable.first
    }

    /// Commits `replacement` over the already-committed `original` at `range`,
    /// trying every host mechanism and verifying the document afterwards.
    ///
    /// WeChat ignores `replacementRange` for both `setMarkedText(_:)` and
    /// `insertText(_:)`, so a naive commit appends the English after the original
    /// instead of replacing it. Every strategy here therefore reads the text back
    /// and only reports success when the range really holds the replacement.
    public static func replaceRange(
        original: String,
        replacement: String,
        range: NSRange,
        client: Client
    ) -> Outcome {
        guard !original.isEmpty, !replacement.isEmpty,
              range.location != NSNotFound, range.location >= 0,
              range.length == original.utf16.count,
              range.location <= Int.max - range.length,
              range.location <= Int.max - replacement.utf16.count else { return .stale }
        guard client.readText(range) == original else { return .stale }

        // Delete first. A host such as Sublime Text ignores replacement ranges
        // and would otherwise append the English after the original, then report
        // failure while leaving both copies in the document.
        if replaceUsingDeletion(original: original, replacement: replacement, range: range, client: client) {
            return .replaced
        }
        if replaceUsingMarkedText(original: original, replacement: replacement, range: range, client: client) {
            return .replaced
        }
        return .unverified
    }

    /// Commits `replacement` by deleting backwards from the caret and retyping,
    /// without ever touching the marked-text overlay. Some hosts (chat clients)
    /// ignore every `replacementRange` but still honour the delete command, so
    /// this is the safe fallback when the overlay would append instead.
    public static func replaceRangeByDeletion(
        original: String,
        replacement: String,
        range: NSRange,
        client: Client
    ) -> Outcome {
        guard !original.isEmpty, !replacement.isEmpty,
              range.location != NSNotFound, range.location >= 0,
              range.length == original.utf16.count,
              range.location <= Int.max - range.length,
              range.location <= Int.max - replacement.utf16.count else { return .stale }
        // Some chat/WebView clients intentionally do not expose their text
        // through IMKTextInput. In that case `nil` means "unreadable", not
        // "stale"; the caller has already classified the host as unreadable
        // and the deletion path is the only safe way to replace it.
        let readableOriginal = client.readText(range)
        guard readableOriginal == original || readableOriginal == nil else { return .stale }
        if replaceUsingDeletion(original: original, replacement: replacement, range: range, client: client) {
            return .replaced
        }
        return .unverified
    }

    /// True when the range starting at the original's location now reads back as
    /// the replacement, and the original is not still sitting immediately after
    /// it. A host that ignores the range and inserts at a caret on the same
    /// location satisfies the first check while leaving the original intact.
    private static func replacementLanded(
        _ replacement: String,
        at range: NSRange,
        replacing original: String? = nil,
        client: Client
    ) -> Bool {
        let expected = NSRange(location: range.location, length: replacement.utf16.count)
        guard client.readText(expected) == replacement else { return false }
        if let original {
            let leftover = NSRange(location: NSMaxRange(expected), length: original.utf16.count)
            if client.readText(leftover) == original { return false }
        }
        return true
    }

    /// Commits through a marked-text overlay. Only trusted once the range reads
    /// back as the replacement; hosts that ignore the overlay's range must fail.
    private static func replaceUsingMarkedText(
        original: String,
        replacement: String,
        range: NSRange,
        client: Client
    ) -> Bool {
        guard let setMarkedText = client.setMarkedText, let markedRange = client.markedRange,
              client.readText(range) == original else { return false }
        let beforeSelection = client.selectedRange()
        let beforeCaret = beforeSelection.length == 0 ? beforeSelection.location : NSMaxRange(beforeSelection)
        setMarkedText(original, NSRange(location: original.utf16.count, length: 0), range)
        let marked = markedRange()
        if marked.location == range.location, marked.length == range.length {
            // A range-ignoring host can look deceptively correct when its caret is
            // already at the range's start: it inserts an identical composition
            // before the original, then the commit leaves two copies. Without a
            // delete command there is no safe way to undo that duplicate, so only
            // commit the overlay when the host was already parked at the sentence
            // end (the position a real replacement must use).
            guard beforeCaret == NSMaxRange(range) else {
                client.insertText("", NSRange(location: NSNotFound, length: NSNotFound))
                client.trace?("marked-text range ambiguous; cleared composition")
                return false
            }
            // The overlay is parked exactly over the text we mean to rewrite, so
            // committing it cannot append a duplicate. Length must match too:
            // a host that ignores the range and inserts at a caret sitting on
            // the same location reports a mark that starts here but is shorter.
            client.insertText(replacement, NSRange(location: NSNotFound, length: NSNotFound))
            let landed = replacementLanded(replacement, at: range, replacing: original, client: client)
            client.trace?(landed ? "marked-text commit landed" : "marked-text commit did not land")
            if landed == false {
                undoIgnoredInsert(replacement, previousSelection: client.selectedRange(), client: client)
            }
            return landed
        }
        if marked.location != NSNotFound, marked.length > 0 {
            // A host that ignored the overlay's range parked the composition
            // somewhere else. Clear it and stop. A ranged insert here is what
            // appends the English after the original in Sublime Text.
            client.insertText("", NSRange(location: NSNotFound, length: NSNotFound))
            client.trace?("marked-text ignored range; cleared composition")
            return false
        }
        return false
    }

    /// One ranged insert, for hosts that honor `replacementRange` but do not
    /// report a marked-text range. Verified by reading the range back. A host
    /// that ignores the range appends at the caret; that copy is deleted before
    /// this reports failure, so the original is never left duplicated.
    private static func replaceUsingDirectInsert(
        original: String,
        replacement: String,
        range: NSRange,
        client: Client
    ) -> Bool {
        guard client.readText(range) == original else { return false }
        let before = client.selectedRange()
        client.insertText(replacement, range)
        if replacementLanded(replacement, at: range, replacing: original, client: client) {
            client.trace?("direct range insert landed")
            return true
        }
        undoIgnoredInsert(replacement, previousSelection: before, client: client)
        client.trace?("direct range insert did not land; undone")
        return false
    }

    /// Removes a replacement that a range-ignoring host appended at the caret.
    /// The stray copy may still be a marked composition, so clear that mark
    /// before deleting it; otherwise the empty insert removes the mark and
    /// leaves the duplicate text behind.
    private static func undoIgnoredInsert(
        _ replacement: String,
        previousSelection: NSRange,
        client: Client
    ) {
        if let marked = client.markedRange?(), marked.location != NSNotFound, marked.length > 0 {
            client.insertText("", NSRange(location: NSNotFound, length: NSNotFound))
        }
        let caret = client.selectedRange().location
        let inserted = NSRange(location: caret - replacement.utf16.count, length: replacement.utf16.count)
        if caret != NSNotFound, caret >= replacement.utf16.count, client.readText(inserted) == replacement {
            client.insertText("", inserted)
            return
        }
        // The host may have inserted at the caret it had before this attempt.
        let previousCaret = previousSelection.length == 0 ? previousSelection.location : NSMaxRange(previousSelection)
        let previousInsert = NSRange(location: previousCaret, length: replacement.utf16.count)
        if previousCaret != NSNotFound, client.readText(previousInsert) == replacement {
            client.insertText("", previousInsert)
        }
    }

    /// Deletes back to the start of the range with the host's own delete command,
    /// removing the original and any copy a host appended after it, then inserts
    /// the English exactly once.
    private static func replaceUsingDeletion(
        original: String,
        replacement: String,
        range: NSRange,
        client: Client
    ) -> Bool {
        guard let deleteBackward = client.deleteBackward else { return false }
        let readableOriginal = client.readText(range)
        guard readableOriginal == original || readableOriginal == nil else { return false }
        let selected = client.selectedRange()
        let selectedIsUsable = selected.location != NSNotFound && selected.location >= 0
        var caret = selected.length == 0 ? selected.location : NSMaxRange(selected)
        var blindDelete = false

        // Sublime reports a caret of 0 after the suggestion panel is clicked.
        // Hosts based on Chromium (notably WeChat) may instead report NSNotFound
        // even though their IME connection still has the caret at the end of the
        // sentence. In that case the range we verified above is the only safe
        // anchor we have: delete exactly the original's grapheme count from the
        // live IME caret, then verify the document before inserting anything.
        if selectedIsUsable {
            client.setSelection?(NSRange(location: NSMaxRange(range), length: 0))
            let moved = client.selectedRange().location
            if moved != NSNotFound { caret = moved }
            if caret != NSMaxRange(range) {
                client.trace?("delete skipped; caret \(caret) is not at \(NSMaxRange(range))")
                return false
            }
        } else {
            blindDelete = true
            caret = NSMaxRange(range)
            client.trace?("delete using captured sentence end \(caret) because host hid selection")
        }

        // Stop the moment the caret reaches the start of the range so real text
        // before the original is never touched, even if a host over-reports how
        // much it appended. A blind host cannot report caret movement, so it is
        // limited to exactly the original's grapheme count.
        let cap = original.count + replacement.count + 8
        var deletions = 0
        var previous = caret
        while deletions < cap {
            let location = client.selectedRange().location
            if blindDelete {
                if deletions >= original.count { break }
            } else {
                if location == NSNotFound || location <= range.location { break }
                // A host whose delete command does not move the caret would have us
                // loop until the cap and eat unrelated text. Stop the moment the
                // caret stops advancing.
                if location >= previous, deletions > 0 { break }
                previous = location
            }
            deleteBackward()
            deletions += 1
        }
        let reachedRangeStart = blindDelete || client.selectedRange().location == range.location
        guard reachedRangeStart,
              deletions == original.count,
              client.readText(range) != original else { return false }
        client.insertText(replacement, NSRange(location: NSNotFound, length: NSNotFound))
        // An unreadable host cannot provide read-back verification. At this
        // point we have deleted exactly the original grapheme count from the
        // captured sentence end and committed the replacement at that caret;
        // treating the operation as successful is preferable to reporting
        // "no response" and leaving the UI waiting forever. Readable hosts
        // still require the strict postcondition above.
        if client.readText(range) == nil {
            client.trace?("blind deletion replacement committed")
            return true
        }
        return replacementLanded(replacement, at: range, replacing: original, client: client)
    }

    /// One document-relative commit, with read-back verification. Never creates a
    /// marked overlay: some clients ignore that overlay's replacement range.
    public static func replace(
        original: String,
        replacement: String,
        range: NSRange,
        expectedSelection: NSRange,
        selectedRange: () -> NSRange,
        readText: (NSRange) -> String?,
        insertText: (String, NSRange) -> Void
    ) -> Outcome {
        guard !original.isEmpty, !replacement.isEmpty,
              range.location != NSNotFound, range.location >= 0,
              range.length == original.utf16.count,
              range.location <= Int.max - range.length,
              range.location <= Int.max - replacement.utf16.count,
              readText(range) == original else { return .stale }
        // Some clients report the caret one character behind the just-typed
        // terminator. The verified text is the commit condition, not the caret.
        _ = expectedSelection
        _ = selectedRange

        insertText(replacement, range)
        let replacedRange = NSRange(location: range.location, length: replacement.utf16.count)
        guard selectedRange() == NSRange(location: NSMaxRange(replacedRange), length: 0),
              readText(replacedRange) == replacement else { return .unverified }
        return .replaced
    }

    /// Finds `original` on the caret's line. Prefers the copy that ends at the caret,
    /// including the case where the client has not yet moved the caret past a period.
    public static func rangeOfOriginal(document: String?, documentOffset: Int = 0, selected: NSRange, original: String) -> NSRange? {
        guard let document, selected.location != NSNotFound, selected.length == 0, documentOffset >= 0 else { return nil }
        let nsDocument = document as NSString
        let originalText = original as NSString
        let length = originalText.length
        guard length > 0, selected.location <= nsDocument.length else { return nil }

        let localCaret = selected.location - documentOffset
        guard localCaret >= 0, localCaret <= nsDocument.length else { return nil }
        let line = nsDocument.lineRange(for: NSRange(location: min(localCaret, max(0, nsDocument.length - 1)), length: 0))
        let lineEnd = min(nsDocument.length, line.location + line.length)
        let searchEnd = min(lineEnd, localCaret + length)
        guard searchEnd >= length else { return nil }
        let search = nsDocument.substring(with: NSRange(location: 0, length: searchEnd)) as NSString
        let found = search.range(of: original, options: .backwards)
        guard found.location != NSNotFound else { return nil }
        return NSRange(location: found.location + documentOffset, length: found.length)
    }

    /// The sentence a terminator should rewrite: everything on the current line
    /// up to and including that terminator.
    public static func sentenceOnCurrentLine(document: String?, selected: NSRange) -> (text: String, range: NSRange)? {
        guard let document, selected.location != NSNotFound, selected.location >= 0 else { return nil }
        let nsDocument = document as NSString
        guard selected.location <= nsDocument.length else { return nil }
        let cursor = selected.location
        let lineAnchor = min(cursor, max(0, nsDocument.length - 1))
        let line = nsDocument.lineRange(for: NSRange(location: lineAnchor, length: 0))
        let end = min(nsDocument.length, max(cursor, line.location))
        // Include a terminator the client has not yet moved the caret past.
        let extendedEnd = min(nsDocument.length, max(end, cursor + 1))
        let rawEnd: Int
        if extendedEnd > end {
            let extra = nsDocument.substring(with: NSRange(location: end, length: extendedEnd - end))
            rawEnd = ".!?。？！".contains(extra) ? extendedEnd : end
        } else {
            rawEnd = end
        }
        guard rawEnd > line.location else { return nil }
        let raw = nsDocument.substring(with: NSRange(location: line.location, length: rawEnd - line.location))
        let leading = raw.prefix { $0.isWhitespace }.utf16.count
        let text = String(raw.drop { $0.isWhitespace }).trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.isEmpty == false else { return nil }
        return (text, NSRange(location: line.location + leading, length: text.utf16.count))
    }

    /// Recovers text that was pasted or typed before Lucid started tracking the field.
    /// The trailing tracked text must already be present at the cursor; only its preceding
    /// context on the same line is included.
    public static func sentenceBeforeCursor(
        document: String?,
        selected: NSRange,
        tracked: String
    ) -> (text: String, range: NSRange)? {
        guard let document, selected.location != NSNotFound, selected.length == 0 else { return nil }
        let nsDocument = document as NSString
        guard selected.location <= nsDocument.length else { return nil }

        let trackedText = tracked as NSString
        let trackedLength = trackedText.length
        guard trackedLength > 0 else { return nil }

        // A period can arrive before the client moves the caret past it.
        let cursor = selected.location
        let cursorAfter = min(nsDocument.length, cursor + 1)
        let candidates = [cursor, cursorAfter]
        var trackedStart: Int?
        for end in candidates where end >= trackedLength {
            let start = end - trackedLength
            let range = NSRange(location: start, length: trackedLength)
            if nsDocument.substring(with: range) == tracked {
                trackedStart = start
                break
            }
        }
        guard let trackedStart else { return nil }
        let lineRange = nsDocument.lineRange(for: NSRange(location: trackedStart, length: 0))
        let contextStart = lineRange.location
        // A typed period finishes the whole current line, including words that
        // were already in the field before Lucid started tracking keystrokes.
        let contextEnd = trackedStart + trackedLength
        let contextLength = contextEnd - contextStart
        guard contextLength >= trackedLength else { return nil }

        let context = nsDocument.substring(with: NSRange(location: contextStart, length: contextLength))
        let leadingLength = context.prefix { $0.isWhitespace }.utf16.count
        let text = String(context.drop { $0.isWhitespace })
        guard !text.isEmpty else { return nil }
        let range = NSRange(location: contextStart + leadingLength, length: text.utf16.count)
        return (text, range)
    }
}


/// Decides whether a pause should count as "the user finished this sentence".
public enum SentenceCompletion: Sendable {
    public static func looksFinished(_ text: String) -> Bool {
        let draft = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard draft.isEmpty == false else { return false }
        if draft.hasSuffix(",") || draft.hasSuffix("，") || draft.hasSuffix(";") || draft.hasSuffix(":") || draft.hasSuffix("：") || draft.hasSuffix("-") || draft.hasSuffix("—") || draft.hasSuffix("…") || draft.hasSuffix("...") {
            return false
        }
        if let lastCharacter = draft.last, ".!?。？！".contains(lastCharacter) {
            return true
        }

        let tokens = draft.split { $0.isWhitespace || $0 == "/" }.map(String.init).filter { !$0.isEmpty }
        // One or two tokens is usually still a phrase, not a finished sentence.
        guard tokens.count >= 3 else { return false }

        let letters = draft.filter(\.isLetter)
        guard letters.count >= 8 else { return false }

        let last = tokens.last!.trimmingCharacters(in: CharacterSet(charactersIn: "'\"“”‘’"))
        guard last.isEmpty == false else { return false }
        let normalized = last.lowercased()
        if danglingLastTokens.contains(normalized) { return false }
        if normalized.count < 4, completeShortEndings.contains(normalized) == false {
            return false
        }
        return true
    }

    private static let completeShortEndings: Set<String> = [
        "me", "it", "us", "ok", "no", "yes", "up", "out", "now", "here", "there",
        "too", "him", "her", "them", "one", "all", "off", "on", "in", "go",
    ]

    private static let danglingLastTokens: Set<String> = [
        "a", "an", "the", "to", "for", "of", "and", "or", "but", "if", "when", "while",
        "that", "this", "these", "those", "my", "your", "our", "their", "his", "her", "its",
        "i", "im", "i'm", "ive", "i've", "id", "i'd", "ill", "i'll", "youre", "you're",
        "we", "they", "he", "she", "am", "is", "are", "was", "were", "be", "been", "being",
        "will", "would", "can", "could", "should", "may", "might", "must", "shall",
        "want", "wanna", "need", "like", "at", "in", "on", "with", "from", "about", "into",
        "onto", "as", "than", "then", "also", "just", "very", "so", "because", "have", "has",
        "had", "do", "does", "did", "dont", "don't", "gonna", "going", "let", "lets", "let's",
        "please", "not", "too", "more", "most", "some", "any", "each", "every", "which",
        "whom", "whose", "wo", "yao", "xiang", "gei", "zai", "ba", "bei", "de", "le",
    ]
}
