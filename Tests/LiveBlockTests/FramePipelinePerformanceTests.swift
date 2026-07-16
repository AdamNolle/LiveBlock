import CoreImage
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

    func testFPSCounterResetDoesNotMixCaptureGenerations() {
        let counter = FPSCounter()
        XCTAssertEqual(counter.tick(), 1)
        XCTAssertEqual(counter.tick(), 2)
        counter.reset()
        XCTAssertEqual(counter.tick(), 1)
    }

    func testPendingSnapshotIsCancelledBeforeLaterCaptureGeneration() async {
        let storage = LatestBufferStorage()
        let request = Task { await storage.requestSnapshot(timeout: 10, generation: 7) }
        for _ in 0..<100 where storage.pendingCount() == 0 { await Task.yield() }
        XCTAssertEqual(storage.pendingCount(), 1)

        storage.ingest(makePixelBuffer(width: 16, height: 16),
                       context: CIContext(options: [.useSoftwareRenderer: true]),
                       generation: 8)
        XCTAssertEqual(storage.pendingCount(), 1)
        XCTAssertEqual(storage.cancelPending(), 1)
        let cancelledImage = await request.value
        XCTAssertNil(cancelledImage)
    }

    func testSnapshotIsDeliveredOnlyForMatchingCaptureGeneration() async {
        let storage = LatestBufferStorage()
        let request = Task { await storage.requestSnapshot(timeout: 10, generation: 9) }
        for _ in 0..<100 where storage.pendingCount() == 0 { await Task.yield() }
        XCTAssertEqual(storage.pendingCount(), 1)

        storage.ingest(makePixelBuffer(width: 16, height: 16),
                       context: CIContext(options: [.useSoftwareRenderer: true]),
                       generation: 9)
        let deliveredImage = await request.value
        XCTAssertNotNil(deliveredImage)
        XCTAssertEqual(storage.pendingCount(), 0)
    }

    func testSnapshotWriterIsPrivateCreateNewAndPreservesExistingBytes() throws {
        let buffer = makePixelBuffer(width: 16, height: 16)
        let image = try XCTUnwrap(
            CIContext(options: [.useSoftwareRenderer: true]).createCGImage(
                CIImage(cvPixelBuffer: buffer),
                from: CGRect(x: 0, y: 0, width: 16, height: 16)
            )
        )
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlock-snapshot-\(UUID().uuidString).png")
        defer { try? FileManager.default.removeItem(at: url) }

        XCTAssertTrue(SnapshotPNGWriter.write(image, to: url))
        let first = try Data(contentsOf: url)
        XCTAssertFalse(first.isEmpty)
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        let mode = try XCTUnwrap(attributes[.posixPermissions] as? NSNumber).intValue
        XCTAssertEqual(mode & 0o777, 0o600)

        XCTAssertFalse(SnapshotPNGWriter.write(image, to: url))
        XCTAssertEqual(try Data(contentsOf: url), first)
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
