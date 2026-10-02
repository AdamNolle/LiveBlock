import XCTest
@testable import LiveBlock

final class DesktopContractsTests: XCTestCase {
    func testMacOSCapabilityContractIsLocalInferenceOnly() throws {
        let capability = try XCTUnwrap(DesktopContracts.macOSCapabilities())
        XCTAssertEqual(capability.contractVersion, 1)
        XCTAssertEqual(capability.platform, "macos")
        XCTAssertEqual(capability.captureBackend, "screen_capture_kit")
        XCTAssertTrue(capability.localFrameProcessing)
        XCTAssertFalse(capability.telemetryEnabled)
        XCTAssertFalse(capability.productionTrainingRuntime)
        XCTAssertFalse(capability.releaseReady)
    }

    func testSharedBehaviorLocksVocabularyHotkeysAndPanicSemantics() throws {
        let behavior = try XCTUnwrap(DesktopContracts.behavior())
        XCTAssertEqual(behavior.runtimeClasses, ["Logo", "Ad banner", "Sponsored"])
        XCTAssertEqual(behavior.hotkeys.map(\.macos), [
            "Command+Shift+L", "Command+Shift+B", "Command+Shift+S",
            "Command+Shift+Option+Period",
        ])
        XCTAssertTrue(behavior.panicClearsCaptureIntent)
        XCTAssertTrue(behavior.panicCancelsRecovery)
        XCTAssertTrue(behavior.panicClearsOverlays)
        XCTAssertTrue(behavior.panicClosesEditor)
        XCTAssertTrue(DesktopContracts.validateMacOS())
    }
}
