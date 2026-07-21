import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
README = (ROOT / "README.md").read_text()
RUNBOOKS = (ROOT / "docs/DESKTOP_VALIDATION_RUNBOOKS.md").read_text()
PRIVACY = (ROOT / "docs/PRIVACY_SECURITY_REVIEW.md").read_text()
RELEASE_NOTES = (ROOT / "docs/RELEASE_NOTES_DRAFT.md").read_text()
RELEASE_ARTIFACTS = (ROOT / "docs/RELEASE_ARTIFACTS.md").read_text()
WINDOWS_MAIN = (ROOT / "platform/windows/src-tauri/src/main.rs").read_text()
LINUX_MAIN = (ROOT / "platform/linux/src-tauri/src/main.rs").read_text()


class ReleaseDocumentationContractTests(unittest.TestCase):
    def test_readme_links_release_claim_boundaries(self):
        for path in (
            "docs/DESKTOP_RELEASE_MATRIX.md",
            "docs/DESKTOP_VALIDATION_RUNBOOKS.md",
            "docs/PRIVACY_SECURITY_REVIEW.md",
            "docs/RELEASE_NOTES_DRAFT.md",
            "docs/MODEL_DISTRIBUTION_SECURITY.md",
        ):
            self.assertIn(f"]({path})", README)
        for stale_claim in (
            "Windows-10%2B",
            "Capture (60 Hz)",
            "Experimental scaffold",
            "D3D11 texture / DMA-BUF",
            "33 unit tests",
        ):
            self.assertNotIn(stale_claim, README)
        self.assertIn("0/21", README)
        self.assertIn("No production-ready package", README)

    def test_runbooks_cover_every_required_manual_scenario(self):
        for phrase in (
            "Clean install",
            "Upgrade and uninstall",
            "Permissions and lifecycle",
            "Displays, panic, and soak",
            "Crash-recovery scenarios",
            "sleep/wake",
            "lock/unlock",
            "multi-output",
            "blocked-credentials",
            "verified-real-device",
            "desktop_validation_evidence.py init",
            "desktop_validation_evidence.py record",
            "desktop_validation_evidence.py hashes",
        ):
            self.assertIn(phrase, RUNBOOKS)
        self.assertIn("Native lifecycle observation and bounded\nrecovery are implemented but uncertified", RUNBOOKS)
        self.assertIn("GNOME evidence must use the bounded movable preview", RUNBOOKS)

    def test_release_notes_are_explicitly_unreleased_and_blocked(self):
        self.assertIn("Status: not released; not release-ready", RELEASE_NOTES)
        self.assertIn("No current platform profile reports `releaseReady: true`", RELEASE_NOTES)
        self.assertIn("Human review is **0 approved / 21 pending**", RELEASE_NOTES)
        self.assertIn("Zero-copy DirectML texture inference is not implemented", RELEASE_NOTES)
        self.assertIn("Bounded wgpu/WGSL patch generation is implemented", RELEASE_NOTES)
        self.assertIn("Real Vulkan/GL GPU and zero-copy compositor execution remain uncertified", RELEASE_NOTES)

    def test_release_blocker_diagnostic_preserves_external_authority_boundary(self):
        normalized = " ".join(RELEASE_ARTIFACTS.split())
        for phrase in (
            "tools/release_blockers.py",
            "contracts/release-blocker-assessment.schema.json",
            "accountable dependency approval",
            "production signing/publication",
            "real-device platform validation",
            "cannot authorize model installation",
        ):
            self.assertIn(phrase, normalized)

    def test_privacy_review_preserves_evidence_boundaries(self):
        normalized = " ".join(PRIVACY.split()).lower()
        for phrase in (
            "no frame upload client exists",
            "Arbitrary paths",
            "same user",
            "network-denied real-device functional tests",
            "No analytics or remote crash reporter",
            "Authentication does not substitute for detector quality",
        ):
            self.assertIn(phrase.lower(), normalized)

    def test_tauri_runtime_has_csp_and_no_unused_privileged_plugins(self):
        for platform in ("windows", "linux"):
            config = json.loads(
                (ROOT / f"platform/{platform}/src-tauri/tauri.conf.json").read_text()
            )
            csp = config["app"]["security"]["csp"]
            self.assertIsInstance(csp, str)
            self.assertIn("default-src 'self'", csp)
            self.assertIn("script-src 'self'", csp)
            cargo = (ROOT / f"platform/{platform}/src-tauri/Cargo.toml").read_text()
            main = (ROOT / f"platform/{platform}/src-tauri/src/main.rs").read_text()
            for plugin in ("tauri-plugin-shell", "tauri-plugin-fs", "tauri-plugin-dialog", "tauri-plugin-os"):
                self.assertNotIn(plugin, cargo)
            for initializer in ("tauri_plugin_shell", "tauri_plugin_fs", "tauri_plugin_dialog", "tauri_plugin_os"):
                self.assertNotIn(initializer, main)

    def test_renderer_paths_are_constrained_before_file_operations(self):
        for source in (WINDOWS_MAIN, LINUX_MAIN):
            self.assertIn("paths::validate_screenshot_path(&path)", source)
            self.assertIn("paths::validate_label_path(&path, false)", source)
            self.assertIn("paths::validate_label_path(&path, true)", source)
            self.assertIn("validate_label_binding(&path, &doc)", source)
            self.assertIn("move_regular_file_pair_no_replace(", source)
            self.assertIn("(source.as_path(), destination.as_path())", source)


if __name__ == "__main__":
    unittest.main()
