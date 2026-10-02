import XCTest
import AppKit
@testable import LiveBlock

final class RegionStoreTests: XCTestCase {
    @MainActor
    func testCoverOnlyHonorsRegionSwitchAndExclusionsWithoutStartingCapture() async throws {
        guard let screen = NSScreen.screens.first else { throw XCTSkip("A display is needed to verify overlay geometry") }
        let suite = "LiveBlockCoverTests-\(UUID().uuidString)"
        let preferences = try XCTUnwrap(UserDefaults(suiteName: suite))
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(suite).json")
        defer {
            preferences.removePersistentDomain(forName: suite)
            try? FileManager.default.removeItem(at: file)
        }
        let store = RegionStore(storageURL: file, preferences: preferences)
        let region = NormalizedRegion(x: 0.1, y: 0.2, width: 0.1, height: 0.1)
        store.add(region)
        let manager = ScreenCaptureManager(regionStore: store)
        manager.detectionEnabled = true
        manager.startCover(on: screen)
        XCTAssertTrue(manager.isRunning)
        XCTAssertTrue(manager.isCoverOnly)
        XCTAssertFalse(manager.isStarting)
        XCTAssertFalse(manager.detectionEnabled)
        XCTAssertEqual(manager.framesPerSecond, 0)
        XCTAssertEqual(manager.currentPatches.count, 1)
        XCTAssertEqual(manager.currentPatches.first?.normalizedRect.minY ?? -1, 0.2, accuracy: 0.002)
        manager.frontmostIsExcluded = true
        XCTAssertTrue(manager.currentPatches.isEmpty)
        manager.frontmostIsExcluded = false
        XCTAssertEqual(manager.currentPatches.count, 1)
        store.setEnabled(id: region.id, on: false)
        manager.refreshCover()
        XCTAssertTrue(manager.currentPatches.isEmpty)
        XCTAssertEqual(store.current.count, 1)
        await manager.stop()
        XCTAssertFalse(manager.isRunning)
        XCTAssertFalse(manager.isCoverOnly)
    }

    func testDisabledRegionRemainsSavedButLeavesPaintInput() throws {
        let suite = "LiveBlockRegionEnabledTests-\(UUID().uuidString)"
        let preferences = try XCTUnwrap(UserDefaults(suiteName: suite))
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("\(suite).json")
        defer {
            preferences.removePersistentDomain(forName: suite)
            try? FileManager.default.removeItem(at: file)
        }
        let store = RegionStore(storageURL: file, preferences: preferences)
        let first = NormalizedRegion(x: 0.1, y: 0.1, width: 0.2, height: 0.2)
        let second = NormalizedRegion(x: 0.5, y: 0.5, width: 0.2, height: 0.2)
        store.add(first); store.add(second)
        preferences.set([first.id.uuidString.lowercased(), "invalid legacy ID"], forKey: RegionStore.disabledIdsKey)
        XCTAssertEqual(store.enabled.map(\.id), [second.id])
        XCTAssertEqual(store.current.count, 2)
        let reopened = RegionStore(storageURL: file, preferences: preferences)
        XCTAssertEqual(reopened.enabled.map(\.id), [second.id])
        XCTAssertFalse(reopened.isEnabled(id: first.id))
        reopened.setEnabled(id: first.id, on: true)
        XCTAssertEqual(Set(store.enabled.map(\.id)), Set([first.id, second.id]))
    }


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

    func testInpaintingEngineProducesNoPatchesForEmptyInput() {
        let engine = InpaintingEngine()
        let buffer = makePixelBuffer(width: 64, height: 64)
        let patches = engine.inpaintPatches(frame: buffer, regions: [])
        XCTAssertEqual(patches.count, 0)
    }

    func testInpaintingEngineProducesPatchPerRegion() {
        let engine = InpaintingEngine()
        let buffer = makePixelBuffer(width: 128, height: 128)
        let region = AdBoundingBox(rect: CGRect(x: 16, y: 16, width: 64, height: 32),
                                   confidence: 1.0,
                                   label: "test",
                                   source: .user)
        let patches = engine.inpaintPatches(frame: buffer, regions: [region])
        XCTAssertEqual(patches.count, 1)
        let patch = try! XCTUnwrap(patches.first)
        // Normalized rect is in [0..1]
        XCTAssertGreaterThanOrEqual(patch.normalizedRect.minX, 0)
        XCTAssertLessThanOrEqual(patch.normalizedRect.maxX, 1)
        XCTAssertGreaterThan(patch.image.width, 0)
        XCTAssertGreaterThan(patch.image.height, 0)
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
