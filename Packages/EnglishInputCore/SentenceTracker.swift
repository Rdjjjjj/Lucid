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
        ".!?。？！\n\r".contains(character)
    }
}
