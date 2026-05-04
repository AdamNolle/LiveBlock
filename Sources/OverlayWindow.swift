import AppKit
import SwiftUI

class OverlayWindow: NSWindow {
    
    init(contentRect: NSRect) {
        // Borderless, transparent, floating window
        super.init(contentRect: contentRect,
                   styleMask: [.borderless, .resizable],
                   backing: .buffered,
                   defer: false)
        
        self.isOpaque = false
        self.backgroundColor = .clear
        self.level = .floating // Stays above normal windows
        
        // Privacy: do not show this window in screen captures
        self.sharingType = .none
        
        // We want the window to ignore mouse events so the user can interact with the video beneath it,
        // BUT we need a way to move/resize it. 
        // We will start by letting it accept events, and the SwiftUI view will handle a "Control Mode" toggle.
        self.ignoresMouseEvents = false
        self.hasShadow = false
        self.titlebarAppearsTransparent = true
        self.titleVisibility = .hidden
        
        // Make the window movable by clicking anywhere on its background
        self.isMovableByWindowBackground = true
    }
    
    // Custom property to toggle passthrough
    var isControlModeActive: Bool = true {
        didSet {
            // When Control Mode is OFF, mouse events pass through to the underlying apps
            self.ignoresMouseEvents = !isControlModeActive
        }
    }
    
    override var canBecomeKey: Bool {
        return true
    }
    
    override var canBecomeMain: Bool {
        return true
    }
}
