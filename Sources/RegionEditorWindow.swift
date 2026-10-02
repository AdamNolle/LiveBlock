import AppKit
import SwiftUI

/// Full-screen drawing window — only on screen while the user is editing.
///
/// Captures clicks (that's its job: drawing rectangles). Has a thin always-
/// visible top toolbar with a Done button and Esc handling. Closing the
/// window is reversible — the editor is opened/closed by `AppController`.
final class RegionEditorWindow: NSPanel {

    init(rootView: AnyView, targetScreen: NSScreen) {
        // Use full screen.frame (not visibleFrame) so coordinates line up
        // 1:1 with the captured frame's pixel space. The capture pipeline
        // configures SCStream with screen.frame * scaleFactor.
        super.init(contentRect: targetScreen.frame,
                   styleMask: [.borderless, .nonactivatingPanel],
                   backing: .buffered,
                   defer: false)

        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = false
        self.level = NSWindow.Level(rawValue: NSWindow.Level.floating.rawValue + 1)
        self.sharingType = .none
        // canJoinAllSpaces and moveToActiveSpace are mutually exclusive
        // on macOS 26 — the runtime asserts. Pick canJoinAllSpaces so the
        // editor appears on whatever Space the user is on.
        self.collectionBehavior = [.canJoinAllSpaces]
        self.titleVisibility = .hidden
        self.titlebarAppearsTransparent = true
        self.isMovable = false
        self.isReleasedWhenClosed = false

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host

        align(to: targetScreen)
        self.orderOut(nil)
    }

    func align(to screen: NSScreen) {
        setFrame(screen.frame, display: true, animate: false)
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
