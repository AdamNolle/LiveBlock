import XCTest
@testable import LiveBlock

final class UserActionSequenceTests: XCTestCase {
    func testNewerCaptureStartSupersedesQueuedStart() {
        var policy = MacUserActionSequencePolicy()
        let first = policy.claimCaptureStart()
        let second = policy.claimCaptureStart()

        XCTAssertFalse(policy.admitsCaptureStart(first))
        XCTAssertTrue(policy.admitsCaptureStart(second))
        XCTAssertTrue(policy.allowsRenderVisibility)
    }

    func testStopRejectsQueuedStartAndDelayedWindowUntilExplicitRestart() {
        var policy = MacUserActionSequencePolicy()
        let beforeStop = policy.claimCaptureStart()
        policy.claimStop()

        XCTAssertFalse(policy.admitsCaptureStart(beforeStop))
        XCTAssertFalse(policy.admitsDelayedAction(beforeStop))
        XCTAssertFalse(policy.allowsRenderVisibility)

        let afterStop = policy.claimCaptureStart()
        XCTAssertTrue(policy.admitsCaptureStart(afterStop))
        XCTAssertTrue(policy.admitsDelayedAction(afterStop))
        XCTAssertTrue(policy.allowsRenderVisibility)
    }

    func testPanicRejectsPrePanicSnapshotAndOnboardingOpen() {
        var policy = MacUserActionSequencePolicy()
        let snapshot = policy.claimCaptureStart()
        policy.claimPanic()

        XCTAssertFalse(policy.admitsCaptureStart(snapshot))
        XCTAssertFalse(policy.admitsDelayedAction(snapshot))
        XCTAssertFalse(policy.allowsRenderVisibility)

        let explicitRestart = policy.claimCaptureStart()
        XCTAssertTrue(policy.admitsCaptureStart(explicitRestart))
        XCTAssertTrue(policy.allowsRenderVisibility)
    }

    func testShutdownIsTerminalEvenForLaterClaimedActions() {
        var policy = MacUserActionSequencePolicy()
        let beforeShutdown = policy.claimCaptureStart()
        policy.claimShutdown()
        let afterShutdown = policy.claimCaptureStart()

        XCTAssertTrue(policy.shutdownRequested)
        XCTAssertFalse(policy.admitsCaptureStart(beforeShutdown))
        XCTAssertFalse(policy.admitsCaptureStart(afterShutdown))
        XCTAssertFalse(policy.admitsDelayedAction(afterShutdown))
        XCTAssertFalse(policy.allowsRenderVisibility)
    }
}
