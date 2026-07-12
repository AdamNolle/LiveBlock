import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = (ROOT / "platform/windows/src-tauri/src/main.rs").read_text()
LINUX = (ROOT / "platform/linux/src-tauri/src/main.rs").read_text()
IPC = (ROOT / "platform/_shared-frontend/src/ipc.ts").read_text()

EXPECTED_COMMANDS = {
    "get_capabilities",
    "start_capture",
    "stop_capture",
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
    "show_window",
    "hide_window",
    "quit",
}


def handler_commands(source: str) -> set[str]:
    match = re.search(r"tauri::generate_handler!\[(.*?)\]\)", source, re.S)
    if not match:
        raise AssertionError("generate_handler list not found")
    return {name.strip() for name in match.group(1).split(",") if name.strip()}


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
            self.assertIn('emit("capture-state-changed", true)', source)
            self.assertIn('emit("capture-state-changed", false)', source)
            self.assertIn('serde(rename_all = "camelCase")', source)
            self.assertIn("struct ScreenshotData", source)
        self.assertIn("startCapture: (monitorId: string)", IPC)
        self.assertIn("loadScreenshot: (path: string) => invoke<ScreenshotData>", IPC)
        self.assertIn('listen<boolean>("capture-state-changed"', IPC)

    def test_release_training_uses_shared_fail_closed_gate(self):
        for source in (WINDOWS, LINUX):
            self.assertIn("developer_training_runtime_available()", source)
            self.assertIn("release builds are inference-only", source)
        self.assertIn('Err("Linux source training dispatch is not implemented"', LINUX)

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


if __name__ == "__main__":
    unittest.main()
