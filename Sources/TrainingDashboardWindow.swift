import AppKit
import SwiftUI

final class TrainingDashboardWindow: NSWindow, NSWindowDelegate {

    init(rootView: AnyView) {
        let initial = NSRect(x: 100, y: 100, width: 720, height: 560)
        super.init(contentRect: initial,
                   styleMask: [.titled, .closable, .miniaturizable, .resizable],
                   backing: .buffered,
                   defer: false)
        self.title = "LiveBlock — Training"
        self.minSize = NSSize(width: 600, height: 480)
        self.center()
        self.isReleasedWhenClosed = false
        self.collectionBehavior = [.fullScreenAllowsTiling]
        self.sharingType = .none

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        self.contentView = host
    }
}
