import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = (ROOT / "Sources/ScreenCaptureManager.swift").read_text()
CONTROLLER = (ROOT / "Sources/AppController.swift").read_text()
TRAINING = (ROOT / "Sources/TrainingController.swift").read_text()
LIVEBLOCK_APP = (ROOT / "Sources/LiveBlockApp.swift").read_text()
PERFORMANCE_TESTS = (
    ROOT / "Tests/LiveBlockTests/FramePipelinePerformanceTests.swift"
).read_text()
ACTION_TESTS = (ROOT / "Tests/LiveBlockTests/UserActionSequenceTests.swift").read_text()
TRAINING_TESTS = (
    ROOT / "Tests/LiveBlockTests/TrainingOperationPolicyTests.swift"
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

        topology_handler = re.search(
            r"func handleScreenConfigurationChange\(\) \{(.*?)\n    \}",
            CONTROLLER,
            re.S,
        )
        self.assertIsNotNone(topology_handler)
        topology = re.search(
            r"guard let screen = self\.currentScreen\(\) else \{(.*?)\n            \}",
            topology_handler.group(1),
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

    def test_panic_and_quit_install_synchronous_action_barriers(self):
        panic = re.search(r"func panicDisable\(\) \{(.*?)\n    \}", CONTROLLER, re.S)
        self.assertIsNotNone(panic)
        panic_source = panic.group(1)
        self.assertLess(panic_source.index("userActionPolicy.claimPanic()"), panic_source.index("Task {"))
        self.assertLess(panic_source.index("autoCaptureEnabled = false"), panic_source.index("Task {"))
        self.assertLess(
            panic_source.index("hidePrivacyWindowsForTerminalAction()"),
            panic_source.index("await self?.captureManager.stop()"),
        )

        self.assertIn("func quit() {\n        beginQuit()", CONTROLLER)
        quit_action = re.search(r"private func beginQuit\(\) \{(.*?)\n    \}", CONTROLLER, re.S)
        self.assertIsNotNone(quit_action)
        quit_source = quit_action.group(1)
        self.assertIn("guard quitTask == nil else { return }", quit_source)
        self.assertLess(quit_source.index("userActionPolicy.claimShutdown()"), quit_source.index("Task {"))
        self.assertLess(
            quit_source.index("hidePrivacyWindowsForTerminalAction()"),
            quit_source.index("await self.captureManager.stop()"),
        )
        self.assertLess(
            quit_source.index("hidePrivacyWindowsForTerminalAction()"),
            quit_source.index("trainingController.prepareForShutdown()"),
        )
        self.assertLess(
            quit_source.index("trainingController.prepareForShutdown()"),
            quit_source.index("Task {"),
        )
        self.assertLess(
            quit_source.index("await trainingShutdownTask?.value"),
            quit_source.index("NSApp.terminate(nil)"),
        )
        self.assertIn("allowsRenderVisibility", LIVEBLOCK_APP)
        self.assertIn("func applicationShouldTerminate(_ sender: NSApplication)", LIVEBLOCK_APP)
        self.assertIn("return .terminateLater", LIVEBLOCK_APP)
        self.assertIn("sender.reply(toApplicationShouldTerminate: true)", LIVEBLOCK_APP)

    def test_model_update_completion_cannot_publish_after_shutdown(self):
        update = re.search(
            r"func chooseAndInstallSignedModelUpdate\(\) \{(.*?)\n    \}",
            CONTROLLER,
            re.S,
        )
        self.assertIsNotNone(update)
        source = update.group(1)
        self.assertIn("panel.runModal() == .OK", source)
        self.assertGreaterEqual(source.count("shutdownRequested"), 4)
        self.assertEqual(
            source.count(
                "guard let self, !self.userActionPolicy.shutdownRequested else { return }"
            ),
            2,
        )

    def test_source_training_cancellation_rejects_stale_process_callbacks(self):
        for required in (
            "struct TrainingOperationPolicy",
            "operationPolicy.shutdown()",
            "guard operationIsCurrent(operation) else { return }",
            "guard let self, self.process === proc else { return }",
            "[process, pid]",
            "guard process.isRunning else { return }",
            "operationKind != .verifiedInstall",
            "quit waits for its atomic transaction",
        ):
            self.assertIn(required, TRAINING)
        self.assertNotIn("if let p = self.process, p.isRunning", TRAINING)
        for test_name in (
            "testCancelInvalidatesStaleCompletionAndAllowsNewOperation",
            "testFinishingStaleOperationCannotReleaseNewOwner",
            "testShutdownIsTerminalAndInvalidatesCurrentOperation",
        ):
            self.assertIn(test_name, TRAINING_TESTS)

    def test_auto_capture_and_onboarding_delays_are_panic_guarded(self):
        self.assertIn("autoCaptureGeneration &+= 1", CONTROLLER)
        self.assertIn("self.autoCaptureGeneration == generation", CONTROLLER)
        self.assertIn("self.autoCaptureEnabled", CONTROLLER)
        self.assertIn("startFirstBlockFromOnboarding()", LIVEBLOCK_APP)
        self.assertNotIn("DispatchQueue.main.asyncAfter(deadline: .now() + 0.4)", LIVEBLOCK_APP)
        for test_name in (
            "testNewerCaptureStartSupersedesQueuedStart",
            "testStopRejectsQueuedStartAndDelayedWindowUntilExplicitRestart",
            "testPanicRejectsPrePanicSnapshotAndOnboardingOpen",
            "testShutdownIsTerminalEvenForLaterClaimedActions",
        ):
            self.assertIn(test_name, ACTION_TESTS)
        self.assertIn("queued auto-capture", RUNBOOK)
        self.assertIn("delayed onboarding", RUNBOOK)

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
