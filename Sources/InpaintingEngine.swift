import Foundation
import CoreVideo
import CoreImage
import CoreImage.CIFilterBuiltins
import QuartzCore

struct InpaintPatch: Sendable, Equatable {
    /// Rect in [0..1] coords with origin top-left — ready to position in a SwiftUI overlay.
    let normalizedRect: CGRect
    /// Rendered patch — content-extrapolated fill.
    let image: CGImage

    static func == (lhs: InpaintPatch, rhs: InpaintPatch) -> Bool {
        // Identity-equal CGImages and identical rects → no SwiftUI rebuild.
        lhs.normalizedRect == rhs.normalizedRect && lhs.image === rhs.image
    }
}

/// Replaces target regions with a content-extrapolated fill.
///
/// Algorithm:
///  1. Pick a blend axis based on aspect ratio:
///     - wide regions (banner ads) → vertical mirror-blend (sample from above + below)
///     - tall regions (sidebars) → horizontal mirror-blend (sample from left + right)
///     - square-ish → try vertical first, then horizontal
///  2. Take a band of the surrounding frame the same size as the region.
///  3. Reflect each band across the region's adjacent edge.
///  4. Cross-fade the two reflections with a linear gradient mask.
///  5. If sampling bands aren't available (region near a screen edge), gracefully
///     fall back to a single-side reflection or, last resort, the average border colour.
///
/// This is dramatically better than a solid colour fill for:
///   - banner ads on a uniform page background (the page chrome above + below
///     extrapolates cleanly through the region)
///   - sidebar ads adjacent to consistent-width content
///
/// It is NOT a generative inpainter. For "reconstructs the dog behind the ad,"
/// see ASSESSMENT.md §4 / §8 Phase 7 (LaMa via Metal Performance Shaders).
final class InpaintingEngine: @unchecked Sendable {

    private let context: CIContext
    private let colorSpace: CGColorSpace

    init() {
        self.context = CIContext(options: [.useSoftwareRenderer: false])
        self.colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
    }

    /// A solid fill colour for a paint-over cover patch (premultiplied RGBA,
    /// 0...1). Mirrors the Rust core's `Fill::Solid`/`Fill::opaque_black()`.
    struct CoverFill: Sendable {
        let r: CGFloat
        let g: CGFloat
        let b: CGFloat
        let a: CGFloat
        /// Fully opaque black — the safe default cover for protected content.
        static let opaqueBlack = CoverFill(r: 0, g: 0, b: 0, a: 1)
    }

    /// DRM-SAFE paint-over: produce a flat, OPAQUE cover patch for each region
    /// WITHOUT reading the pixels underneath. This mirrors the Rust core's
    /// capture-free `paint_over_regions(..., Fill::opaque_black())`: when the
    /// content is protected (HDCP/DRM) the system blanks those pixels to black,
    /// so there is nothing meaningful to sample and the mirror-blend inpainter
    /// would only smear black. Instead we draw a solid cover the user can't see
    /// through. This is ordinary overlay drawing, not DRM circumvention — we
    /// never attempt to recover or reveal the protected pixels.
    ///
    /// `regions` carry pixel-space rects (bottom-left origin, CV space), the
    /// same convention `inpaintPatches` uses, so the normalized output rects
    /// line up 1:1 with the render overlay.
    func paintOverPatches(regions: [AdBoundingBox],
                          bufferSize: CGSize,
                          fill: CoverFill = .opaqueBlack) -> [InpaintPatch] {
        guard !regions.isEmpty, bufferSize.width > 0, bufferSize.height > 0 else { return [] }
        let extent = CGRect(x: 0, y: 0, width: bufferSize.width, height: bufferSize.height)
        let color = CGColor(colorSpace: colorSpace,
                            components: [fill.r, fill.g, fill.b, fill.a])
            ?? CGColor(gray: 0, alpha: 1)

        var patches: [InpaintPatch] = []
        patches.reserveCapacity(regions.count)
        for region in regions {
            let clamped = region.rect.intersection(extent)
            guard !clamped.isNull, clamped.width >= 2, clamped.height >= 2 else { continue }
            guard let image = renderSolidPatch(color: color, size: clamped.size) else { continue }
            // CV (bottom-left, pixel) → normalized top-left for the overlay.
            let norm = CGRect(x: clamped.minX / bufferSize.width,
                              y: (bufferSize.height - clamped.maxY) / bufferSize.height,
                              width: clamped.width / bufferSize.width,
                              height: clamped.height / bufferSize.height)
            patches.append(InpaintPatch(normalizedRect: norm, image: image))
        }
        return patches
    }

    /// Produce one patch per region. Empty input -> empty output.
    func inpaintPatches(frame: CVPixelBuffer, regions: [AdBoundingBox]) -> [InpaintPatch] {
        guard !regions.isEmpty else { return [] }

        let ciImage = CIImage(cvPixelBuffer: frame)
        let bufferW = CGFloat(CVPixelBufferGetWidth(frame))
        let bufferH = CGFloat(CVPixelBufferGetHeight(frame))
        let extent = CGRect(x: 0, y: 0, width: bufferW, height: bufferH)
        guard bufferW > 0, bufferH > 0 else { return [] }

        var patches: [InpaintPatch] = []
        patches.reserveCapacity(regions.count)

        for region in regions {
            let clamped = region.rect.intersection(extent)
            guard !clamped.isNull, clamped.width >= 2, clamped.height >= 2 else { continue }

            // Re-render every frame: cache by rect-only would return stale fills
            // when content scrolls / video plays behind a static region. The
            // ScreenCaptureManager throttle (≤30 Hz) bounds how often this runs.
            guard let image = renderFill(image: ciImage, rect: clamped, frameExtent: extent) else {
                continue
            }

            // Convert from CV (bottom-left, pixel) → normalized top-left.
            let norm = CGRect(x: clamped.minX / bufferW,
                              y: (bufferH - clamped.maxY) / bufferH,
                              width: clamped.width / bufferW,
                              height: clamped.height / bufferH)
            patches.append(InpaintPatch(normalizedRect: norm, image: image))
        }
        return patches
    }

    // MARK: - Fill rendering

    private func renderFill(image: CIImage, rect: CGRect, frameExtent: CGRect) -> CGImage? {
        let aspect = rect.width / max(rect.height, 1)
        let preferVertical = aspect >= 1.0  // wider than tall → blend top↔bottom

        // Try preferred axis, then the other, then graceful fallback.
        let axes: [MirrorAxis] = preferVertical ? [.vertical, .horizontal] : [.horizontal, .vertical]
        for axis in axes {
            if let cg = mirrorBlendPatch(image: image, rect: rect, frameExtent: frameExtent, axis: axis) {
                return cg
            }
        }

        // Last resort: edge-color fill (the previous behaviour).
        let color = averageBorderColor(in: image, around: rect, frameExtent: frameExtent)
        return renderSolidPatch(color: color, size: rect.size)
    }

    private enum MirrorAxis { case vertical, horizontal }
    private enum BandSide { case near, far }

    private func mirrorBlendPatch(image: CIImage,
                                  rect: CGRect,
                                  frameExtent: CGRect,
                                  axis: MirrorAxis) -> CGImage? {
        let nearBandRect: CGRect
        let farBandRect: CGRect

        switch axis {
        case .vertical:
            // CV space: bottom-left origin. "Near" = above region (higher y).
            nearBandRect = CGRect(x: rect.minX, y: rect.maxY,
                                  width: rect.width, height: rect.height)
            farBandRect  = CGRect(x: rect.minX, y: rect.minY - rect.height,
                                  width: rect.width, height: rect.height)
        case .horizontal:
            // "Near" = right of region.
            nearBandRect = CGRect(x: rect.maxX, y: rect.minY,
                                  width: rect.width, height: rect.height)
            farBandRect  = CGRect(x: rect.minX - rect.width, y: rect.minY,
                                  width: rect.width, height: rect.height)
        }

        let nearAvail = bandAvailable(nearBandRect, in: frameExtent)
        let farAvail = bandAvailable(farBandRect, in: frameExtent)
        guard nearAvail || farAvail else { return nil }

        let nearReflection = nearAvail
            ? reflectBand(image: image, bandRect: nearBandRect, into: rect, axis: axis, side: .near)
            : nil
        let farReflection = farAvail
            ? reflectBand(image: image, bandRect: farBandRect, into: rect, axis: axis, side: .far)
            : nil

        let composed: CIImage
        if let n = nearReflection, let f = farReflection {
            guard let mask = blendMask(rect: rect, axis: axis) else { return nil }
            let blend = CIFilter.blendWithMask()
            blend.inputImage = n            // shows where mask is white (near edge)
            blend.backgroundImage = f       // shows where mask is black (far edge)
            blend.maskImage = mask
            guard let output = blend.outputImage else { return nil }
            composed = output
        } else if let n = nearReflection {
            composed = n
        } else if let f = farReflection {
            composed = f
        } else {
            return nil
        }

        return context.createCGImage(composed, from: rect)
    }

    private func bandAvailable(_ band: CGRect, in extent: CGRect) -> Bool {
        let clipped = band.intersection(extent)
        guard !clipped.isNull && !clipped.isEmpty else { return false }
        // We need at least 60% of the band inside the frame to mirror cleanly.
        let bandArea = band.width * band.height
        let clippedArea = clipped.width * clipped.height
        return bandArea > 0 && clippedArea / bandArea > 0.6
    }

    private func reflectBand(image: CIImage,
                             bandRect: CGRect,
                             into target: CGRect,
                             axis: MirrorAxis,
                             side: BandSide) -> CIImage? {
        let cropped = image.cropped(to: bandRect.intersection(image.extent))
        guard !cropped.extent.isNull, !cropped.extent.isEmpty else { return nil }

        switch axis {
        case .vertical:
            // Reflection axis is the region's near edge (top in CV when side==.near, bottom when .far)
            let axisY: CGFloat = side == .near ? target.maxY : target.minY
            return cropped
                .transformed(by: CGAffineTransform(scaleX: 1, y: -1))
                .transformed(by: CGAffineTransform(translationX: 0, y: 2 * axisY))
        case .horizontal:
            let axisX: CGFloat = side == .near ? target.maxX : target.minX
            return cropped
                .transformed(by: CGAffineTransform(scaleX: -1, y: 1))
                .transformed(by: CGAffineTransform(translationX: 2 * axisX, y: 0))
        }
    }

    private func blendMask(rect: CGRect, axis: MirrorAxis) -> CIImage? {
        let gradient = CIFilter.linearGradient()
        switch axis {
        case .vertical:
            // Near = top (max y) → white; far = bottom (min y) → black
            gradient.point0 = CGPoint(x: rect.midX, y: rect.maxY)
            gradient.point1 = CGPoint(x: rect.midX, y: rect.minY)
        case .horizontal:
            // Near = right (max x) → white; far = left (min x) → black
            gradient.point0 = CGPoint(x: rect.maxX, y: rect.midY)
            gradient.point1 = CGPoint(x: rect.minX, y: rect.midY)
        }
        gradient.color0 = CIColor.white
        gradient.color1 = CIColor.black
        return gradient.outputImage?.cropped(to: rect)
    }

    // MARK: - Fallback (edge-colour fill)

    private func averageBorderColor(in image: CIImage, around rect: CGRect, frameExtent: CGRect) -> CGColor {
        let inset = max(4.0, min(rect.width, rect.height) * 0.04)
        let strips = [
            CGRect(x: rect.minX - inset, y: rect.maxY,        width: rect.width + 2 * inset, height: inset),
            CGRect(x: rect.minX - inset, y: rect.minY - inset, width: rect.width + 2 * inset, height: inset),
            CGRect(x: rect.minX - inset, y: rect.minY,         width: inset,                  height: rect.height),
            CGRect(x: rect.maxX,         y: rect.minY,         width: inset,                  height: rect.height),
        ]
        .map { $0.intersection(frameExtent) }
        .filter { !$0.isNull && !$0.isEmpty }

        var sumR: CGFloat = 0, sumG: CGFloat = 0, sumB: CGFloat = 0, weight: CGFloat = 0
        for strip in strips {
            guard let sample = sampleAverage(image: image, in: strip) else { continue }
            let w = strip.width * strip.height
            sumR += sample.r * w; sumG += sample.g * w; sumB += sample.b * w
            weight += w
        }
        guard weight > 0 else { return CGColor(gray: 0, alpha: 1) }
        return CGColor(colorSpace: colorSpace,
                       components: [sumR / weight, sumG / weight, sumB / weight, 1.0])
            ?? CGColor(gray: 0, alpha: 1)
    }

    private struct RGB { let r: CGFloat; let g: CGFloat; let b: CGFloat }

    private func sampleAverage(image: CIImage, in rect: CGRect) -> RGB? {
        let filter = CIFilter.areaAverage()
        filter.inputImage = image
        filter.extent = rect
        guard let output = filter.outputImage else { return nil }

        var bitmap = [UInt8](repeating: 0, count: 4)
        context.render(output,
                       toBitmap: &bitmap,
                       rowBytes: 4,
                       bounds: CGRect(x: 0, y: 0, width: 1, height: 1),
                       format: .RGBA8,
                       colorSpace: colorSpace)
        return RGB(r: CGFloat(bitmap[0]) / 255.0,
                   g: CGFloat(bitmap[1]) / 255.0,
                   b: CGFloat(bitmap[2]) / 255.0)
    }

    private func renderSolidPatch(color: CGColor, size: CGSize) -> CGImage? {
        let width = max(1, Int(size.width.rounded()))
        let height = max(1, Int(size.height.rounded()))
        guard let ctx = CGContext(data: nil,
                                  width: width,
                                  height: height,
                                  bitsPerComponent: 8,
                                  bytesPerRow: width * 4,
                                  space: colorSpace,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            return nil
        }
        ctx.setFillColor(color)
        ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
        return ctx.makeImage()
    }
}
