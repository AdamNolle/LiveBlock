import AppKit
import CoreGraphics

/// Stable, serializable identity and geometry for one macOS display.
///
/// `NSScreen` instances are topology snapshots and can become stale across
/// sleep/hot-plug. Persist the CG display ID instead and resolve a fresh screen
/// whenever capture starts.
struct DisplayDescriptor: Identifiable, Equatable, Sendable {
    let id: CGDirectDisplayID
    let name: String
    let pointFrame: CGRect
    let pixelWidth: Int
    let pixelHeight: Int
    let isMain: Bool

    var pixelSize: CGSize {
        CGSize(width: pixelWidth, height: pixelHeight)
    }

    var scaleDescription: String {
        guard pointFrame.width > 0 else { return "unknown scale" }
        let scale = Double(pixelWidth) / pointFrame.width
        return String(format: "%.2f×", scale)
    }

    var menuLabel: String {
        "\(name) · \(pixelWidth)×\(pixelHeight) · \(scaleDescription)"
    }

    static func from(_ screen: NSScreen, mainDisplayID: CGDirectDisplayID?) -> DisplayDescriptor? {
        guard let id = screen.liveBlockDisplayID else { return nil }
        let authoritativeWidth = CGDisplayPixelsWide(id)
        let authoritativeHeight = CGDisplayPixelsHigh(id)
        let fallbackWidth = Int((screen.frame.width * screen.backingScaleFactor).rounded())
        let fallbackHeight = Int((screen.frame.height * screen.backingScaleFactor).rounded())
        return DisplayDescriptor(
            id: id,
            name: screen.localizedName,
            pointFrame: screen.frame,
            pixelWidth: authoritativeWidth > 0 ? authoritativeWidth : fallbackWidth,
            pixelHeight: authoritativeHeight > 0 ? authoritativeHeight : fallbackHeight,
            isMain: id == mainDisplayID
        )
    }
}

enum DisplayTargetResolver {
    /// Preserve an available explicit target. If it disappeared, fail over in
    /// a deterministic order: main display, then lowest display ID.
    static func resolvedID(preferred: CGDirectDisplayID?,
                           descriptors: [DisplayDescriptor]) -> CGDirectDisplayID? {
        if let preferred, descriptors.contains(where: { $0.id == preferred }) {
            return preferred
        }
        if let main = descriptors.first(where: \.isMain) {
            return main.id
        }
        return descriptors.min(by: { $0.id < $1.id })?.id
    }

    static func descriptor(id: CGDirectDisplayID,
                           in descriptors: [DisplayDescriptor]) -> DisplayDescriptor? {
        descriptors.first(where: { $0.id == id })
    }
}

extension NSScreen {
    var liveBlockDisplayID: CGDirectDisplayID? {
        (deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
    }

    static func liveBlockScreen(id: CGDirectDisplayID) -> NSScreen? {
        screens.first(where: { $0.liveBlockDisplayID == id })
    }

    static func liveBlockDescriptors() -> [DisplayDescriptor] {
        let mainID = main?.liveBlockDisplayID
        return screens.compactMap { DisplayDescriptor.from($0, mainDisplayID: mainID) }
    }
}
