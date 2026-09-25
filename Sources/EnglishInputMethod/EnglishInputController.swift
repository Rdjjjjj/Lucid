import AppKit
import InputMethodKit

/// Minimal native InputMethodKit bridge. Keystrokes are committed immediately as ordinary text.
/// Sentence correction is intentionally kept out of this pass-through until replacement behavior is verified per client app.
@objc(EnglishInputController)
public final class EnglishInputController: IMKInputController {
    public override func inputText(_ string: String?, key keyCode: Int, modifiers flags: Int, client sender: Any?) -> Bool {
        guard let string, !string.isEmpty, let textClient = sender as? IMKTextInput else { return false }
        textClient.insertText(string, replacementRange: NSRange(location: NSNotFound, length: NSNotFound))
        return true
    }
}
