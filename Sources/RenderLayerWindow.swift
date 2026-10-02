import AppKit
import SwiftUI

/// Always-click-through full-display window.
///
/// Has NO interactive content. Renders inpainted patches and nothing else.
/// `ignoresMouseEvents` is hardcoded to `true` — there is no toggle, so
/// this window can never trap input no matter what state the rest of the
/// app is in.
final class RenderLayerWindow: NSPanel {

    init(rootView: AnyView, targetScreen: NSScreen) {
        // Use full screen.frame so the overlay pixel-aligns with the captured
        // frame (which is configured to screen.frame * scaleFactor).
        super.init(contentRect: targetScreen.frame,
                   styleMask: [.borderless, .nonactivatingPanel],
                   backing: .buffered,
                   defer: false)

        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = false
        // Sit above all normal app windows. `screenSaver` is the highest
        // non-system level we can safely use; .floating is below
        // full-screen apps so blocks would disappear behind YouTube etc.
        // Note: this level sits ABOVE the Region Editor's level, so the
        // Region Editor would be visually obscured. AppDelegate observes
        // controller.isEditorOpen and orders this window out while the
        // editor is open — see LiveBlockApp.swift.
        self.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.overlayWindow)))
        self.sharingType = .none
        self.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        self.ignoresMouseEvents = true   // permanent
        self.acceptsMouseMovedEvents = false
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

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
