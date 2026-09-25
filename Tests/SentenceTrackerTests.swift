import Foundation
import Testing
@testable import EnglishInputCore

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
}
