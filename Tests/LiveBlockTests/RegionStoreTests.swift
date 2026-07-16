import XCTest
@testable import LiveBlock

final class RegionStoreTests: XCTestCase {

    func testNormalizedRegionClampsValues() {
        let r = NormalizedRegion(x: -0.5, y: 1.2, width: 5, height: 5)
        XCTAssertEqual(r.x, 0)
        XCTAssertEqual(r.y, 1)
        XCTAssertLessThanOrEqual(r.x + r.width, 1.0001)
        XCTAssertLessThanOrEqual(r.y + r.height, 1.0001)
    }

    func testRectInTargetSize() {
        let r = NormalizedRegion(x: 0.25, y: 0.5, width: 0.5, height: 0.25)
        let rect = r.rect(in: CGSize(width: 1000, height: 800))
        XCTAssertEqual(rect.origin.x, 250, accuracy: 0.001)
        XCTAssertEqual(rect.origin.y, 400, accuracy: 0.001)
        XCTAssertEqual(rect.width, 500, accuracy: 0.001)
        XCTAssertEqual(rect.height, 200, accuracy: 0.001)
    }

    func testCVRectFlipsToBottomLeft() {
        let r = NormalizedRegion(x: 0.0, y: 0.0, width: 0.5, height: 0.5)
        let cv = r.cvRect(inPixelBufferSize: CGSize(width: 100, height: 100))
        // Top-left rect at (0,0,50,50) → bottom-left rect at (0, 50, 50, 50)
        XCTAssertEqual(cv.origin.x, 0, accuracy: 0.001)
        XCTAssertEqual(cv.origin.y, 50, accuracy: 0.001)
        XCTAssertEqual(cv.width, 50, accuracy: 0.001)
        XCTAssertEqual(cv.height, 50, accuracy: 0.001)
    }

    func testRoundTripPersistence() throws {
        let tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlockRegionStoreTests-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: tmp) }

        let store = RegionStore(storageURL: tmp)
        XCTAssertEqual(store.current.count, 0)

        store.add(NormalizedRegion(x: 0.1, y: 0.1, width: 0.2, height: 0.3))
        store.add(NormalizedRegion(x: 0.4, y: 0.4, width: 0.1, height: 0.1))
        XCTAssertEqual(store.current.count, 2)

        // Re-open from same URL and confirm contents survived the round-trip.
        let reopened = RegionStore(storageURL: tmp)
        XCTAssertEqual(reopened.current.count, 2)
        let firstWidth = try XCTUnwrap(reopened.current.first?.width)
        XCTAssertEqual(firstWidth, 0.2, accuracy: 0.001)
    }

    func testDisabledRegionsAreExcludedFromCaptureSnapshot() {
        let tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlockDisabledRegionTests-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: tmp) }

        let enabled = NormalizedRegion(x: 0.1, y: 0.1, width: 0.2, height: 0.2)
        let disabled = NormalizedRegion(x: 0.5, y: 0.5, width: 0.2, height: 0.2)
        let store = RegionStore(storageURL: tmp)
        store.add(enabled)
        store.add(disabled)

        XCTAssertEqual(store.current(excluding: [disabled.id]).map(\.id), [enabled.id])
    }

    func testBlockEventsCountAppearancesInsteadOfFrames() {
        let tracker = BlockEventTracker()
        let id = UUID()
        let user = AdBoundingBox(rect: CGRect(x: 0, y: 0, width: 20, height: 20),
                                 confidence: 1, label: "User", source: .user, regionID: id)
        let detected = AdBoundingBox(rect: CGRect(x: 40, y: 40, width: 20, height: 20),
                                     confidence: 0.8, label: "Logo", source: .detection)

        XCTAssertEqual(tracker.update(userBoxes: [user], detectedBoxes: [detected]).total, 2)
        XCTAssertEqual(tracker.update(userBoxes: [user], detectedBoxes: [detected]).total, 0)
        _ = tracker.update(userBoxes: [], detectedBoxes: [])
        XCTAssertEqual(tracker.update(userBoxes: [user], detectedBoxes: [detected]).total, 2)
    }

    func testActiveStreamIdentityRejectsRetiredObjects() {
        let identity = AtomicObjectIdentity()
        let old = NSObject()
        let current = NSObject()
        identity.set(old, generation: 1)
        XCTAssertTrue(identity.matches(old))
        identity.set(current, generation: 2)
        identity.clear(ifGeneration: 1)
        XCTAssertTrue(identity.matches(current))
        XCTAssertFalse(identity.matches(old))
        identity.clear(ifMatching: current)
        XCTAssertFalse(identity.matches(current))
    }

    func testDetectionCacheRejectsResultsAfterClear() {
        let cache = DetectionCache()
        let generation = cache.generation()
        cache.clear()
        cache.store([
            AdBoundingBox(rect: CGRect(x: 0, y: 0, width: 10, height: 10),
                          confidence: 1, label: "stale", source: .detection)
        ], ifGeneration: generation)
        XCTAssertTrue(cache.load().isEmpty)
    }

    func testSnapshotRequestTimesOutWithoutCapturedFrame() async {
        let storage = LatestBufferStorage()
        let started = Date()
        let result = await storage.requestSnapshot(timeout: 0.1, generation: 1)
        XCTAssertNil(result)
        XCTAssertLessThan(Date().timeIntervalSince(started), 1.0)
    }

    func testInpaintingEngineProducesNoPatchesForEmptyInput() {
        let engine = InpaintingEngine()
        let buffer = makePixelBuffer(width: 64, height: 64)
        let patches = engine.inpaintPatches(frame: buffer, regions: [])
        XCTAssertEqual(patches.count, 0)
    }

    func testInpaintingEngineProducesPatchPerRegionForEveryFillStyle() throws {
        let engine = InpaintingEngine()
        let buffer = makePixelBuffer(width: 128, height: 128)
        let region = AdBoundingBox(rect: CGRect(x: 16, y: 16, width: 64, height: 32),
                                   confidence: 1.0,
                                   label: "test",
                                   source: .user)
        for style in [InpaintFillStyle.smart, .blur, .averageColor, .solidBlack] {
            let patches = engine.inpaintPatches(frame: buffer, regions: [region], style: style)
            XCTAssertEqual(patches.count, 1, "style \(style) must render")
            let patch = try XCTUnwrap(patches.first)
            XCTAssertGreaterThanOrEqual(patch.normalizedRect.minX, 0)
            XCTAssertLessThanOrEqual(patch.normalizedRect.maxX, 1)
            XCTAssertGreaterThan(patch.image.width, 0)
            XCTAssertGreaterThan(patch.image.height, 0)
        }
    }

    // MARK: - Helpers

    private func makePixelBuffer(width: Int, height: Int) -> CVPixelBuffer {
        var pb: CVPixelBuffer?
        let attrs: [String: Any] = [
            kCVPixelBufferCGImageCompatibilityKey as String: true,
            kCVPixelBufferCGBitmapContextCompatibilityKey as String: true
        ]
        CVPixelBufferCreate(kCFAllocatorDefault,
                            width, height,
                            kCVPixelFormatType_32BGRA,
                            attrs as CFDictionary,
                            &pb)
        return pb!
    }
}
