import AppKit
import SwiftUI

/// Small floating HUD shown while LiveBlocker is actively capturing.
/// Mirrors the design's "Mini HUD" screen — Logo + status + region count
/// + pause button. Click-through-respecting; can be dragged.
final class MiniHUDWindow: NSPanel {
    init(rootView: AnyView) {
        let initialSize = Self.idealSize(for: NSScreen.main)
        super.init(contentRect: NSRect(origin: .zero, size: initialSize),
                   styleMask: [.borderless, .nonactivatingPanel],
                   backing: .buffered,
                   defer: false)

        self.isFloatingPanel = true
        self.becomesKeyOnlyIfNeeded = true
        self.hidesOnDeactivate = false
        self.isReleasedWhenClosed = false
        self.level = .floating
        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = true
        self.collectionBehavior = [.canJoinAllSpaces, .stationary]
        self.sharingType = .none
        self.isMovableByWindowBackground = true

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host

        // Default position: bottom-center of the main screen.
        if let screen = NSScreen.main {
            let f = screen.visibleFrame
            let s = Self.idealSize(for: screen)
            self.setFrame(NSRect(x: f.midX - s.width / 2,
                                  y: f.minY + 24,
                                  width: s.width,
                                  height: s.height),
                          display: true)
        }
    }

    /// Default 380×90; clamped down on small displays so the HUD never
    /// hogs more than 30% of the screen width.
    private static func idealSize(for screen: NSScreen?) -> NSSize {
        let preferred = NSSize(width: 380, height: 90)
        guard let screen else { return preferred }
        let maxWidth = screen.visibleFrame.width * 0.30
        let width = min(preferred.width, max(280, maxWidth))
        let height = preferred.height
        return NSSize(width: width, height: height)
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
