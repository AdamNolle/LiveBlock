import Foundation
import CoreVideo

/// Swift mirror of the Rust core's `region_is_protected_black(&Frame, NormRect,
/// luma_threshold)`. Detects regions that the system has blanked to black
/// because the content is HDCP/DRM-protected.
///
/// IMPORTANT — this is NOT circumvention. We are only *reading* whatever pixels
/// the capture API already handed us. When content is protected, the OS gives
/// us black where the protected pixels would be; this detector recognizes that
/// blanking so the pipeline can lay down a clean SOLID cover (via
/// `InpaintingEngine.paintOverPatches`) instead of mirror-blending smeared
/// black. We never attempt to recover or reveal the protected content.
///
/// Two capture situations:
///   1. Capture IS running and returns black for a protected sub-region (e.g.
///      a DRM video element on an otherwise-capturable page). This detector
///      finds those regions so we cover them flatly rather than smearing.
///   2. Capture is fully blocked / the whole frame is protected black. There
///      are no meaningful pixels to auto-detect against; in that case paint-over
///      is driven entirely by USER-marked regions (the capture-free PaintOver
///      path), and this detector simply reports the sampled regions as black.
enum ProtectedRegionDetector {

    /// Default luma threshold below which a sampled pixel counts as "black".
    /// Matches the conservative value used on the Rust side; protected blanking
    /// is a true 0, so a low threshold avoids false positives on genuinely dark
    /// (but real) content like a black UI background that we *could* mirror.
    static let defaultLumaThreshold: UInt8 = 16

    /// Fraction of sampled pixels that must be black for the region to be
    /// treated as protected. High (0.985) so near-solid black content the user
    /// legitimately drew over isn't misclassified — but DRM blanking, which is
    /// uniformly 0, sails past it.
    static let blackCoverageThreshold: Double = 0.985

    /// Returns `true` if the region of `pixelBuffer` described by `normRect`
    /// (normalized [0..1], TOP-LEFT origin — the same convention
    /// `NormalizedRegion` uses) is near-uniformly black.
    ///
    /// Samples a coarse grid (not every pixel) so this stays cheap enough to
    /// run per active region on the video queue. Assumes 32-bit BGRA, which is
    /// how `ScreenCaptureManager` configures the stream
    /// (`kCVPixelFormatType_32BGRA`). For any other pixel format it returns
    /// `false` (fail-safe: we'd rather mirror-blend than wrongly cover).
    static func regionIsProtectedBlack(_ pixelBuffer: CVPixelBuffer,
                                       normRect: CGRect,
                                       lumaThreshold: UInt8 = defaultLumaThreshold) -> Bool {
        guard CVPixelBufferGetPixelFormatType(pixelBuffer) == kCVPixelFormatType_32BGRA else {
            return false
        }
        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        guard width > 0, height > 0 else { return false }

        // Convert the normalized TOP-LEFT rect into pixel rows/cols. Pixel
        // buffers are row-major from the top, so top-left maps directly.
        let x0 = clampIndex(Int((normRect.minX * CGFloat(width)).rounded(.down)), max: width - 1)
        let y0 = clampIndex(Int((normRect.minY * CGFloat(height)).rounded(.down)), max: height - 1)
        let x1 = clampIndex(Int((normRect.maxX * CGFloat(width)).rounded(.up)), max: width)
        let y1 = clampIndex(Int((normRect.maxY * CGFloat(height)).rounded(.up)), max: height)
        guard x1 > x0, y1 > y0 else { return false }

        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else { return false }
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let ptr = base.assumingMemoryBound(to: UInt8.self)

        // Sample at most ~32x32 points across the region — enough to catch a
        // non-black pixel without scanning megapixels every frame.
        let maxSamplesPerAxis = 32
        let regionW = x1 - x0
        let regionH = y1 - y0
        let stepX = max(1, regionW / maxSamplesPerAxis)
        let stepY = max(1, regionH / maxSamplesPerAxis)

        var sampled = 0
        var black = 0
        var y = y0
        while y < y1 {
            let rowBase = y * bytesPerRow
            var x = x0
            while x < x1 {
                let pixel = rowBase + x * 4   // BGRA
                let b = ptr[pixel + 0]
                let g = ptr[pixel + 1]
                let r = ptr[pixel + 2]
                // Rec. 601 luma; integer math, no float per-pixel cost.
                let luma = (UInt(r) * 77 + UInt(g) * 150 + UInt(b) * 29) >> 8
                if luma <= UInt(lumaThreshold) { black += 1 }
                sampled += 1
                x += stepX
            }
            y += stepY
        }

        guard sampled > 0 else { return false }
        return Double(black) / Double(sampled) >= blackCoverageThreshold
    }

    private static func clampIndex(_ value: Int, max: Int) -> Int {
        if value < 0 { return 0 }
        if value > max { return max }
        return value
    }
}
