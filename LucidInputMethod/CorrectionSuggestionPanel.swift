import AppKit

final class CorrectionSuggestionPanel: NSPanel {
    let originalText: String?
    let replaceableText: String?
    private let onReplace: () -> Void
    private let onKeep: () -> Void

    init(
        title: String,
        displayText: String,
        originalText: String?,
        replaceableText: String?,
        onReplace: @escaping () -> Void,
        onKeep: @escaping () -> Void
    ) {
        self.originalText = originalText
        self.replaceableText = replaceableText
        self.onReplace = onReplace
        self.onKeep = onKeep
        let showActions = replaceableText != nil
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 460, height: showActions ? 122 : 78),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        isFloatingPanel = true
        level = .statusBar
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        becomesKeyOnlyIfNeeded = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        contentView = makeContentView(title: title, text: displayText, showActions: showActions)
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    private func makeContentView(title: String, text: String, showActions: Bool) -> NSView {
        let container = NSVisualEffectView()
        container.material = .popover
        container.state = .active
        container.wantsLayer = true
        container.layer?.cornerRadius = 12
        container.layer?.masksToBounds = true

        let titleLabel = NSTextField(labelWithString: title)
        titleLabel.font = .systemFont(ofSize: 11, weight: .semibold)
        titleLabel.textColor = .secondaryLabelColor

        let body = NSTextField(wrappingLabelWithString: text)
        body.font = .systemFont(ofSize: 14)
        body.lineBreakMode = .byWordWrapping

        var views: [NSView] = [titleLabel, body]
        if showActions {
            let replaceButton = NSButton(title: "使用英文", target: self, action: #selector(replace))
            replaceButton.bezelStyle = .rounded
            replaceButton.keyEquivalent = "\r"
            let keepButton = NSButton(title: "保留原文", target: self, action: #selector(keep))
            keepButton.bezelStyle = .rounded
            let buttons = NSStackView(views: [replaceButton, keepButton])
            buttons.orientation = .horizontal
            buttons.spacing = 8
            views.append(buttons)
        }

        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 14, bottom: 12, right: 14)
        container.addSubview(stack)
        stack.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            stack.topAnchor.constraint(equalTo: container.topAnchor),
            stack.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            container.widthAnchor.constraint(greaterThanOrEqualToConstant: 360),
            container.widthAnchor.constraint(lessThanOrEqualToConstant: 520),
        ])
        return container
    }

    @objc private func replace() {
        onReplace()
    }

    @objc private func keep() {
        onKeep()
    }
}
