import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = (ROOT / "tools/release_windows.ps1").read_text()
DOC = (ROOT / "docs/WINDOWS_RELEASE.md").read_text()
CI = (ROOT / ".github/workflows/ci.yml").read_text()
CONFIG = json.loads((ROOT / "platform/windows/src-tauri/tauri.conf.json").read_text())
BUILD_RS = (ROOT / "platform/windows/src-tauri/build.rs").read_text()
CARGO_TOML = (ROOT / "platform/windows/src-tauri/Cargo.toml").read_text()


class WindowsPackagingContractTests(unittest.TestCase):
    def test_release_script_separates_build_only_and_signed_execution(self):
        for mode in ("DryRun", "BuildOnly", "Execute"):
            self.assertIn(f'"{mode}"', SCRIPT)
        self.assertIn("LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING", SCRIPT)
        self.assertIn("The empty model-keyring override is forbidden", SCRIPT)
        self.assertIn("BuildOnly mode refuses leftover production model staging", SCRIPT)
        self.assertIn("verify_signed_onnx_bundle.py", SCRIPT)
        self.assertIn("git status --porcelain --untracked-files=all", SCRIPT)
        self.assertIn("--bundles msi,nsis --ci", SCRIPT)
        self.assertIn("windows-installers-build-only", SCRIPT)
        self.assertIn("windows-installers-signed", SCRIPT)
        self.assertIn("Output directory already exists", SCRIPT)

    def test_execute_uses_certificate_store_and_verifies_final_bytes(self):
        self.assertIn("LB_WINDOWS_CERTIFICATE_SHA1", SCRIPT)
        self.assertIn("LB_WINDOWS_TIMESTAMP_URL", SCRIPT)
        self.assertIn('StartsWith("https://"', SCRIPT)
        self.assertIn("signtool.exe", SCRIPT)
        self.assertIn("/fd SHA256", SCRIPT)
        self.assertIn("/td SHA256", SCRIPT)
        self.assertIn("verify /pa /all /v", SCRIPT)
        self.assertIn("Get-AuthenticodeSignature", SCRIPT)
        self.assertNotIn("CERTIFICATE_PASSWORD", SCRIPT)
        self.assertNotIn(".pfx", SCRIPT.lower())
        self.assertIn("release_evidence.py inventory", SCRIPT)
        self.assertIn("Get-FileHash -Algorithm SHA256", SCRIPT)
        self.assertIn("Wait-RegularFileStable", SCRIPT)
        self.assertIn("did not become stable within 30 seconds", SCRIPT)
        self.assertIn('Start-Process -FilePath "msiexec.exe"', SCRIPT)
        self.assertIn('"/a"', SCRIPT)
        self.assertIn("-Wait -PassThru", SCRIPT)
        self.assertIn('"onnxruntime.dll"', SCRIPT)
        self.assertIn("msi-payload-inventory.json", SCRIPT)

    def test_model_staging_is_cleanup_guarded_and_distribution_limits_are_explicit(self):
        self.assertIn("finally", SCRIPT)
        self.assertIn("WriteAllBytes($DevelopmentKeyring, $originalKeyring)", SCRIPT)
        self.assertIn("Remove-Item -LiteralPath $ArtifactStaging", SCRIPT)
        self.assertIn('$RuntimeStaging = Join-Path $Resources "onnxruntime.dll"', SCRIPT)
        self.assertIn("stage_windows_onnxruntime.py", SCRIPT)
        self.assertIn("onnxruntime-THIRD-PARTY-NOTICES.txt", SCRIPT)
        self.assertIn("onnxruntime-LICENSE.txt", SCRIPT)
        self.assertIn("Remove-Item -LiteralPath $RuntimeStaging", SCRIPT)
        self.assertIn("Do not distribute these installers", DOC)
        self.assertIn("MSIX creation", DOC)
        self.assertIn("not implemented", DOC)
        self.assertIn("remains open", DOC)
        self.assertIn("empty development trust", DOC)

    def test_production_build_cryptographically_requires_complete_model_resources(self):
        self.assertIn("resources/liveblock-detector.onnx", BUILD_RS)
        self.assertIn("resources/liveblock-detector.manifest.json", BUILD_RS)
        self.assertIn("must be a regular non-symlink file", BUILD_RS)
        self.assertIn("ArtifactFormat::Onnx", BUILD_RS)
        self.assertIn("manifest", BUILD_RS)
        self.assertIn(".verify(artifact, &ring)", BUILD_RS)
        self.assertIn("empty-ring override cannot accompany", BUILD_RS)

    def test_tauri_and_ci_build_both_unsigned_installer_formats(self):
        self.assertEqual(CONFIG["build"]["beforeBuildCommand"], "cd ../_shared-frontend && npm run build")
        self.assertIn("npm ci", SCRIPT)
        self.assertIn("native CLI module locks itself", SCRIPT)
        self.assertEqual(CONFIG["bundle"]["targets"], ["msi", "nsis"])
        self.assertFalse(CONFIG["bundle"]["windows"]["allowDowngrades"])
        self.assertIn("release_windows.ps1 -Mode BuildOnly", CI)
        self.assertIn("windows-build-only-package-evidence", CI)
        self.assertIn("package-inventory.json", CI)
        self.assertIn("package.sha256", CI)
        self.assertIn("msi-payload-inventory.json", CI)
        self.assertIn("msi-extracted/**", CI)
        self.assertIn('"load-dynamic"', CARGO_TOML)
        self.assertNotIn('"download-binaries"', CARGO_TOML)


if __name__ == "__main__":
    unittest.main()
