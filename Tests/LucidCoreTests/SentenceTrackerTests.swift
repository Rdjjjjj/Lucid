import Foundation
import Testing
@testable import LucidCore

struct SentenceTrackerTests {
    @Test func completesPunctuationTerminatedSentenceAndTracksUTF16Range() {
        var tracker = SentenceTracker()
        let result = tracker.append("I want to mai coffee.").first
        #expect(result?.text == "I want to mai coffee.")
        #expect(result?.utf16Range == NSRange(location: 0, length: 21))
    }

    @Test func flushesUnpunctuatedSentenceAfterPause() {
        var tracker = SentenceTracker()
        #expect(tracker.append("Can you help me").isEmpty)
        let completed = tracker.flushOnPause()
        #expect(completed?.text == "Can you help me")
        #expect(tracker.flushOnPause() == nil)
    }

    @Test func tracksMultipleSentencesAndOffsets() {
        var tracker = SentenceTracker()
        let completed = tracker.append("Hi! I need hepl.")
        #expect(completed.map(\.text) == ["Hi!", "I need hepl."])
        #expect(completed.map(\.utf16Range) == [NSRange(location: 0, length: 3), NSRange(location: 4, length: 12)])
    }

    @Test func streamedDecimalPointDoesNotEndSentence() {
        var tracker = SentenceTracker()
        var completed: [CompletedSentence] = []
        for character in "The value is 3.14 now." {
            completed += tracker.append(String(character))
        }
        #expect(completed.map(\.text) == ["The value is 3.14 now."])
    }

    @Test func deleteBackwardRemovesLastCharacter() {
        var tracker = SentenceTracker()
        _ = tracker.append("mai")
        tracker.deleteBackward()
        #expect(tracker.pendingText == "ma")
        #expect(tracker.flushOnPause()?.text == "ma")
    }

    @Test func resetInvalidatesAndClearsCurrentSession() {
        var tracker = SentenceTracker()
        _ = tracker.append("draft")
        let oldVersion = tracker.version
        tracker.reset()
        #expect(tracker.pendingText.isEmpty)
        #expect(tracker.version > oldVersion)
        #expect(tracker.flushOnPause() == nil)
    }

    @Test func rejectsInsecureRemoteConfiguration() {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "http://example.com")!, model: "model")
        #expect(throws: AIClientError.self) { try config.validate() }
    }

    @Test func allowsLocalDevelopmentRelayOverHTTP() throws {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "http://localhost:8080/v1")!, model: "model")
        try config.validate()
    }

    @Test func allowsFetchingModelsBeforeSelectingOne() throws {
        let config = AIConfiguration(apiProtocol: .openAICompatible, baseURL: URL(string: "https://api.example.com/v1")!, model: "")
        try config.validate(requireModel: false)
        #expect(throws: AIClientError.self) { try config.validate() }
    }

    @Test func replacementRangeUsesOriginalLengthFromCursor() {
        let range = CommittedTextReplacement.range(
            originalUTF16Length: 6,
            selected: NSRange(location: 6, length: 0),
            origin: 0
        )
        #expect(range == NSRange(location: 0, length: 6))
    }

    @Test func replacementRangeDoesNotUseRewrittenLength() {
        let originalLength = ("shenme" as NSString).length
        let rewrittenLength = ("What?" as NSString).length
        let cursor = NSRange(location: originalLength, length: 0)
        let correct = CommittedTextReplacement.range(originalUTF16Length: originalLength, selected: cursor, origin: 0)
        let wrong = CommittedTextReplacement.range(originalUTF16Length: rewrittenLength, selected: cursor, origin: 0)
        #expect(correct == NSRange(location: 0, length: 6))
        #expect(wrong != correct)
    }

    @Test func looksFinishedIgnoresSingleWord() {
        #expect(SentenceCompletion.looksFinished("shenme") == false)
        #expect(SentenceCompletion.looksFinished("coffee") == false)
        #expect(SentenceCompletion.looksFinished("wo") == false)
    }

    @Test func looksFinishedIgnoresIncompletePhrases() {
        #expect(SentenceCompletion.looksFinished("I want to") == false)
        #expect(SentenceCompletion.looksFinished("I want to mai") == false)
        #expect(SentenceCompletion.looksFinished("wo xiang") == false)
        #expect(SentenceCompletion.looksFinished("mai coffee") == false)
        #expect(SentenceCompletion.looksFinished("Hello,") == false)
    }

    @Test func looksFinishedAcceptsCompleteSentences() {
        #expect(SentenceCompletion.looksFinished("I want to mai coffee") == true)
        #expect(SentenceCompletion.looksFinished("Can you help me") == true)
        #expect(SentenceCompletion.looksFinished("Hello?") == true)
        #expect(SentenceCompletion.looksFinished("I want to mai coffee.") == true)
    }

    @Test func parsesOpenAIChatContent() {
        let json = #"{"choices":[{"message":{"role":"assistant","content":"Is Breaking Bad a good show?"}}]}"#
        let text = CorrectionResponseParser.sentence(from: Data(json.utf8))
        #expect(text == "Is Breaking Bad a good show?")
    }

    @Test func parsesArrayContentAndJSONPayload() throws {
        let arrayJSON = #"{"choices":[{"message":{"content":[{"type":"text","text":"Is Breaking Bad a good show?"}]}}]}"#
        #expect(CorrectionResponseParser.sentence(from: Data(arrayJSON.utf8)) == "Is Breaking Bad a good show?")

        let inner = #"{"corrected_text":"Is Breaking Bad a good show?"}"#
        let payload: [String: Any] = [
            "choices": [
                ["message": ["content": inner]]
            ]
        ]
        let wrapped = try JSONSerialization.data(withJSONObject: payload)
        #expect(CorrectionResponseParser.sentence(from: wrapped) == "Is Breaking Bad a good show?")
    }

    @Test func recoversTruncatedJSONContent() {
        let truncated = "{" + "\"corrected_text\":" + "\"Is Breaking Bad a good show?\""
        #expect(CorrectionResponseParser.cleanedSentence(truncated) == "Is Breaking Bad a good show?")
    }

    @Test func ignoresEmptyContent() {
        let json = #"{"choices":[{"message":{"content":"","reasoning_content":"thinking"}}]}"#
        #expect(CorrectionResponseParser.sentence(from: Data(json.utf8)) == nil)
    }

    @Test func prefersContentOverReasoning() {
        let json = #"{"choices":[{"message":{"content":"Is Breaking Bad a good show?","reasoning_content":"We need answer. User asks juemingdushi"}}]}"#
        #expect(CorrectionResponseParser.sentence(from: Data(json.utf8)) == "Is Breaking Bad a good show?")
    }

    @Test func usesContentWhenReasoningIsLong() throws {
        let payload: [String: Any] = [
            "choices": [[
                "message": [
                    "content": "Is Breaking Bad a good show?",
                    "reasoning_content": String(repeating: "think ", count: 80)
                ]
            ]]
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        #expect(CorrectionResponseParser.sentence(from: data) == "Is Breaking Bad a good show?")
    }

    @Test func newlineDoesNotEndSentence() {
        var tracker = SentenceTracker()
        #expect(tracker.append("Can you help me\n").isEmpty)
        #expect(tracker.pendingText.contains("Can you help me"))
    }
}
