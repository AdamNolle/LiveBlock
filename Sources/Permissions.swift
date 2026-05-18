import AppKit
import CoreGraphics
import ApplicationServices

/// Centralized helpers for the two macOS permissions LiveBlock needs:
/// Screen Recording (for SCStream capture) and Accessibility (for global
/// hotkeys via NSEvent.addGlobalMonitorForEvents).
///
/// Both are checked synchronously and cheaply; we poll on app activation
/// rather than subscribing because TCC does not broadcast change events.
enum Permissions {

    enum Kind {
        case screenRecording
        case accessibility
    }

    /// Returns true if the user has granted Screen Recording in
    /// System Settings → Privacy & Security → Screen Recording.
    /// Does NOT trigger the system prompt on its own.
    static func screenRecordingGranted() -> Bool {
        CGPreflightScreenCaptureAccess()
    }

    /// Returns true if the app is in the Accessibility allowlist.
    /// Does NOT trigger the system prompt on its own.
    static func accessibilityGranted() -> Bool {
        AXIsProcessTrusted()
    }

    /// Triggers the system Accessibility prompt the first time. The system
    /// returns the current trust state and, if not already granted, shows
    /// the modal asking the user to add this app under Privacy → Accessibility.
    /// Subsequent calls are no-ops if already prompted.
    @discardableResult
    static func requestAccessibility() -> Bool {
        let key = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        let opts = [key: true] as CFDictionary
        return AXIsProcessTrustedWithOptions(opts)
    }

    /// Triggers the system prompt for Screen Recording. Does nothing if
    /// the user has previously denied — they must toggle it back on in
    /// System Settings. Use `openSystemSettings(.screenRecording)` to
    /// open the right pane.
    static func requestScreenRecording() {
        CGRequestScreenCaptureAccess()
    }

    /// Opens the System Settings pane for the given permission. Falls
    /// back to the top-level Privacy & Security pane if the deep link
    /// is rejected.
    static func openSystemSettings(_ kind: Kind) {
        let urlString: String = {
            switch kind {
            case .screenRecording:
                return "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            case .accessibility:
                return "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        }()
        if let url = URL(string: urlString) {
            NSWorkspace.shared.open(url)
        }
    }
}
