import AppKit

/// Lightweight global+local hotkey wrapper around `NSEvent` monitors.
///
/// - The global monitor handles keystrokes that originate in *other* apps
///   (requires Accessibility permission).
/// - The local monitor handles keystrokes when LiveBlock has focus AND
///   *consumes the event* if a binding fires, so SwiftUI `.keyboardShortcut`
///   modifiers with the same combo don't double-fire.
@MainActor
final class HotKeyMonitor {

    typealias HotKey = (key: String, flags: NSEvent.ModifierFlags)
    typealias Handler = () -> Void

    private struct Binding {
        let hotKey: HotKey
        let handler: Handler
    }

    private var bindings: [Binding] = []
    private var globalMonitor: Any?
    private var localMonitor: Any?

    func register(key: String, flags: NSEvent.ModifierFlags, handler: @escaping Handler) {
        bindings.append(Binding(hotKey: (key.lowercased(), flags), handler: handler))
    }

    func start() {
        stop()
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
            _ = self?.dispatch(event)
        }
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self else { return event }
            // Returning nil consumes the event so SwiftUI's keyboardShortcut
            // doesn't fire the same handler a second time.
            return self.dispatch(event) ? nil : event
        }
    }

    func stop() {
        if let g = globalMonitor { NSEvent.removeMonitor(g); globalMonitor = nil }
        if let l = localMonitor { NSEvent.removeMonitor(l); localMonitor = nil }
    }

    deinit {
        if let g = globalMonitor { NSEvent.removeMonitor(g) }
        if let l = localMonitor { NSEvent.removeMonitor(l) }
    }

    /// Returns `true` if any binding fired (event should be consumed).
    @discardableResult
    private func dispatch(_ event: NSEvent) -> Bool {
        let pressed = event.charactersIgnoringModifiers?.lowercased() ?? ""
        let mask = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        for binding in bindings {
            // Exact-match on modifiers — `contains` would let any superset
            // (e.g. ⌘⇧⌥B) trigger the ⌘⇧B binding, which clashes with the
            // panic ⌘⇧⌥. hotkey.
            if binding.hotKey.key == pressed && mask == binding.hotKey.flags {
                binding.handler()
                return true
            }
        }
        return false
    }
}
