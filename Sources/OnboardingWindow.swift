import AppKit
import SwiftUI

/// First-run onboarding window. 3-step welcome → permissions → first-block.
/// Shown automatically on first launch when `UserDefaults` flag is unset;
/// can also be re-opened from the menu bar.
final class OnboardingWindow: NSWindow, NSWindowDelegate {
    init(rootView: AnyView) {
        super.init(contentRect: NSRect(x: 100, y: 100, width: 540, height: 640),
                   styleMask: [.titled, .closable, .resizable],
                   backing: .buffered,
                   defer: false)
        self.title = "Welcome to LiveBlock"
        self.minSize = NSSize(width: 480, height: 560)
        self.isReleasedWhenClosed = false
        self.center()
        self.collectionBehavior = [.fullScreenAllowsTiling]
        self.sharingType = .none

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host
        self.delegate = self
    }

    func windowWillClose(_ notification: Notification) {
        UserDefaults.standard.set(true, forKey: "didOnboard")
    }
}
