import AppKit

/// A click-anywhere view so an information-only panel can be dismissed by
/// clicking it, the way a transient tooltip behaves.
private final class DismissOnClickView: NSVisualEffectView {
    var onClick: (() -> Void)?

    override func mouseDown(with event: NSEvent) {
        onClick?()
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .arrow)
    }
}

final class CorrectionSuggestionPanel: NSPanel, NSWindowDelegate {
    let originalText: String?
    let replaceableText: String?
    private let onReplace: () -> Void
    private let onKeep: () -> Void
    /// Status panels dismiss without cancelling an in-flight rewrite. Suggestion
    /// panels still discard the pending sentence when the user keeps the original.
    private let cancelsRequestOnDismiss: Bool
    private var autoDismissTimer: Timer?

    init(
        title: String,
        displayText: String,
        originalText: String?,
        replaceableText: String?,
        cancelsRequestOnDismiss: Bool = true,
        onReplace: @escaping () -> Void,
        onKeep: @escaping () -> Void
    ) {
        self.originalText = originalText
        self.replaceableText = replaceableText
        self.cancelsRequestOnDismiss = cancelsRequestOnDismiss
        self.onReplace = onReplace
        self.onKeep = onKeep

        let showActions = replaceableText != nil
        let panelSize = Self.panelSize(for: displayText, showActions: showActions)
        super.init(
            contentRect: NSRect(origin: .zero, size: panelSize),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        isFloatingPanel = true
        // A toast should never sit at the system status-bar level: that level
        // can cover other apps and makes a harmless status message feel modal.
        // Floating is still above the editor but keeps the overlay local.
        level = .floating
        ignoresMouseEvents = !showActions
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        becomesKeyOnlyIfNeeded = true
        hidesOnDeactivate = true
        isReleasedWhenClosed = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        contentView = makeContentView(title: title, text: displayText, showActions: showActions, size: panelSize)
        // Auto Layout can otherwise resize an NSPanel to the wrapping label's
        // unbounded intrinsic width. Re-apply the intended size after installing
        // the content view so a short sentence never becomes a screen-wide panel.
        setContentSize(panelSize)
        minSize = panelSize
        maxSize = panelSize

        // Information-only panels carry no buttons, so they would otherwise stay
        // on screen forever. Suggestion panels also get a generous timeout: if
        // the user continues typing or switches apps, the pending sentence is
        // safely kept and the overlay disappears instead of blocking the editor.
        let duration: TimeInterval
        if showActions {
            duration = 12.0
        } else {
            duration = displayText.hasPrefix("正在") ? 1.2 : 3.0
        }
        autoDismissTimer = Timer.scheduledTimer(withTimeInterval: duration, repeats: false) { [weak self] _ in
            self?.dismiss()
        }
    }

    deinit {
        autoDismissTimer?.invalidate()
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    /// Hides the panel and tells the controller to drop its state. Kept separate
    /// from `orderOut` so the auto-dismiss timer and the buttons share one path.
    func dismiss() {
        autoDismissTimer?.invalidate()
        autoDismissTimer = nil
        if cancelsRequestOnDismiss {
            onKeep()
        } else {
            orderOut(nil)
        }
    }

    private func makeContentView(title: String, text: String, showActions: Bool, size: NSSize) -> NSView {
        let container = DismissOnClickView()
        container.material = .popover
        container.state = .active
        container.wantsLayer = true
        container.layer?.cornerRadius = 12
        container.layer?.masksToBounds = true

        let titleLabel = NSTextField(labelWithString: title)
        titleLabel.font = .systemFont(ofSize: 11, weight: .semibold)
        titleLabel.textColor = .secondaryLabelColor

        let bodyFont = NSFont.systemFont(ofSize: showActions ? 14 : 13)
        let body = NSTextField(wrappingLabelWithString: text)
        body.font = bodyFont
        body.lineBreakMode = .byTruncatingTail
        body.maximumNumberOfLines = showActions ? 3 : 2
        body.preferredMaxLayoutWidth = size.width - (showActions ? 28 : 24)

        let horizontalPadding: CGFloat = showActions ? 14 : 12
        let verticalPadding: CGFloat = showActions ? 10 : 9
        let textWidth = size.width - horizontalPadding * 2
        body.translatesAutoresizingMaskIntoConstraints = false
        var views: [NSView] = showActions ? [titleLabel, body] : [body]
        if showActions {
            let replaceButton = NSButton(title: "使用英文", target: self, action: #selector(replace))
            replaceButton.bezelStyle = .rounded
            replaceButton.keyEquivalent = "\r"
            let keepButton = NSButton(title: "保留原文", target: self, action: #selector(keep))
            keepButton.bezelStyle = .rounded
            let buttons = NSStackView(views: [replaceButton, keepButton])
            buttons.orientation = .horizontal
            buttons.spacing = 8
            buttons.distribution = .fillEqually
            views.append(buttons)
        }

        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = showActions ? 7 : 0
        stack.edgeInsets = NSEdgeInsets(top: verticalPadding, left: horizontalPadding, bottom: verticalPadding, right: horizontalPadding)
        container.addSubview(stack)
        stack.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            stack.topAnchor.constraint(equalTo: container.topAnchor),
            stack.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            body.widthAnchor.constraint(equalToConstant: textWidth),
            container.widthAnchor.constraint(equalToConstant: size.width),
            container.heightAnchor.constraint(equalToConstant: size.height),
        ])
        return container
    }

    private static func panelSize(for text: String, showActions: Bool) -> NSSize {
        let width: CGFloat = showActions ? 360 : 260
        let horizontalPadding: CGFloat = showActions ? 28 : 24
        let bodyFont = NSFont.systemFont(ofSize: showActions ? 14 : 13)
        let textWidth = width - horizontalPadding
        let measured = (text as NSString).boundingRect(
            with: NSSize(width: textWidth, height: .greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading],
            attributes: [.font: bodyFont]
        ).height
        let bodyLineHeight = bodyFont.ascender - bodyFont.descender + bodyFont.leading
        let maxBodyHeight = bodyLineHeight * CGFloat(showActions ? 3 : 2)
        let bodyHeight = min(max(measured, bodyLineHeight), maxBodyHeight)
        let height: CGFloat
        if showActions {
            // title + body + buttons + compact vertical insets
            height = min(max(108, 10 + 14 + 7 + bodyHeight + 8 + 30 + 10), 150)
        } else {
            height = min(max(44, bodyHeight + 18), 62)
        }
        return NSSize(width: width, height: ceil(height))
    }

    func windowShouldBecomeKey(_ window: NSWindow) -> Bool {
        false
    }

    @objc private func replace() {
        autoDismissTimer?.invalidate()
        autoDismissTimer = nil
        onReplace()
    }

    @objc private func keep() {
        dismiss()
    }
}
