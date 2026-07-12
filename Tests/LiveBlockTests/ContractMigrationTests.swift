import XCTest
@testable import LiveBlock

final class ContractMigrationTests: XCTestCase {
    func testLegacyLabelDecodesAsSchemaOneAndFutureVersionFailsClosed() throws {
        let legacy = Data(#"{"image":"shot.png","imageWidth":1,"imageHeight":1,"boxes":[],"labeledAt":"1970-01-01T00:00:00Z"}"#.utf8)
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let migrated = try decoder.decode(LabelDocument.self, from: legacy)
        XCTAssertEqual(migrated.schemaVersion, LabelDocument.currentSchemaVersion)

        let future = Data(#"{"schemaVersion":99,"image":"shot.png","imageWidth":1,"imageHeight":1,"boxes":[],"labeledAt":"1970-01-01T00:00:00Z"}"#.utf8)
        XCTAssertThrowsError(try decoder.decode(LabelDocument.self, from: future))
    }

    func testLegacyRegionArrayMigratesToEnvelope() throws {
        let path = temporaryPath("legacy-regions")
        defer { try? FileManager.default.removeItem(at: path) }
        let legacy = #"[{"id":"00000000-0000-0000-0000-000000000000","x":0.1,"y":0.2,"width":0.3,"height":0.4}]"#
        try Data(legacy.utf8).write(to: path)

        let store = RegionStore(storageURL: path)
        XCTAssertEqual(store.current.count, 1)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: path)) as? [String: Any]
        )
        XCTAssertEqual(object["schemaVersion"] as? Int, 1)
        XCTAssertEqual((object["regions"] as? [[String: Any]])?.count, 1)
    }

    func testFutureRegionSchemaIsNotQuarantinedOrRewritten() throws {
        let path = temporaryPath("future-regions")
        defer { try? FileManager.default.removeItem(at: path) }
        let future = #"{"schemaVersion":99,"regions":{"futureShape":true}}"#
        try Data(future.utf8).write(to: path)

        let store = RegionStore(storageURL: path)
        XCTAssertTrue(store.current.isEmpty)
        XCTAssertNotNil(store.persistenceCompatibilityError)
        store.add(NormalizedRegion(x: 0, y: 0, width: 0.2, height: 0.2))
        XCTAssertTrue(store.current.isEmpty)
        XCTAssertEqual(try String(contentsOf: path, encoding: .utf8), future)
    }

    private func temporaryPath(_ stem: String) -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlock-\(stem)-\(UUID().uuidString).json")
    }
}
