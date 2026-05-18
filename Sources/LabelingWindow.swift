import AppKit
import SwiftUI

/// A regular resizable window for batch-labeling captured screenshots.
///
/// Standard chrome (titlebar, traffic lights). Closes via the red close
/// button — `onWillClose` fires first so the controller can flush
/// in-progress edits to disk.
final class LabelingWindow: NSWindow, NSWindowDelegate {

    var onWillClose: (() -> Void)?

    init(rootView: AnyView) {
        let initial = NSRect(x: 100, y: 100, width: 1100, height: 760)
        super.init(contentRect: initial,
                   styleMask: [.titled, .closable, .miniaturizable, .resizable],
                   backing: .buffered,
                   defer: false)
        self.title = "LiveBlock — Label Screenshots"
        self.minSize = NSSize(width: 720, height: 520)
        self.center()
        self.isReleasedWhenClosed = false
        self.collectionBehavior = [.fullScreenAllowsTiling]
        self.sharingType = .none  // don't appear in our own SCStream
        self.delegate = self

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host
    }

    func windowWillClose(_ notification: Notification) {
        onWillClose?()
    }
}
