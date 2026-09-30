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
