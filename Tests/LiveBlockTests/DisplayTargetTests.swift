import XCTest
@testable import LiveBlock

final class DisplayTargetTests: XCTestCase {
    private func descriptor(_ id: UInt32, main: Bool = false) -> DisplayDescriptor {
        DisplayDescriptor(id: id,
                          name: "Display \(id)",
                          pointFrame: CGRect(x: id == 30 ? -1920 : 0,
                                             y: 0, width: 1920, height: 1080),
                          pixelWidth: id == 20 ? 3840 : 1920,
                          pixelHeight: id == 20 ? 2160 : 1080,
                          isMain: main)
    }

    func testResolverRetainsAvailableExplicitTarget() {
        let displays = [descriptor(10, main: true), descriptor(20)]
        XCTAssertEqual(DisplayTargetResolver.resolvedID(preferred: 20,
                                                        descriptors: displays), 20)
    }

    func testResolverFallsBackToMainWhenTargetDisappears() {
        let displays = [descriptor(30), descriptor(10, main: true)]
        XCTAssertEqual(DisplayTargetResolver.resolvedID(preferred: 99,
                                                        descriptors: displays), 10)
    }

    func testResolverUsesStableLowestIDWithoutMain() {
        let displays = [descriptor(30), descriptor(20)]
        XCTAssertEqual(DisplayTargetResolver.resolvedID(preferred: nil,
                                                        descriptors: displays), 20)
    }

    func testResolverFailsClosedWithoutDisplays() {
        XCTAssertNil(DisplayTargetResolver.resolvedID(preferred: 10,
                                                      descriptors: []))
    }

    func testDescriptorPreservesNegativeOriginAndAuthoritativePixels() {
        let display = descriptor(30)
        XCTAssertEqual(display.pointFrame.minX, -1920)
        XCTAssertEqual(display.pixelSize, CGSize(width: 1920, height: 1080))
        XCTAssertEqual(display.scaleDescription, "1.00×")
    }

    func testRetryPolicyIsBoundedAndExponential() {
        XCTAssertEqual(CaptureRetryPolicy.delay(forAttempt: 0), 0.5)
        XCTAssertEqual(CaptureRetryPolicy.delay(forAttempt: 1), 1)
        XCTAssertEqual(CaptureRetryPolicy.delay(forAttempt: 2), 2)
        XCTAssertEqual(CaptureRetryPolicy.delay(forAttempt: 3), 4)
        XCTAssertNil(CaptureRetryPolicy.delay(forAttempt: 4))
        XCTAssertNil(CaptureRetryPolicy.delay(forAttempt: -1))
    }
}
