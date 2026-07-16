import AppKit
import SwiftUI

/// Small always-visible floating control window.
///
/// Sized like a HUD palette (Photos / CleanShot pinned controls). Has a
/// title bar with a close button (close hides, doesn't quit). Never traps
/// clicks outside its own ~380x220 frame, so it can never lock the user out.
final class ControlPanelWindow: NSPanel, NSWindowDelegate {
    private var suppressCloseHint = false

    init(rootView: AnyView) {
        let initial = Self.defaultFrame()
        super.init(contentRect: initial,
                   styleMask: [.titled, .closable, .resizable, .nonactivatingPanel, .utilityWindow, .hudWindow],
                   backing: .buffered,
                   defer: false)

        self.title = "LiveBlock"
        self.titlebarAppearsTransparent = false
        self.titleVisibility = .visible

        self.isFloatingPanel = true
        self.becomesKeyOnlyIfNeeded = false
        self.hidesOnDeactivate = false
        self.isReleasedWhenClosed = false
        self.level = .floating

        // Visible across spaces. NOT fullScreenAuxiliary — full-screen apps
        // are typically the user's foreground task; we shouldn't hover on top.
        // canJoinAllSpaces and moveToActiveSpace are mutually exclusive on
        // macOS 26; canJoinAllSpaces is what we want here.
        self.collectionBehavior = [.canJoinAllSpaces, .stationary]

        // Don't appear in our own ScreenCaptureKit output.
        self.sharingType = .none

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host
        self.minSize = NSSize(width: 360, height: 360)
        self.setContentSize(initial.size)

        self.standardWindowButton(.miniaturizeButton)?.isHidden = true
        self.standardWindowButton(.zoomButton)?.isHidden = true
        self.delegate = self
    }

    /// Prevent application termination from persisting the user-close hint or
    /// opening a modal alert while AppKit is closing windows.
    func prepareForShutdown() {
        suppressCloseHint = true
    }

    /// On the first time the user closes the Control Panel, show a local
    /// accessible reminder that the app keeps running in the menu bar. Using an
    /// NSAlert avoids both a surprise notification-permission prompt and the
    /// deprecated NSUserNotification API.
    func windowWillClose(_ notification: Notification) {
        guard !suppressCloseHint else { return }
        let key = "didShowMenuBarHint"
        guard !UserDefaults.standard.bool(forKey: key) else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, !self.suppressCloseHint,
                  !UserDefaults.standard.bool(forKey: key) else { return }
            UserDefaults.standard.set(true, forKey: key)
            let alert = NSAlert()
            alert.messageText = "LiveBlock keeps running"
            alert.informativeText = "Click the shield icon in your menu bar to bring the control panel back, or press \u{2318}\u{21E7}L to toggle blocking from anywhere."
            alert.alertStyle = .informational
            alert.addButton(withTitle: "Got it")
            alert.runModal()
        }
    }

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    /// Anchor the panel to the top-right of the active display, sized to
    /// fit naturally on small screens (12" MacBook ≈ 1280pt wide) while
    /// keeping the 400×480 default on anything bigger.
    private static func defaultFrame() -> NSRect {
        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let preferredW: CGFloat = 400
        let preferredH: CGFloat = 480
        // Cap to 80% of the screen so the panel can't fall off a 1280×800 lid.
        let w = max(360, min(preferredW, screen.width * 0.80))
        let h = max(360, min(preferredH, screen.height * 0.80))
        let inset: CGFloat = 20
        return NSRect(x: screen.maxX - w - inset,
                      y: screen.maxY - h - inset,
                      width: w,
                      height: h)
    }
}
