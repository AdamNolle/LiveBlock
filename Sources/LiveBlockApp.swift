import SwiftUI

@main
struct LiveBlockApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    
    var body: some Scene {
        // We do not use a standard WindowGroup because we need a custom NSWindow.
        // The AppDelegate will manage the overlay window lifecycle.
        Settings {
            Text("Settings: Currently Empty")
        }
    }
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var overlayWindow: OverlayWindow!
    
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Create the custom transparent overlay window
        let screenSize = NSScreen.main?.frame.size ?? CGSize(width: 800, height: 600)
        let initialRect = NSRect(x: screenSize.width / 2 - 400,
                                 y: screenSize.height / 2 - 300,
                                 width: 800,
                                 height: 600)
        
        overlayWindow = OverlayWindow(contentRect: initialRect)
        
        let overlayView = OverlayView()
        let hostingView = NSHostingView(rootView: overlayView)
        overlayWindow.contentView = hostingView
        
        overlayWindow.makeKeyAndOrderFront(nil)
    }
}
