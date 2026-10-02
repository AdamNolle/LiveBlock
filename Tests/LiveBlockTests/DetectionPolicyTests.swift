import XCTest
import CoreVideo
@testable import LiveBlock

final class DetectionPolicyTests: XCTestCase {
    func testSingleLabelCannotTurnWeakObjectIntoCertainDetection() throws {
        let weak = try XCTUnwrap(VisionProcessor.combinedConfidence(object: 0.29, classification: 1))
        XCTAssertLessThan(weak, 0.5)
        let strong = try XCTUnwrap(VisionProcessor.combinedConfidence(object: 0.87, classification: 1))
        XCTAssertGreaterThan(strong, 0.5)
        // Multiple labels must also include the object's uncertainty.
        let ambiguous = try XCTUnwrap(VisionProcessor.combinedConfidence(object: 0.6, classification: 0.7))
        XCTAssertLessThan(ambiguous, 0.5)
    }

    func testInvalidConfidenceCannotAuthorizeCover() {
        for invalid: Float in [.nan, .infinity, -0.1, 1.1] {
            XCTAssertNil(VisionProcessor.combinedConfidence(object: invalid, classification: 1))
            XCTAssertNil(VisionProcessor.combinedConfidence(object: 1, classification: invalid))
        }
    }

    func testLabelClassesRoundTripAndLegacyDefaultsToAd() throws {
        let box = LabelBox(x: 0.1, y: 0.2, width: 0.3, height: 0.4, labelClass: .numberKeep)
        let encoded = try JSONEncoder().encode(box)
        XCTAssertEqual(try JSONDecoder().decode(LabelBox.self, from: encoded).labelClass, .numberKeep)
        var legacy = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        legacy.removeValue(forKey: "class")
        let data = try JSONSerialization.data(withJSONObject: legacy)
        XCTAssertEqual(try JSONDecoder().decode(LabelBox.self, from: data).labelClass, .ad)
        legacy["class"] = "unknown_remove"
        XCTAssertThrowsError(try JSONDecoder().decode(LabelBox.self, from: JSONSerialization.data(withJSONObject: legacy)))
    }

    func testGenericObjectsAndUnknownClassesNeverErase() {
        for label in ["person", "car", "dog", "logo", "team_keep", "number_keep", "class_0"] {
            XCTAssertFalse(SponsorClassAllowlist.allowsAutoBlock(label: label))
        }
        XCTAssertTrue(SponsorClassAllowlist.allowsAutoBlock(label: "ad"))
        XCTAssertTrue(SponsorClassAllowlist.allowsAutoBlock(label: "SPONSOR_REMOVE"))
    }

    func testOverlappingIdentitySuppressesAutomaticRemoval() {
        let ad = NormalizedDetection(normRect: CGRect(x: 0.1, y: 0.1, width: 0.4, height: 0.2), confidence: 0.99, label: "ad")
        let number = NormalizedDetection(normRect: CGRect(x: 0.2, y: 0.15, width: 0.1, height: 0.1), confidence: 0.8, label: "number_keep")
        XCTAssertTrue(SponsorClassAllowlist.removableDetections([ad, number]).isEmpty)
        let separateAd = NormalizedDetection(normRect: CGRect(x: 0.6, y: 0.6, width: 0.1, height: 0.1), confidence: 0.99, label: "sponsor_remove")
        XCTAssertEqual(SponsorClassAllowlist.removableDetections([ad, number, separateAd]).map(\.label), ["sponsor_remove"])
    }

    func testScreenshotWaiterIsReleasedOnStop() async {
        let storage = LatestBufferStorage()
        let waiter = Task { await storage.requestSnapshot() }
        // Yield until the task is scheduled, then cancel all waiters. Timeout
        // also guarantees this never hangs if task scheduling races the stop.
        await Task.yield()
        storage.cancelPending()
        let image = await waiter.value
        XCTAssertNil(image)
    }
}
