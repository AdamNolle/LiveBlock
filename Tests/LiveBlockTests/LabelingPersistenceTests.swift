import XCTest
@testable import LiveBlock

final class LabelingPersistenceTests: XCTestCase {
    func testTrainingDirectoriesAreOwnerOnly() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlockTrainingPathsTests-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let existing = root.appendingPathComponent("screenshots", isDirectory: true)
        try FileManager.default.createDirectory(at: existing, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: root.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: existing.path)

        XCTAssertTrue(TrainingPaths.ensureDirectories(at: root))
        for name in ["", "screenshots", "labels", "exports", "trash"] {
            let url = name.isEmpty ? root : root.appendingPathComponent(name, isDirectory: true)
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o700)
        }
    }

    func testPairedDiscardMovesScreenshotAndLabelWithoutChangingBytes() throws {
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let screenshotBytes = Data("screenshot".utf8)
        let labelBytes = Data("label".utf8)
        try screenshotBytes.write(to: fixture.screenshot)
        try labelBytes.write(to: fixture.label)

        try LabelingFileMover.movePairToTrashNoReplace(
            screenshot: fixture.screenshot,
            label: fixture.label,
            trash: fixture.trash
        )

        XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.screenshot.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.label.path))
        XCTAssertEqual(try Data(contentsOf: fixture.trash.appendingPathComponent("frame.png")), screenshotBytes)
        XCTAssertEqual(try Data(contentsOf: fixture.trash.appendingPathComponent("frame.json")), labelBytes)
    }

    func testPairedDiscardPreservesExistingDestinationAndSources() throws {
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        try Data("source-screenshot".utf8).write(to: fixture.screenshot)
        try Data("source-label".utf8).write(to: fixture.label)
        let existing = fixture.trash.appendingPathComponent("frame.png")
        try Data("existing".utf8).write(to: existing)

        XCTAssertThrowsError(
            try LabelingFileMover.movePairToTrashNoReplace(
                screenshot: fixture.screenshot,
                label: fixture.label,
                trash: fixture.trash
            )
        )
        XCTAssertEqual(try Data(contentsOf: existing), Data("existing".utf8))
        XCTAssertEqual(try Data(contentsOf: fixture.screenshot), Data("source-screenshot".utf8))
        XCTAssertEqual(try Data(contentsOf: fixture.label), Data("source-label".utf8))
    }

    func testPairedDiscardRollsBackLabelWhenScreenshotMoveFails() throws {
        let fixture = try makeFixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let labelBytes = Data("source-label".utf8)
        try labelBytes.write(to: fixture.label)

        XCTAssertThrowsError(
            try LabelingFileMover.movePairToTrashNoReplace(
                screenshot: fixture.screenshot,
                label: fixture.label,
                trash: fixture.trash
            )
        )
        XCTAssertEqual(try Data(contentsOf: fixture.label), labelBytes)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: fixture.trash.appendingPathComponent("frame.json").path
            )
        )
    }

    private func makeFixture() throws -> (
        root: URL,
        screenshot: URL,
        label: URL,
        trash: URL
    ) {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("LiveBlockLabelingMoveTests-\(UUID().uuidString)", isDirectory: true)
        let screenshots = root.appendingPathComponent("screenshots", isDirectory: true)
        let labels = root.appendingPathComponent("labels", isDirectory: true)
        let trash = root.appendingPathComponent("trash", isDirectory: true)
        for directory in [screenshots, labels, trash] {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        return (
            root,
            screenshots.appendingPathComponent("frame.png"),
            labels.appendingPathComponent("frame.json"),
            trash
        )
    }
}
