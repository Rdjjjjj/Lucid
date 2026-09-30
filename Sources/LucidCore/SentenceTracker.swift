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
