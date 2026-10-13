import XCTest
@testable import LiveBlock

final class TrainingSafetyTests: XCTestCase {
    func testLegacyLabelDefaultsToAdAndNewClassRoundTrips() throws {
        let id = UUID()
        let legacy = """
        {"id":"\(id.uuidString)","x":0.1,"y":0.2,"width":0.3,"height":0.4}
        """.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(LabelBox.self, from: legacy)
        XCTAssertEqual(decoded.labelClass, .ad)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(decoded)) as? [String: Any]
        )
        XCTAssertNil(object["class"])

        let keep = LabelBox(x: 0.1, y: 0.2, width: 0.3, height: 0.4,
                            labelClass: .teamKeep)
        let encoded = try JSONEncoder().encode(keep)
        XCTAssertEqual(try JSONDecoder().decode(LabelBox.self, from: encoded).labelClass,
                       .teamKeep)
    }

    func testKeepBoxIsCutOutOfSponsorBox() {
        let sponsor = AdBoundingBox(rect: CGRect(x: 0, y: 0, width: 100, height: 100),
                                    confidence: 0.9, label: "sponsor_remove", source: .detection)
        let keep = AdBoundingBox(rect: CGRect(x: 40, y: 40, width: 20, height: 20),
                                 confidence: 0.8, label: "team_keep", source: .detection)
        let pieces = ScreenCaptureManager.excludingKeepBoxes([sponsor], keeps: [keep])
        XCTAssertEqual(pieces.count, 4)
        XCTAssertEqual(pieces.reduce(0) { $0 + $1.rect.width * $1.rect.height }, 9_600)
        XCTAssertTrue(pieces.allSatisfy {
            let overlap = $0.rect.intersection(keep.rect)
            return overlap.isNull || overlap.width == 0 || overlap.height == 0
        })
    }
}
