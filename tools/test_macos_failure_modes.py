import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = (ROOT / "Sources/ScreenCaptureManager.swift").read_text()
CONTROLLER = (ROOT / "Sources/AppController.swift").read_text()
PERFORMANCE_TESTS = (
    ROOT / "Tests/LiveBlockTests/FramePipelinePerformanceTests.swift"
).read_text()
RUNBOOK = (ROOT / "docs/DESKTOP_VALIDATION_RUNBOOKS.md").read_text()


class MacOSFailureModeContractTests(unittest.TestCase):
    def test_missing_display_clears_capture_before_hiding_overlay(self):
        unavailable = re.search(
            r"func failClosedForUnavailableDisplay\(\) async -> Bool \{(.*?)\n    \}",
            CAPTURE,
            re.S,
        )
        self.assertIsNotNone(unavailable)
        source = unavailable.group(1)
        self.assertLess(
            source.index("await stop(preserveIntent: true)"),
            source.index("guard !Task.isCancelled"),
        )
        self.assertIn("!isRunning, !isStarting, stream == nil", source)
        self.assertIn("No usable display is currently available", source)

        topology = re.search(
            r"guard let screen = self\.currentScreen\(\) else \{(.*?)\n            \}",
            CONTROLLER,
            re.S,
        )
        self.assertIsNotNone(topology)
        source = topology.group(1)
        self.assertLess(
            source.index("await self.captureManager.failClosedForUnavailableDisplay()"),
            source.index("self.renderLayer?.orderOut(nil)"),
        )
        self.assertIn("if await self.captureManager.failClosedForUnavailableDisplay()", source)
        self.assertNotIn("screenRestartWasRunning = false", source)

    def test_stream_reset_clears_every_visible_or_cross_generation_seam(self):
        reset = re.search(
            r"private func resetStreamState\(clearError: Bool\) \{(.*?)\n    \}",
            CAPTURE,
            re.S,
        )
        self.assertIsNotNone(reset)
        source = reset.group(1)
        for required in (
            "detectionCache.clear()",
            "blockEventTracker.reset()",
            "renderStateTracker.reset()",
            "fpsCounter.reset()",
            "latestBufferStorage.cancelPending()",
            "isRunning = false",
            "currentPatches = []",
            "currentDetections = []",
            "lastDetectionLabels = []",
            "framesPerSecond = 0",
            "renderMilliseconds = 0",
        ):
            self.assertIn(required, source)

    def test_recovery_and_suspension_remain_bounded_and_intent_guarded(self):
        self.assertIn("static let delays: [TimeInterval] = [0.5, 1, 2, 4]", CAPTURE)
        self.assertIn("systemSuspensionReasons.insert(reason)", CAPTURE)
        self.assertIn("systemSuspensionReasons.remove(reason)", CAPTURE)
        self.assertIn("guard systemSuspensionReasons.isEmpty, captureDesired else { return }", CAPTURE)
        self.assertIn("guard captureDesired, systemSuspensionReasons.isEmpty else { return }", CAPTURE)
        self.assertIn("guard !Task.isCancelled, let self, self.captureDesired", CAPTURE)

    def test_single_flight_static_reuse_and_generation_cancellation_have_tests(self):
        for test_name in (
            "testRenderSingleFlightGateDropsConcurrentClaim",
            "testStaticFrameReuseStillRendersConfigurationChanges",
            "testEmptyOverlayRendersOnceThenReusesFrames",
            "testFPSCounterResetDoesNotMixCaptureGenerations",
            "testPendingSnapshotIsCancelledBeforeLaterCaptureGeneration",
            "testSnapshotIsDeliveredOnlyForMatchingCaptureGeneration",
            "testSnapshotWriterIsPrivateCreateNewAndPreservesExistingBytes",
        ):
            self.assertIn(test_name, PERFORMANCE_TESTS)
        self.assertIn("Task.detached", PERFORMANCE_TESTS)
        self.assertIn("waitForPendingRequest", PERFORMANCE_TESTS)
        self.assertNotIn("where storage.pendingCount() == 0 { await Task.yield() }", PERFORMANCE_TESTS)
        self.assertIn("generation(ifMatching: stream)", CAPTURE)
        self.assertIn("requestSnapshot(\n                timeout: 2.0,\n                generation: expectedGeneration", CAPTURE)
        self.assertIn("currentLifecycleGeneration == expectedGeneration", CONTROLLER)
        self.assertIn("CGImageDestinationCreateWithData", CAPTURE)
        self.assertIn("O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW", CAPTURE)
        self.assertIn("S_IRUSR | S_IWUSR", CAPTURE)
        self.assertIn("try handle.synchronize()", CAPTURE)
        self.assertIn("removeItem(at: url)", CAPTURE)
        self.assertIn("Real Apple\nhardware is mandatory for these claims", RUNBOOK)
        self.assertIn("30 minutes", RUNBOOK)
        self.assertIn("within 500 ms", RUNBOOK)
        self.assertIn("temporarily exposes no usable display", RUNBOOK)
        self.assertIn("no pre-panic request consumes a frame", RUNBOOK)


if __name__ == "__main__":
    unittest.main()
