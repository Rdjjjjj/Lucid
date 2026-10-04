import Foundation
import XCTest
@testable import LucidCore

final class SentenceTrackerTests: XCTestCase {
    func testCompletesPunctuationTerminatedSentenceAndTracksUTF16Range() {
        var tracker = SentenceTracker()
        let result = tracker.append("I want to mai coffee.").first
        XCTAssertEqual(result?.text, "I want to mai coffee.")
        XCTAssertEqual(result?.utf16Range, NSRange(location: 0, length: 21))
    }

    func testFlushesUnpunctuatedSentenceAfterPause() {
        var tracker = SentenceTracker()
        XCTAssertTrue(tracker.append("Can you help me").isEmpty)
        let completed = tracker.flushOnPause()
        XCTAssertEqual(completed?.text, "Can you help me")
        XCTAssertNil(tracker.flushOnPause())
    }

    func testTracksMultipleSentencesAndOffsets() {
        var tracker = SentenceTracker()
        let completed = tracker.append("Hi! I need hepl.")
        XCTAssertEqual(completed.map(\.text), ["Hi!", "I need hepl."])
        XCTAssertEqual(completed.map(\.utf16Range), [NSRange(location: 0, length: 3), NSRange(location: 4, length: 12)])
    }

    func testStreamedDecimalPointDoesNotEndSentence() {
        var tracker = SentenceTracker()
        var completed: [CompletedSentence] = []
        for character in "The value is 3.14 now." {
            completed += tracker.append(String(character))
        }
        XCTAssertEqual(completed.map(\.text), ["The value is 3.14 now."])
    }

    func testDeleteBackwardRemovesLastCharacter() {
        var tracker = SentenceTracker()
        _ = tracker.append("mai")
        tracker.deleteBackward()
        XCTAssertEqual(tracker.pendingText, "ma")
        XCTAssertEqual(tracker.flushOnPause()?.text, "ma")
    }

    func testResetInvalidatesAndClearsCurrentSession() {
        var tracker = SentenceTracker()
        _ = tracker.append("draft")
        let oldVersion = tracker.version
        tracker.reset()
        XCTAssertTrue(tracker.pendingText.isEmpty)
        XCTAssertGreaterThan(tracker.version, oldVersion)
        XCTAssertNil(tracker.flushOnPause())
    }

    func testRejectsInsecureRemoteConfiguration() {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "http://example.com")!, model: "model")
        XCTAssertThrowsError(try config.validate())
    }

    func testAllowsLocalDevelopmentRelayOverHTTP() throws {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "http://localhost:8080/v1")!, model: "model")
        try config.validate()
    }

    func testAllowsFetchingModelsBeforeSelectingOne() throws {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "https://api.example.com/v1")!, model: "")
        try config.validate(requireModel: false)
        XCTAssertThrowsError(try config.validate())
    }

    func testReplacementRangeUsesOriginalLengthFromCursor() {
        let range = CommittedTextReplacement.range(
            originalUTF16Length: 6,
            selected: NSRange(location: 6, length: 0),
            origin: 0
        )
        XCTAssertEqual(range, NSRange(location: 0, length: 6))
    }

    func testReplacementRangeDoesNotUseRewrittenLength() {
        let originalLength = ("shenme" as NSString).length
        let rewrittenLength = ("What?" as NSString).length
        let cursor = NSRange(location: originalLength, length: 0)
        let correct = CommittedTextReplacement.range(originalUTF16Length: originalLength, selected: cursor, origin: 0)
        let wrong = CommittedTextReplacement.range(originalUTF16Length: rewrittenLength, selected: cursor, origin: 0)
        XCTAssertEqual(correct, NSRange(location: 0, length: 6))
        XCTAssertNotEqual(wrong, correct)
    }


    func testRecoversPastedContextBeforeTrackedTerminator() {
        let pasted = "jiexialaiwojiangyanshigaishurufaruheshiyong."
        let recovered = CommittedTextReplacement.sentenceBeforeCursor(
            document: pasted,
            selected: NSRange(location: (pasted as NSString).length, length: 0),
            tracked: "."
        )
        XCTAssertEqual(recovered?.text, pasted)
        XCTAssertEqual(recovered?.range, NSRange(location: 0, length: (pasted as NSString).length))
    }

    func testPeriodRewritesTheWholeCurrentLineNotOnlyTrackedText() {
        let document = "Can u tuichi this meeting?"
        let recovered = CommittedTextReplacement.sentenceOnCurrentLine(
            document: document,
            selected: NSRange(location: (document as NSString).length, length: 0)
        )
        XCTAssertEqual(recovered?.text, document)
        XCTAssertEqual(recovered?.range, NSRange(location: 0, length: (document as NSString).length))
    }

    func testPeriodIncludesTextTypedBeforeLucidWasActivated() {
        let document = "Please review this. Can u tuichi this meeting?"
        let recovered = CommittedTextReplacement.sentenceOnCurrentLine(
            document: document,
            selected: NSRange(location: (document as NSString).length - 1, length: 0)
        )
        XCTAssertEqual(recovered?.text, document)
    }

    func testCurrentLineDoesNotCrossANewline() {
        let document = "old line.\nCan u tuichi this meeting?"
        let recovered = CommittedTextReplacement.sentenceOnCurrentLine(
            document: document,
            selected: NSRange(location: (document as NSString).length, length: 0)
        )
        XCTAssertEqual(recovered?.text, "Can u tuichi this meeting?")
    }

    func testDoesNotRecoverContextFromPreviousLine() {
        let document = "previous sentence.\npasted text."
        let recovered = CommittedTextReplacement.sentenceBeforeCursor(
            document: document,
            selected: NSRange(location: (document as NSString).length, length: 0),
            tracked: "."
        )
        XCTAssertEqual(recovered?.text, "pasted text.")
    }

    func testDoesNotRecoverWhenTrackedTextDoesNotMatchCursor() {
        let recovered = CommittedTextReplacement.sentenceBeforeCursor(
            document: "pasted text?",
            selected: NSRange(location: 12, length: 0),
            tracked: "."
        )
        XCTAssertNil(recovered)
    }

    func testLooksFinishedIgnoresSingleWord() {
        XCTAssertFalse(SentenceCompletion.looksFinished("shenme"))
        XCTAssertFalse(SentenceCompletion.looksFinished("coffee"))
        XCTAssertFalse(SentenceCompletion.looksFinished("wo"))
    }

    func testLooksFinishedIgnoresIncompletePhrases() {
        XCTAssertFalse(SentenceCompletion.looksFinished("I want to"))
        XCTAssertFalse(SentenceCompletion.looksFinished("I want to mai"))
        XCTAssertFalse(SentenceCompletion.looksFinished("wo xiang"))
        XCTAssertFalse(SentenceCompletion.looksFinished("mai coffee"))
        XCTAssertFalse(SentenceCompletion.looksFinished("Hello,"))
    }

    func testLooksFinishedAcceptsCompleteSentences() {
        XCTAssertTrue(SentenceCompletion.looksFinished("I want to mai coffee"))
        XCTAssertTrue(SentenceCompletion.looksFinished("Can you help me"))
        XCTAssertTrue(SentenceCompletion.looksFinished("Hello?"))
        XCTAssertTrue(SentenceCompletion.looksFinished("I want to mai coffee."))
    }

    func testParsesOpenAIChatContent() {
        let json = #"{"choices":[{"message":{"role":"assistant","content":"Is Breaking Bad a good show?"}}]}"#
        let text = CorrectionResponseParser.sentence(from: Data(json.utf8))
        XCTAssertEqual(text, "Is Breaking Bad a good show?")
    }

    func testParsesArrayContentAndJSONPayload() throws {
        let arrayJSON = #"{"choices":[{"message":{"content":[{"type":"text","text":"Is Breaking Bad a good show?"}]}}]}"#
        XCTAssertEqual(CorrectionResponseParser.sentence(from: Data(arrayJSON.utf8)), "Is Breaking Bad a good show?")

        let inner = #"{"corrected_text":"Is Breaking Bad a good show?"}"#
        let payload: [String: Any] = [
            "choices": [
                ["message": ["content": inner]]
            ]
        ]
        let wrapped = try JSONSerialization.data(withJSONObject: payload)
        XCTAssertEqual(CorrectionResponseParser.sentence(from: wrapped), "Is Breaking Bad a good show?")
    }

    func testRecoversTruncatedJSONContent() {
        let truncated = "{" + "\"corrected_text\":" + "\"Is Breaking Bad a good show?\""
        XCTAssertEqual(CorrectionResponseParser.cleanedSentence(truncated), "Is Breaking Bad a good show?")
    }

    func testIgnoresEmptyContent() {
        let json = #"{"choices":[{"message":{"content":"","reasoning_content":"thinking"}}]}"#
        XCTAssertNil(CorrectionResponseParser.sentence(from: Data(json.utf8)))
    }

    func testPrefersContentOverReasoning() {
        let json = #"{"choices":[{"message":{"content":"Is Breaking Bad a good show?","reasoning_content":"We need answer. User asks juemingdushi"}}]}"#
        XCTAssertEqual(CorrectionResponseParser.sentence(from: Data(json.utf8)), "Is Breaking Bad a good show?")
    }

    func testUsesContentWhenReasoningIsLong() throws {
        let payload: [String: Any] = [
            "choices": [[
                "message": [
                    "content": "Is Breaking Bad a good show?",
                    "reasoning_content": String(repeating: "think ", count: 80)
                ]
            ]]
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        XCTAssertEqual(CorrectionResponseParser.sentence(from: data), "Is Breaking Bad a good show?")
    }

    func testNewlineDoesNotEndSentence() {
        var tracker = SentenceTracker()
        XCTAssertTrue(tracker.append("Can you help me\n").isEmpty)
        XCTAssertTrue(tracker.pendingText.contains("Can you help me"))
    }
}

final class CommittedSentenceReplacementTests: XCTestCase {
    private final class Client {
        var document: NSMutableString
        var selection: NSRange
        var writes: [NSRange] = []
        var ignoresRange = false
        init(_ text: String, cursor: Int? = nil) {
            document = NSMutableString(string: text)
            selection = NSRange(location: cursor ?? text.utf16.count, length: 0)
        }
        func read(_ range: NSRange) -> String? {
            guard range.location <= document.length, range.length <= document.length - range.location else { return nil }
            return document.substring(with: range)
        }
        func insert(_ text: String, _ range: NSRange) {
            writes.append(range)
            let target = ignoresRange ? selection : range
            document.replaceCharacters(in: target, with: text)
            selection = NSRange(location: target.location + text.utf16.count, length: 0)
        }
        func replace(_ original: String, with english: String, range: NSRange, expected: NSRange? = nil) -> CommittedTextReplacement.Outcome {
            CommittedTextReplacement.replace(original: original, replacement: english, range: range,
                expectedSelection: expected ?? selection,
                selectedRange: { self.selection }, readText: read, insertText: insert)
        }
    }

    func testPastedPinyinIsReplacedInOneCommitNotAppended() {
        let original = "jiexialaiwojiangyanshigaishurufaruheshiyong."
        let english = "Next, I will demonstrate how to use this input method."
        let client = Client(original)
        let range = NSRange(location: 0, length: original.utf16.count)
        XCTAssertEqual(client.replace(original, with: english, range: range), .replaced)
        XCTAssertEqual(client.document as String, english)
        XCTAssertEqual(client.writes, [range])
    }

    func testPreservesPrefixSuffixAndUTF16Boundaries() {
        let original = "请帮我检查🙂."
        let prefix = "Previous line.\n  "
        let suffix = "\nDo not change this."
        let range = NSRange(location: prefix.utf16.count, length: original.utf16.count)
        let client = Client(prefix + original + suffix, cursor: NSMaxRange(range))
        XCTAssertEqual(client.replace(original, with: "Please check this.", range: range), .replaced)
        XCTAssertEqual(client.document as String, prefix + "Please check this." + suffix)
    }

    func testMovedCursorStillReplacesWhenOriginalTextMatches() {
        // The caret is not the commit condition. A period often leaves the caret
        // one character behind; the verified original text is what matters.
        let client = Client("nihao.", cursor: 5)
        XCTAssertEqual(client.replace("nihao.", with: "Hello.", range: NSRange(location: 0, length: 6), expected: NSRange(location: 6, length: 0)), .replaced)
        XCTAssertEqual(client.document as String, "Hello.")
    }

    func testPeriodNotYetInSelectionStillRecoversSentence() {
        let document = "gaixiechengyingwen."
        let recovered = CommittedTextReplacement.sentenceBeforeCursor(
            document: document,
            selected: NSRange(location: document.utf16.count - 1, length: 0),
            tracked: "gaixiechengyingwen."
        )
        XCTAssertEqual(recovered?.text, document)
        XCTAssertEqual(recovered?.range.location, 0)
    }

    func testRangeOfOriginalFindsSentenceWhenCaretLagsPeriod() {
        let document = "gaixiechengyingwen."
        let range = CommittedTextReplacement.rangeOfOriginal(
            document: document,
            selected: NSRange(location: document.utf16.count - 1, length: 0),
            original: document
        )
        XCTAssertEqual(range, NSRange(location: 0, length: document.utf16.count))
    }

    func testEditedOriginalRejectsSuggestionWithoutWriting() {
        let client = Client("hello.")
        XCTAssertEqual(client.replace("nihao.", with: "Hello.", range: NSRange(location: 0, length: 6)), .stale)
        XCTAssertTrue(client.writes.isEmpty)
    }

    func testIgnoredRangeIsNotReportedAsSuccessOrRetried() {
        let client = Client("nihao.")
        client.ignoresRange = true
        XCTAssertEqual(client.replace("nihao.", with: "Hello.", range: NSRange(location: 0, length: 6)), .unverified)
        XCTAssertEqual(client.writes.count, 1)
    }

    func testUnknownRangeNeverInsertsAtCaret() {
        let client = Client("nihao.")
        XCTAssertEqual(client.replace("nihao.", with: "Hello.", range: NSRange(location: NSNotFound, length: 6)), .stale)
        XCTAssertTrue(client.writes.isEmpty)
    }

    func testPastedIndentedSentenceKeepsMatchingRange() {
        let document = "previous\n  nihao."
        let recovered = CommittedTextReplacement.sentenceBeforeCursor(document: document, selected: NSRange(location: document.utf16.count, length: 0), tracked: ".")
        XCTAssertEqual(recovered?.text, "nihao.")
        XCTAssertEqual(recovered?.range, NSRange(location: 11, length: 6))
    }
}

/// Hosts that ignore `replacementRange` (WeChat, Codex) used to get the English
/// appended after the original. These tests model that behaviour directly so the
/// verified commit logic cannot regress into a silent append.
final class VerifiedReplacementTests: XCTestCase {
    /// Minimal editable-text host. `honorsReplacementRange` mirrors WeChat: when
    /// false the host ignores the range and inserts at the caret instead.
    private final class Host {
        var document: NSMutableString
        var selection: NSRange
        var honorsReplacementRange = true
        var supportsMarkedText = true
        var supportsDeleteBackward = true
        var reportsUnknownSelection = false
        private var marked: NSRange = NSRange(location: NSNotFound, length: 0)
        private(set) var inserts: [String] = []

        init(_ text: String) {
            document = NSMutableString(string: text)
            selection = NSRange(location: text.utf16.count, length: 0)
        }

        func read(_ range: NSRange) -> String? {
            guard range.location != NSNotFound,
                  range.location <= document.length,
                  range.length <= document.length - range.location else { return nil }
            return document.substring(with: range)
        }

        /// Marked composition text lives in the document, as it does in a real
        /// editor. A range-ignoring host drops it in at the caret instead of
        /// replacing `replacementRange`, which is exactly the WeChat append bug.
        private func insertMarked(_ text: String, replacementRange: NSRange) {
            let anchor: NSRange
            if honorsReplacementRange, replacementRange.location != NSNotFound {
                anchor = replacementRange
            } else {
                anchor = NSRange(location: selection.location, length: 0)
            }
            guard anchor.location <= document.length else { return }
            let length = min(anchor.length, document.length - anchor.location)
            document.replaceCharacters(in: NSRange(location: anchor.location, length: length), with: text)
            marked = NSRange(location: anchor.location, length: (text as NSString).length)
            selection = NSRange(location: marked.location + marked.length, length: 0)
        }

        private func commit(_ text: String, replacementRange: NSRange) {
            inserts.append(text)
            let target: NSRange
            if marked.location != NSNotFound {
                // An empty commit clears an uncommitted composition, the same way
                // an editor abandons inline input. A non-empty commit replaces it.
                if text.isEmpty {
                    document.replaceCharacters(in: marked, with: "")
                    selection = NSRange(location: marked.location, length: 0)
                    marked = NSRange(location: NSNotFound, length: 0)
                    return
                }
                target = marked
            } else if honorsReplacementRange, replacementRange.location != NSNotFound {
                target = replacementRange
            } else {
                target = NSRange(location: selection.location, length: 0)
            }
            marked = NSRange(location: NSNotFound, length: 0)
            guard target.location <= document.length else { return }
            let length = min(target.length, document.length - target.location)
            document.replaceCharacters(in: NSRange(location: target.location, length: length), with: text)
            selection = NSRange(location: target.location + (text as NSString).length, length: 0)
        }

        func client() -> CommittedTextReplacement.Client {
            CommittedTextReplacement.Client(
                readText: { [self] in read($0) },
                selectedRange: { [self] in
                    reportsUnknownSelection
                        ? NSRange(location: NSNotFound, length: NSNotFound)
                        : selection
                },
                setMarkedText: supportsMarkedText ? { [self] text, _, replacementRange in
                    insertMarked(text, replacementRange: replacementRange)
                } : nil,
                markedRange: supportsMarkedText ? { [self] in marked } : nil,
                insertText: { [self] text, replacementRange in
                    commit(text, replacementRange: replacementRange)
                },
                deleteBackward: supportsDeleteBackward ? { [self] in
                    if marked.location != NSNotFound {
                        document.replaceCharacters(in: marked, with: "")
                        selection = NSRange(location: marked.location, length: 0)
                        marked = NSRange(location: NSNotFound, length: 0)
                        return
                    }
                    guard selection.location > 0, selection.location <= document.length else { return }
                    let charRange = document.rangeOfComposedCharacterSequence(at: selection.location - 1)
                    document.replaceCharacters(in: charRange, with: "")
                    selection = NSRange(location: charRange.location, length: 0)
                } : nil,
                setSelection: { [self] range in
                    guard range.location != NSNotFound, range.location <= document.length else { return }
                    selection = NSRange(location: range.location, length: 0)
                }
            )
        }
    }

    func testPanelFocusLossStillReplacesWhenDeleteWorks() {
        let host = Host("nihao.")
        host.honorsReplacementRange = false
        host.selection = NSRange(location: 0, length: 0)
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, "Hello.")
    }

    func testUnknownSelectionUsesCapturedSentenceEndForChromiumLikeHost() {
        let host = Host("nihao.")
        host.honorsReplacementRange = false
        host.reportsUnknownSelection = true
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, "Hello.")
    }

    func testIgnoredInsertIsUndoneWhenHostCannotDelete() {
        let host = Host("nihao.")
        host.honorsReplacementRange = false
        host.supportsDeleteBackward = false
        host.selection = NSRange(location: 0, length: 0)
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .unverified
        )
        XCTAssertEqual(host.document as String, "nihao.")
    }

    func testSelectedSentenceStillYieldsRangeBehindCaret() {
        let selected = NSRange(location: 4, length: 6)
        XCTAssertEqual(
            CommittedTextReplacement.rangeBehindCaret(originalUTF16Length: 6, selected: selected),
            selected
        )
    }

    func testHonestHostReplacesInOneCommit() {
        let host = Host("nihao.")
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, "Hello.")
    }

    func testRangeIgnoringHostStillReplacesInsteadOfAppending() {
        let host = Host("can u tuichi this meeting?")
        host.honorsReplacementRange = false
        let original = "can u tuichi this meeting?"
        let replacement = "can you postpone this meeting"
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: original, replacement: replacement, range: NSRange(location: 0, length: original.utf16.count), client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, replacement)
        XCTAssertFalse((host.document as String).contains(original))
    }

    func testRangeIgnoringHostWithExistingSuffixKeepsSuffix() {
        let original = "nihao."
        let prefix = "previous "
        let suffix = " after"
        let host = Host(prefix + original + suffix)
        host.honorsReplacementRange = false
        host.selection = NSRange(location: (prefix + original).utf16.count, length: 0)
        let range = NSRange(location: prefix.utf16.count, length: original.utf16.count)
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: original, replacement: "Hello.", range: range, client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, prefix + "Hello." + suffix)
    }

    func testMarkedTextOnlyRequiresNoDelete() {
        let host = Host("nihao.")
        host.honorsReplacementRange = true
        host.supportsDeleteBackward = false
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .replaced
        )
        XCTAssertEqual(host.document as String, "Hello.")
    }

    func testRangeIgnoringHostWithoutDeleteNeverCorrupts() {
        // WeChat-shaped worst case: the host ignores the overlay's range AND
        // exposes no delete command. Nothing can replace the text, but the
        // original must survive untouched — no appended duplicate.
        let host = Host("nihao.")
        host.honorsReplacementRange = false
        host.supportsDeleteBackward = false
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .unverified
        )
        XCTAssertFalse((host.document as String).contains("Hello."))
    }

    func testHostWithoutMarkedOrDeleteNeverAppends() {
        let host = Host("nihao.")
        host.supportsMarkedText = false
        host.supportsDeleteBackward = false
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .unverified
        )
        XCTAssertEqual(host.document as String, "nihao.")
    }

    func testStaleOriginalIsNeverTouched() {
        let host = Host("hello.")
        XCTAssertEqual(
            CommittedTextReplacement.replaceRange(original: "nihao.", replacement: "Hello.", range: NSRange(location: 0, length: 6), client: host.client()),
            .stale
        )
        XCTAssertEqual(host.document as String, "hello.")
        XCTAssertTrue(host.inserts.isEmpty)
    }
}

/// WeChat reports the caret one character past the sentence it is replacing.
/// The caret-derived range then misses by one, so the precisely recovered range
/// has to win instead of the caret guess.
final class PreferredRangeTests: XCTestCase {
    private func document(_ text: String) -> (NSRange) -> String? {
        let ns = text as NSString
        return { range in
            guard range.location >= 0, range.location <= ns.length,
                  range.length <= ns.length - range.location else { return nil }
            return ns.substring(with: range)
        }
    }

    func testPrefersRecoveredRangeWhenCaretRangeMissesByOne() {
        let original = "can u tuichi this meeting?"
        let read = document(original)
        let caretRange = NSRange(location: 1, length: original.utf16.count)
        let recoveredRange = NSRange(location: 0, length: original.utf16.count)
        let chosen = CommittedTextReplacement.preferredRange(
            original: original,
            candidates: [caretRange, recoveredRange],
            readText: read
        )
        XCTAssertEqual(chosen, recoveredRange)
    }

    func testPrefersCaretRangeWhenItMatches() {
        let original = "nihao."
        let read = document("previous " + original)
        let caretRange = NSRange(location: 9, length: 6)
        let staleStored = NSRange(location: 0, length: 6)
        let chosen = CommittedTextReplacement.preferredRange(
            original: original,
            candidates: [caretRange, staleStored],
            readText: read
        )
        XCTAssertEqual(chosen, caretRange)
    }

    func testFallsBackToFirstCandidateWhenNothingMatches() {
        let original = "nihao."
        let read = document("hello.")
        let caretRange = NSRange(location: 0, length: 6)
        let chosen = CommittedTextReplacement.preferredRange(
            original: original,
            candidates: [caretRange, nil].compactMap { $0 },
            readText: read
        )
        XCTAssertEqual(chosen, caretRange)
    }

    func testIgnoresCandidatesWithWrongLength() {
        let original = "nihao."
        let read = document("nihao.")
        let chosen = CommittedTextReplacement.preferredRange(
            original: original,
            candidates: [NSRange(location: NSNotFound, length: 6), NSRange(location: 0, length: 3), NSRange(location: 0, length: 6)],
            readText: read
        )
        XCTAssertEqual(chosen, NSRange(location: 0, length: 6))
    }
}

final class RangeSearchingTests: XCTestCase {
    private func document(_ text: String) -> (NSRange) -> String? {
        let ns = text as NSString
        return { range in
            guard range.location >= 0, range.location <= ns.length,
                  range.length <= ns.length - range.location else { return nil }
            return ns.substring(with: range)
        }
    }

    func testFindsOriginalEndingAtTheCaret() {
        let text = "hello nihao?"
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: (text as NSString).length, length: 0),
            readText: document(text)
        )
        XCTAssertEqual(found, NSRange(location: 6, length: 6))
    }

    func testFindsOriginalWhenCaretLagsBehindTheTerminator() {
        // WeChat can report the caret one character behind a just-typed "?".
        let text = "nihao?"
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: 5, length: 0),
            readText: document(text)
        )
        XCTAssertEqual(found, NSRange(location: 0, length: 6))
    }

    func testPicksTheLastOccurrenceOfRepeatedText() {
        let text = "nihao? and then nihao?"
        let caret = (text as NSString).length
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: caret, length: 0),
            readText: document(text)
        )
        XCTAssertEqual(found, NSRange(location: 16, length: 6))
    }

    func testReturnsAbsoluteRangeWhenTheWindowIsTruncated() {
        let text = "0123456789nihao?"
        let caret = (text as NSString).length
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: caret, length: 0),
            windowLength: 10,
            readText: document(text)
        )
        XCTAssertEqual(found, NSRange(location: 10, length: 6))
    }

    func testReturnsNilWhenHostExposesNoText() {
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: 6, length: 0),
            readText: { _ in nil }
        )
        XCTAssertNil(found)
    }

    func testReturnsNilWhenOriginalIsAbsent() {
        let found = CommittedTextReplacement.rangeSearchingBackwards(
            original: "nihao?",
            selected: NSRange(location: 6, length: 0),
            readText: document("hello.")
        )
        XCTAssertNil(found)
    }
}

final class LooseOriginalMatchTests: XCTestCase {
    func testAcceptsAutoCapitalisedSentence() {
        // TextEdit/Notes capitalise the first letter after the keystroke, so the
        // document reads "Nihao?" while the tracker recorded "nihao?".
        XCTAssertTrue(CommittedTextReplacement.matchesLoosely("Nihao?", original: "nihao?"))
    }

    func testAcceptsExactMatch() {
        XCTAssertTrue(CommittedTextReplacement.matchesLoosely("nihao?", original: "nihao?"))
    }

    func testRejectsDifferentLength() {
        XCTAssertFalse(CommittedTextReplacement.matchesLoosely("Nihao??", original: "nihao?"))
    }

    func testRejectsDifferentText() {
        XCTAssertFalse(CommittedTextReplacement.matchesLoosely("zaijian?", original: "nihao?"))
    }

    func testRejectsUnreadableHost() {
        XCTAssertFalse(CommittedTextReplacement.matchesLoosely(nil, original: "nihao?"))
    }
}
