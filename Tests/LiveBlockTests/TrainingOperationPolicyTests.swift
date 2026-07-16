import XCTest
@testable import LiveBlock

final class TrainingOperationPolicyTests: XCTestCase {
    func testCancelInvalidatesStaleCompletionAndAllowsNewOperation() {
        var policy = TrainingOperationPolicy()
        let first = try! XCTUnwrap(policy.begin())
        XCTAssertTrue(policy.owns(first))

        policy.cancel()
        XCTAssertFalse(policy.owns(first))

        let second = try! XCTUnwrap(policy.begin())
        XCTAssertNotEqual(first, second)
        XCTAssertFalse(policy.owns(first))
        XCTAssertTrue(policy.owns(second))
    }

    func testFinishingStaleOperationCannotReleaseNewOwner() {
        var policy = TrainingOperationPolicy()
        let first = try! XCTUnwrap(policy.begin())
        policy.cancel()
        let second = try! XCTUnwrap(policy.begin())

        policy.finish(first)
        XCTAssertTrue(policy.owns(second))
        policy.finish(second)
        XCTAssertFalse(policy.owns(second))
        XCTAssertNil(policy.activeOperation)
    }

    func testShutdownIsTerminalAndInvalidatesCurrentOperation() {
        var policy = TrainingOperationPolicy()
        let operation = try! XCTUnwrap(policy.begin())

        policy.shutdown()

        XCTAssertTrue(policy.shutdownRequested)
        XCTAssertFalse(policy.owns(operation))
        XCTAssertNil(policy.begin())
    }
}
