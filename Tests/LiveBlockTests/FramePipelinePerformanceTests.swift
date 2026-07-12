import XCTest
@testable import LiveBlock

final class FramePipelinePerformanceTests: XCTestCase {
    func testInpaintingPerformance1080pFourRegions() {
        let engine = InpaintingEngine()
        let buffer = makePixelBuffer(width: 1920, height: 1080)
        let regions = [
            CGRect(x: 120, y: 120, width: 360, height: 160),
            CGRect(x: 620, y: 180, width: 300, height: 180),
            CGRect(x: 1_050, y: 500, width: 420, height: 150),
            CGRect(x: 1_520, y: 240, width: 180, height: 360),
        ].map {
            AdBoundingBox(rect: $0, confidence: 1, label: "benchmark", source: .user)
        }

        // Records a machine-specific baseline without imposing a brittle CI
        // threshold. Release profiling and Instruments traces remain the
        // authoritative frame-budget gate.
        measure(metrics: [XCTClockMetric(), XCTCPUMetric(), XCTMemoryMetric()]) {
            let patches = engine.inpaintPatches(frame: buffer, regions: regions, style: .smart)
            XCTAssertEqual(patches.count, regions.count)
        }
    }

    func testRenderSingleFlightGateDropsConcurrentClaim() {
        let gate = AtomicBool(false)
        XCTAssertTrue(gate.trySetTrue())
        XCTAssertFalse(gate.trySetTrue())
        gate.set(false)
        XCTAssertTrue(gate.trySetTrue())
    }

    func testStaticFrameReuseStillRendersConfigurationChanges() {
        let tracker = RenderStateTracker()
        let first = AdBoundingBox(rect: CGRect(x: 10, y: 20, width: 100, height: 40),
                                  confidence: 1,
                                  label: "manual",
                                  source: .user)

        XCTAssertTrue(tracker.shouldRender(boxes: [first], styleRawValue: 0, frameIsIdle: true))
        tracker.markRendered(boxes: [first], styleRawValue: 0)
        XCTAssertFalse(tracker.shouldRender(boxes: [first], styleRawValue: 0, frameIsIdle: true))
        XCTAssertTrue(tracker.shouldRender(boxes: [first], styleRawValue: 0, frameIsIdle: false))
        XCTAssertTrue(tracker.shouldRender(boxes: [first], styleRawValue: 1, frameIsIdle: true))

        let moved = AdBoundingBox(rect: CGRect(x: 11, y: 20, width: 100, height: 40),
                                  confidence: 1,
                                  label: "manual",
                                  source: .user)
        XCTAssertTrue(tracker.shouldRender(boxes: [moved], styleRawValue: 0, frameIsIdle: true))
    }

    func testEmptyOverlayRendersOnceThenReusesFrames() {
        let tracker = RenderStateTracker()
        XCTAssertTrue(tracker.shouldRender(boxes: [], styleRawValue: 0, frameIsIdle: false))
        tracker.markRendered(boxes: [], styleRawValue: 0)
        XCTAssertFalse(tracker.shouldRender(boxes: [], styleRawValue: 0, frameIsIdle: false))
        tracker.reset()
        XCTAssertTrue(tracker.shouldRender(boxes: [], styleRawValue: 0, frameIsIdle: true))
    }

    private func makePixelBuffer(width: Int, height: Int) -> CVPixelBuffer {
        var pixelBuffer: CVPixelBuffer?
        CVPixelBufferCreate(
            kCFAllocatorDefault,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            [
                kCVPixelBufferCGImageCompatibilityKey as String: true,
                kCVPixelBufferCGBitmapContextCompatibilityKey as String: true,
            ] as CFDictionary,
            &pixelBuffer
        )
        return pixelBuffer!
    }
}
