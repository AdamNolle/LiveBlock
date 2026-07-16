import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = (ROOT / "platform/windows/src-tauri/src/main.rs").read_text()
LINUX = (ROOT / "platform/linux/src-tauri/src/main.rs").read_text()
IPC = (ROOT / "platform/_shared-frontend/src/ipc.ts").read_text()
REGION_EDITOR = (ROOT / "platform/_shared-frontend/src/region-editor.html").read_text()
LABELING = (ROOT / "platform/_shared-frontend/src/labeling.html").read_text()
TRAINING_UI = (ROOT / "platform/_shared-frontend/src/training.html").read_text()
MAC_CONTROLLER = (ROOT / "Sources/AppController.swift").read_text()
WINDOWS_UPDATES = (ROOT / "platform/windows/src-tauri/src/model_updates.rs").read_text()
WINDOWS_LIFECYCLE = (ROOT / "platform/windows/src-tauri/src/lifecycle.rs").read_text()
WINDOWS_CAPTURE_POLICY = (ROOT / "platform/windows/src-tauri/src/capture_policy.rs").read_text()
WINDOWS_CONTROL_PANEL = (ROOT / "platform/_shared-frontend/src/control-panel.html").read_text()
WINDOWS_DETECTION = (ROOT / "platform/windows/src-tauri/src/detection.rs").read_text()
WINDOWS_DIRECTML_DOC = (ROOT / "docs/WINDOWS_DIRECTML.md").read_text()
LINUX_UPDATES = (ROOT / "platform/linux/src-tauri/src/model_updates.rs").read_text()
LINUX_DETECTION = (ROOT / "platform/linux/src-tauri/src/detection.rs").read_text()
LINUX_INPAINTING = (ROOT / "platform/linux/src-tauri/src/inpainting.rs").read_text()
LINUX_WAYLAND_CAPTURE = (ROOT / "platform/linux/src-tauri/src/capture/wayland.rs").read_text()
LINUX_X11_CAPTURE = (ROOT / "platform/linux/src-tauri/src/capture/x11.rs").read_text()
LINUX_CAPTURE = (ROOT / "platform/linux/src-tauri/src/capture/mod.rs").read_text()
LINUX_STATE = (ROOT / "platform/linux/src-tauri/src/state.rs").read_text()
LINUX_WGSL = (ROOT / "platform/linux/src-tauri/src/inpainting.wgsl").read_text()
LINUX_BUILD = (ROOT / "platform/linux/src-tauri/build.rs").read_text()
CI = (ROOT / ".github/workflows/ci.yml").read_text()

EXPECTED_COMMANDS = {
    "get_capabilities",
    "get_behavior_contract",
    "begin_user_action",
    "start_capture",
    "stop_capture",
    "get_capture_telemetry",
    "set_detection_enabled",
    "list_monitors",
    "list_regions",
    "add_region",
    "replace_region",
    "delete_region",
    "clear_regions",
    "capture_screenshot_for_labeling",
    "list_screenshots",
    "load_screenshot",
    "save_label",
    "load_label",
    "discard_screenshot",
    "start_training",
    "cancel_training",
    "install_model_update",
    "show_window",
    "hide_window",
    "quit",
}


def handler_commands(source: str) -> set[str]:
    match = re.search(r"tauri::generate_handler!\[(.*?)\]\)", source, re.S)
    if not match:
        raise AssertionError("generate_handler list not found")
    return {
        name.strip().rsplit("::", 1)[-1]
        for name in match.group(1).split(",")
        if name.strip()
    }


class DesktopAdapterContractTests(unittest.TestCase):
    def test_windows_and_linux_register_same_commands(self):
        self.assertEqual(handler_commands(WINDOWS), EXPECTED_COMMANDS)
        self.assertEqual(handler_commands(LINUX), EXPECTED_COMMANDS)

    def test_frontend_invokes_registered_command_vocabulary(self):
        invoked = set(re.findall(r'invoke(?:<[^>]+>)?\("([a-z_]+)"', IPC))
        self.assertEqual(invoked, EXPECTED_COMMANDS)

    def test_monitor_screenshot_and_event_shapes_are_aligned(self):
        for source in (WINDOWS, LINUX):
            self.assertRegex(source, r"fn start_capture\([\s\S]*?monitor_id: String")
            self.assertIn('emit("capture-state-changed", false)', source)
            self.assertIn('serde(rename_all = "camelCase")', source)
            self.assertIn("struct ScreenshotData", source)
            self.assertIn("fn get_capture_telemetry", source)
        self.assertIn('emit("capture-state-changed", true)', WINDOWS)
        self.assertIn('emit("capture-state-changed", true)', LINUX)
        self.assertIn("source = open_capture()", LINUX)
        self.assertIn("portal-selection", LINUX)
        self.assertIn("x11-root", LINUX)
        self.assertIn("startCapture: async (monitorId: string)", IPC)
        self.assertIn('invoke<number>("begin_user_action")', IPC)
        self.assertIn("getCaptureTelemetry", IPC)
        self.assertIn("loadScreenshot: (path: string) => invoke<ScreenshotData>", IPC)
        self.assertIn('listen<boolean>("capture-state-changed"', IPC)
        self.assertIn('listen<boolean>("protected-content-changed"', IPC)
        self.assertIn('listen<string>("capture-runtime-error"', IPC)

    def test_native_hotkey_and_panic_dispatch_is_not_frontend_only(self):
        for event in (
            "hotkey-toggle-capture",
            "hotkey-toggle-editor",
            "hotkey-capture-screenshot",
            "hotkey-panic-disable",
        ):
            self.assertIn(event, WINDOWS)
        self.assertIn("panic_disable_action", WINDOWS)
        self.assertIn('for label in ["editor", "render", "labeling", "training"]', WINDOWS)
        self.assertIn("dispatch_hotkey", LINUX)
        self.assertIn("PanicDisable", LINUX)

    def test_windows_directml_registration_and_cpu_fallback_are_truthful(self):
        self.assertIn("with_parallel_execution(false)", WINDOWS_DETECTION)
        self.assertIn("with_memory_pattern(false)", WINDOWS_DETECTION)
        self.assertGreaterEqual(WINDOWS_DETECTION.count("error_on_failure()"), 2)
        self.assertIn("cpu_after_directml_load_failure", WINDOWS_DETECTION)
        self.assertIn("directml_registered_cpu_uploaded_tensor", WINDOWS_DETECTION)
        self.assertIn("detector.backend_status()", WINDOWS)
        self.assertIn("registration/load status is not proof", WINDOWS)
        self.assertIn("ID3D12Resource", WINDOWS_DIRECTML_DOC)
        self.assertIn("CreateGPUAllocationFromD3DResource", WINDOWS_DIRECTML_DOC)
        self.assertIn("D3D11On12", WINDOWS_DIRECTML_DOC)
        self.assertIn("leave the texture-transport checklist item open", WINDOWS_DIRECTML_DOC)

    def test_windows_protected_unavailable_state_hides_stale_output(self):
        self.assertIn("checked_mul(height as usize)", WINDOWS_CAPTURE_POLICY)
        self.assertIn("bytes.len() != expected", WINDOWS_CAPTURE_POLICY)
        self.assertIn("a >= 250", WINDOWS_CAPTURE_POLICY)
        transition = re.search(
            r"if let Some\(value\) = protected\.observe_bgra.*?if protected\.is_protected\(\)",
            WINDOWS,
            re.S,
        )
        self.assertIsNotNone(transition)
        source = transition.group(0)
        self.assertLess(source.index('emit("patches-updated"'), source.index("window.hide()"))
        self.assertLess(source.index("window.hide()"), source.index('emit("protected-content-changed"'))
        self.assertIn("window.show()", source)
        self.assertIn("Possible protected or unavailable content · overlays paused", WINDOWS_CONTROL_PANEL)

    def test_windows_lifecycle_recovery_is_fail_closed_and_bounded(self):
        for event in (
            "windows-power-suspension-changed",
            "windows-session-lock-changed",
            "windows-display-topology-changed",
            "windows-lifecycle-availability-changed",
        ):
            self.assertIn(event, WINDOWS_LIFECYCLE)
            self.assertIn(event, WINDOWS)
        self.assertIn("WTSRegisterSessionNotification", WINDOWS_LIFECYCLE)
        self.assertIn("PBT_APMSUSPEND", WINDOWS_LIFECYCLE)
        self.assertIn("PBT_APMRESUMEAUTOMATIC", WINDOWS_LIFECYCLE)
        self.assertIn("FIRST_FRAME_TIMEOUT_MS", WINDOWS)
        self.assertIn("RECOVERY_DELAYS_MS", WINDOWS)
        self.assertIn("capture_desired", WINDOWS)
        self.assertIn("lifecycle_observer_available", WINDOWS)

    def test_linux_portal_revocation_resize_and_frame_validation_fail_closed(self):
        self.assertIn("terminal_stream_message", LINUX_WAYLAND_CAPTURE)
        self.assertIn("PipeWire portal stream was revoked or disconnected", LINUX_WAYLAND_CAPTURE)
        self.assertIn("state_stop.load(Ordering::Acquire)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("!format_stop.load(Ordering::Acquire)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("!format_reported.load(Ordering::Acquire)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("process_terminal.load(Ordering::Acquire)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("FORMAT_RENEGOTIATION_TIMEOUT", LINUX_WAYLAND_CAPTURE)
        self.assertIn("PipeWire video format renegotiation timed out", LINUX_WAYLAND_CAPTURE)
        self.assertIn("format: Option<spa::param::video::VideoInfoRaw>", LINUX_WAYLAND_CAPTURE)
        self.assertIn("user_data.format = Some(format)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("Ok(CaptureEvent::Reset)", LINUX_WAYLAND_CAPTURE)
        self.assertIn("fn event_priority", LINUX_WAYLAND_CAPTURE)
        self.assertIn("Ok(CaptureEvent::Reset) => 1", LINUX_WAYLAND_CAPTURE)
        self.assertIn("checked_mul(4)", LINUX_CAPTURE)
        self.assertIn("Ok(CaptureEvent::Reset) =>", LINUX)
        reset_case = LINUX.split("Ok(CaptureEvent::Reset) =>", 1)[1].split("Err(error) =>", 1)[0]
        self.assertLess(reset_case.index("state.latest_frame.store(None)"), reset_case.index('window.hide()'))
        self.assertIn("if !frame.is_valid_packed_bgra()", LINUX)

    def test_linux_runtime_resize_and_labeling_work_are_generation_owned(self):
        self.assertIn("task.is_finished()", LINUX_STATE)
        self.assertIn("take_finished_capture_runtime", LINUX)
        self.assertIn("advance_capture_generation(&state.capture_generation)", LINUX)
        self.assertIn("owns_capture_generation", LINUX)
        self.assertIn("frame_belongs_to_active_generation", LINUX)
        self.assertIn("capture_screenshot_serialized", LINUX)
        serialized = LINUX.split("async fn capture_screenshot_serialized", 1)[1].split(
            "fn capture_screenshot_inner", 1
        )[0]
        self.assertIn("state.capture.lock().await", serialized)
        self.assertIn("runtime.generation == state.capture_generation", serialized)
        self.assertGreaterEqual(LINUX.count("screenshot_generation_is_current"), 4)
        stale_cleanup = LINUX.split(
            "if !screenshot_generation_is_current(state, generation)", 3
        )[3]
        self.assertIn("remove_file(&destination)", stale_cleanup)

        self.assertIn("ROOT_GEOMETRY_CHECK_INTERVAL", LINUX_X11_CAPTURE)
        self.assertIn("get_geometry(self.root)", LINUX_X11_CAPTURE)
        self.assertIn("create_shm_mapping", LINUX_X11_CAPTURE)
        self.assertIn("self.mapping = new_mapping", LINUX_X11_CAPTURE)
        self.assertIn("Err(frame_error) => match self.refresh_root_geometry()", LINUX_X11_CAPTURE)
        self.assertIn("return Ok(CaptureEvent::Reset)", LINUX_X11_CAPTURE)
        self.assertLess(
            LINUX_X11_CAPTURE.index("self.mapping = new_mapping"),
            LINUX_X11_CAPTURE.index("return Ok(CaptureEvent::Reset)"),
        )

    def test_release_training_uses_shared_fail_closed_gate(self):
        for platform, source in (("Windows", WINDOWS), ("Linux", LINUX)):
            self.assertIn("developer_training_runtime_available()", source)
            self.assertIn("release builds are inference-only", source)
            self.assertIn(f'Err("{platform} source training dispatch is not implemented', source)
        self.assertIn("Inference-only package", TRAINING_UI)
        self.assertIn("never install Python", TRAINING_UI)
        self.assertNotIn("auto.ps1", TRAINING_UI)

    def test_shared_frontend_workflows_are_native_and_fail_visible(self):
        self.assertIn("getBehaviorContract", IPC)
        self.assertNotIn('capture_screenshot_for_labeling\").catch', IPC)
        self.assertIn("pointerdown", REGION_EDITOR)
        self.assertIn("lb.addRegion", REGION_EDITOR)
        self.assertIn("lb.deleteRegion", REGION_EDITOR)
        self.assertIn("lb.clearRegions", REGION_EDITOR)
        self.assertNotIn("TODO(windows-port / linux-port)", REGION_EDITOR)
        self.assertIn("lb.loadLabel(entry.labelPath)", LABELING)
        self.assertIn("lb.saveLabel(entry.labelPath", LABELING)
        self.assertIn("lb.discardScreenshot(entry.path)", LABELING)
        self.assertIn("Label is read-only because its sidecar is incompatible", LABELING)
        for source in (WINDOWS, LINUX):
            self.assertIn("label_path: label.clone()", source)
            self.assertIn("if path.exists()", source)
            self.assertIn("LabelDocument::load(&path)", source)
            self.assertIn("move_regular_file_no_replace(&destination, &label)", source)

    def test_ui_name_panic_and_local_data_policy_are_consistent(self):
        frontend_sources = [IPC] + [
            path.read_text() for path in (ROOT / "platform/_shared-frontend/src").glob("*.html")
        ]
        self.assertFalse(any("LiveBlocker" in source for source in frontend_sources))
        for forbidden in ("fetch(", "XMLHttpRequest", "new WebSocket", "navigator.sendBeacon"):
            self.assertFalse(any(forbidden in source for source in frontend_sources))
        for window in ("labelingWindow", "trainingDashboardWindow", "miniHUDWindow"):
            self.assertIn(f"{window}?.orderOut(nil)", MAC_CONTROLLER)
        self.assertIn('for label in ["editor", "render", "labeling", "training"]', WINDOWS)
        self.assertIn('for label in ["editor", "render", "labeling", "training"]', LINUX)

    def test_model_update_adapters_share_fail_closed_contract(self):
        for platform, source in (("windows", WINDOWS_UPDATES), ("linux", LINUX_UPDATES)):
            self.assertIn("apply_verified_file_update", source)
            self.assertIn("recover_verified_active_manifest", source)
            self.assertIn("TrustedKeyringDocument::from_json(&json, true)", source)
            self.assertIn('"resources/trusted-model-keys.json"', source)
            self.assertIn("let _update_guard = state.model_update.lock()", source)
            self.assertIn("let release_floor = packaged", source)
            self.assertIn("Detector::load(path)", source)
            self.assertIn("load_authenticated_packaged", source)
            keyring = (ROOT / f"platform/{platform}/src-tauri/resources/trusted-model-keys.json").read_text()
            self.assertEqual(json.loads(keyring), {"schemaVersion": 1, "keys": []})
            build = (ROOT / f"platform/{platform}/src-tauri/build.rs").read_text()
            self.assertIn("release packages require a nonempty", build)
            self.assertIn("LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING", build)
        self.assertIn('invoke<ModelUpdateReceipt>("install_model_update"', IPC)
        self.assertNotIn('Detector::load(&candidate)', WINDOWS)

    def test_linux_gpu_inpainting_is_bounded_and_fails_to_cpu(self):
        self.assertIn('include_str!("inpainting.wgsl")', LINUX_INPAINTING)
        self.assertIn("dispatch_workgroups", LINUX_INPAINTING)
        self.assertIn("wgpu readback exceeded 250 ms", LINUX_INPAINTING)
        self.assertIn("wgpu dispatch/readback worker exceeded 300 ms", LINUX_INPAINTING)
        self.assertIn("wgpu initialization exceeded 750 ms", LINUX_INPAINTING)
        self.assertIn('self.backend_status = "cpu_fallback_after_gpu_error"', LINUX_INPAINTING)
        self.assertIn("max_storage_buffer_binding_size", LINUX_INPAINTING)
        self.assertIn("@compute @workgroup_size(8, 8, 1)", LINUX_WGSL)
        self.assertIn("inpainting_backend", LINUX)

    def test_linux_release_runtime_and_provider_contract_is_fail_closed(self):
        self.assertIn("select at most one optional Linux ONNX Runtime", LINUX_DETECTION)
        self.assertIn("configured_inference_backends", LINUX_DETECTION)
        self.assertIn("initialize_runtime(packaged_runtime)", LINUX)
        for required in (
            "libonnxruntime.so",
            "THIRD-PARTY-NOTICES.txt",
            "libonnxruntime_providers_shared.so",
            "libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_rocm.so",
            "libonnxruntime_providers_openvino.so",
            "libonnxruntime_providers_tensorrt.so",
        ):
            self.assertIn(required, LINUX_BUILD)
        self.assertIn("LIVEBLOCK_ALLOW_UNPACKAGED_ORT", LINUX_BUILD)
        self.assertIn("cannot accompany production trust roots", LINUX_BUILD)
        self.assertIn('"resources": ["resources/**/*"]', (ROOT / "platform/linux/src-tauri/tauri.conf.json").read_text())
        self.assertIn("unpackaged ONNX Runtime was accepted", CI)

    def test_html_consumers_only_call_exported_ipc_methods(self):
        exported = set(re.findall(r"^  ([A-Za-z][A-Za-z0-9]+):", IPC, re.M))
        calls = set()
        for html in (ROOT / "platform/_shared-frontend/src").glob("*.html"):
            calls.update(re.findall(r"\blb\.([A-Za-z][A-Za-z0-9]+)\b", html.read_text()))
        self.assertTrue(calls)
        self.assertEqual(calls - exported, set())

    def test_label_and_capability_frontend_contracts_are_versioned(self):
        self.assertIn("schemaVersion: 1", IPC)
        self.assertIn("releaseReady: boolean", IPC)
        self.assertIn('invoke<DesktopCapabilityProfile>("get_capabilities")', IPC)
        self.assertIn('invoke<DesktopBehaviorContract>("get_behavior_contract")', IPC)
        self.assertIn('runtimeClasses: ["Logo", "Ad banner", "Sponsored"]', IPC)


if __name__ == "__main__":
    unittest.main()
