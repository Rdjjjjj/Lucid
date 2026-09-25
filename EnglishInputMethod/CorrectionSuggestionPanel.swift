import AppKit
import EnglishInputCore
import InputMethodKit

@MainActor
final class CorrectionSuggestionPanel: NSPanel {
    private let correctedText: String
    private let onReplace: () -> Void
    private let onKeep: () -> Void

    init(correctedText: String, onReplace: @escaping () -> Void, onKeep: @escaping () -> Void) {
        self.correctedText = correctedText
        self.onReplace = onReplace
        self.onKeep = onKeep
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 96),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        isFloatingPanel = true
        level = .floating
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        becomesKeyOnlyIfNeeded = true
        contentView = makeContentView()
    }

    private func makeContentView() -> NSView {
        let container = NSVisualEffectView()
        container.material = .popover
        container.state = .active
        container.wantsLayer = true
        container.layer?.cornerRadius = 10

        let label = NSTextField(wrappingLabelWithString: correctedText)
        label.font = .systemFont(ofSize: 13)
        label.lineBreakMode = .byWordWrapping

        let replaceButton = NSButton(title: "替换", target: self, action: #selector(replace))
        replaceButton.bezelStyle = .rounded
        replaceButton.keyEquivalent = "\r"
        let keepButton = NSButton(title: "保留原文", target: self, action: #selector(keep))
        keepButton.bezelStyle = .rounded

        let buttons = NSStackView(views: [replaceButton, keepButton])
        buttons.orientation = .horizontal
        buttons.spacing = 8
        buttons.alignment = .centerY

        let stack = NSStackView(views: [label, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 14, bottom: 12, right: 14)
        container.addSubview(stack)
        stack.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            stack.topAnchor.constraint(equalTo: container.topAnchor),
            stack.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        return container
    }

    @objc private func replace() {
        orderOut(nil)
        onReplace()
    }

    @objc private func keep() {
        orderOut(nil)
        onKeep()
    }
}
