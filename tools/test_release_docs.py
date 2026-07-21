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
REVIEW_SCRIPT = (ROOT / "tools/corpus/review_labels.py").read_text()
REVIEW_UI = (ROOT / "tools/corpus/review_ui.html").read_text()
REVIEW_LAUNCHER = ROOT / "tools/review_sports_ads.sh"
HUMAN_REVIEW_DOC = (ROOT / "docs/SPORTS_ADS_HUMAN_REVIEW.md").read_text()


class ReleaseDocumentationContractTests(unittest.TestCase):
    def test_readme_links_release_claim_boundaries(self):
        for path in (
            "docs/DESKTOP_RELEASE_MATRIX.md",
            "docs/DESKTOP_VALIDATION_RUNBOOKS.md",
            "docs/PRIVACY_SECURITY_REVIEW.md",
            "docs/RELEASE_NOTES_DRAFT.md",
            "docs/MODEL_DISTRIBUTION_SECURITY.md",
            "docs/SPORTS_ADS_HUMAN_REVIEW.md",
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

    def test_human_review_has_one_command_attributable_local_ui(self):
        launcher = REVIEW_LAUNCHER.read_text()
        self.assertTrue(REVIEW_LAUNCHER.stat().st_mode & 0o111)
        self.assertIn("review_labels.py", launcher)
        self.assertIn("human-review-plan.json", launcher)
        self.assertIn("ReviewSession", REVIEW_SCRIPT)
        self.assertIn('(\"127.0.0.1\", args.port)', REVIEW_SCRIPT)
        self.assertIn("MAX_REQUEST_BYTES", REVIEW_SCRIPT)
        self.assertIn("write_review_document", REVIEW_SCRIPT)
        self.assertIn("reviewer identity is fixed", REVIEW_SCRIPT)
        self.assertIn("X-LiveBlock-Review-Token", REVIEW_SCRIPT)
        self.assertIn("request Origin is not the local review page", REVIEW_SCRIPT)
        self.assertIn("review_item_revision", REVIEW_SCRIPT)
        self.assertIn("personal full-image review attestation is required", REVIEW_SCRIPT)
        self.assertIn("I personally inspected the full image", REVIEW_UI)
        self.assertIn("Approve human review", REVIEW_UI)
        self.assertIn("/api/status", REVIEW_UI)
        self.assertIn("imageReady", REVIEW_UI)
        self.assertIn("navigationReady=false", REVIEW_UI)
        self.assertIn("function navigate(delta){if(!navigationReady)return", REVIEW_UI)
        self.assertIn("Discard unsaved annotation changes", REVIEW_UI)
        self.assertIn("function markDirty(){dirty=true;clearAttestation()}", REVIEW_UI)
        self.assertIn("./tools/review_sports_ads.sh", HUMAN_REVIEW_DOC)
        self.assertIn("personally-inspected-full-image-v1", HUMAN_REVIEW_DOC)
        self.assertIn("fully compromised same-user process", HUMAN_REVIEW_DOC)

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
