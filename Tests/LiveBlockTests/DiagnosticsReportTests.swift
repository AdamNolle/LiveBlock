import XCTest
@testable import LiveBlock

final class DiagnosticsReportTests: XCTestCase {
    func testPauseReasonCodesNeverIncludeAssociatedAppOrErrorText() {
        XCTAssertEqual(DiagnosticsReport.pauseReasonCode(.fullscreenApp("Private Player")), "fullscreen_app")
        XCTAssertEqual(DiagnosticsReport.pauseReasonCode(.excludedApp("com.private.secret")), "excluded_app")
        XCTAssertEqual(DiagnosticsReport.pauseReasonCode(.startError("/Users/private/model")), "start_error")
    }

    func testEncodedReportDeclaresAndRespectsPrivacyBoundary() throws {
        let report = DiagnosticsReport(
            schema: 1,
            generatedAt: Date(timeIntervalSince1970: 0),
            runtime: .init(appVersion: "1.0", appBuild: "1", osVersion: "macOS", architecture: "arm64"),
            permissions: .init(screenRecording: true, accessibility: false),
            capture: .init(
                desired: true,
                running: false,
                detectionEnabled: true,
                pauseReason: "session_locked",
                hasStartError: false,
                framesPerSecond: 0,
                renderMilliseconds: 5,
                droppedRenderFrames: 2,
                reusedStaticFrames: 4,
                systemSuspensions: ["session_locked"]
            ),
            displays: [.init(
                id: 7,
                pointX: -1440,
                pointY: 0,
                pointWidth: 1440,
                pointHeight: 900,
                pixelWidth: 2880,
                pixelHeight: 1800,
                isMain: false,
                isSelected: true
            )],
            privacy: .init()
        )
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let json = String(decoding: try encoder.encode(report), as: UTF8.self)

        XCTAssertTrue(json.contains("\"containsFrameData\":false"))
        XCTAssertTrue(json.contains("\"containsProcessList\":false"))
        XCTAssertFalse(json.localizedCaseInsensitiveContains("frontmost"))
        XCTAssertTrue(json.contains("\"containsWindowTitles\":false"))
        XCTAssertFalse(json.contains("/Users/"))
        XCTAssertFalse(json.contains("Private Player"))
    }
}
