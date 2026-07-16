import hashlib
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = (ROOT / "tools/release_windows.ps1").read_text()
LIFECYCLE = (ROOT / "tools/test_windows_package_lifecycle.ps1").read_text()
DOC = (ROOT / "docs/WINDOWS_RELEASE.md").read_text()
CI = (ROOT / ".github/workflows/ci.yml").read_text()
CONFIG = json.loads((ROOT / "platform/windows/src-tauri/tauri.conf.json").read_text())
BUILD_RS = (ROOT / "platform/windows/src-tauri/build.rs").read_text()
CARGO_TOML = (ROOT / "platform/windows/src-tauri/Cargo.toml").read_text()
NSIS_TEMPLATE_PATH = ROOT / "platform/windows/src-tauri/nsis/installer.nsi"
NSIS_TEMPLATE = NSIS_TEMPLATE_PATH.read_bytes()
NSIS_UPSTREAM_SHA256 = "ee84148e405adc4d736a46456dd8345a644751bd1f28a335dd7fd833a32d7c3e"
NSIS_GUARD_START = b"  ; LIVEBLOCK BEGIN DOWNGRADE GUARD\n"
NSIS_GUARD_END = b"  ; LIVEBLOCK END DOWNGRADE GUARD\n\n"


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
        self.assertIn("windows-installers-version-fixture-build-only", SCRIPT)
        self.assertIn("windows-installers-signed", SCRIPT)
        self.assertIn("BuildOnlyFixtureVersion is allowed only in BuildOnly mode", SCRIPT)
        self.assertIn("BuildOnlyFixtureVersion must be lower", SCRIPT)
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
        self.assertIn("MSIX/App Installer", DOC)
        self.assertIn("unimplemented", DOC)
        self.assertIn("remain required", DOC)
        self.assertIn("empty development trust", DOC)

    def test_execute_emits_selected_staged_nsis_update_with_obligations(self):
        self.assertIn('applicationUpdateChannel = "stable-staged-nsis"', SCRIPT)
        self.assertIn("applicationUpdateBundleProduced = ($Mode -eq \"Execute\")", SCRIPT)
        self.assertIn('if ($Mode -eq "Execute")', SCRIPT)
        self.assertIn("windows_application_update.py create", SCRIPT)
        self.assertIn("verify_windows_application_update.ps1", SCRIPT)
        self.assertIn("Get-CertificateSha256", SCRIPT)
        self.assertIn("TimeStamperCertificate", SCRIPT)
        self.assertIn("Pkcs.SignedCms", SCRIPT)
        self.assertIn("windows-application-update.p7s", SCRIPT)
        self.assertIn("FileMode]::CreateNew", SCRIPT)
        self.assertIn("windows-stable-staged-nsis-update", SCRIPT)
        for required in (
            "liveblock.cdx.json",
            "dependency-licenses.json",
            "dependency-obligations.json",
            "dependency-license-decisions.json",
            "mpl-source-offer.json",
            "MPL-2.0.txt",
        ):
            self.assertIn(required, SCRIPT)
        self.assertIn("Validate Windows release PowerShell syntax", CI)
        self.assertIn("verify_windows_application_update.ps1", CI)
        self.assertIn("stable staged NSIS", DOC)
        self.assertIn("does not auto-download", DOC)
        self.assertIn("independently supplied trusted SHA-256", DOC)
        self.assertIn("BuildOnly packages cannot", DOC)

    def test_production_build_cryptographically_requires_complete_model_resources(self):
        self.assertIn("resources/liveblock-detector.onnx", BUILD_RS)
        self.assertIn("resources/liveblock-detector.manifest.json", BUILD_RS)
        self.assertIn("must be a regular non-symlink file", BUILD_RS)
        self.assertIn("ArtifactFormat::Onnx", BUILD_RS)
        self.assertIn("manifest", BUILD_RS)
        self.assertIn(".verify(artifact, &ring)", BUILD_RS)
        self.assertIn("empty-ring override cannot accompany", BUILD_RS)

    def test_build_only_lifecycle_is_clean_bounded_and_fail_closed(self):
        self.assertIn("windows-installers-build-only", LIFECYCLE)
        self.assertIn("windows-installers-version-fixture-build-only", LIFECYCLE)
        self.assertIn("Assert-SingleRegisteredVersion", LIFECYCLE)
        self.assertIn('"/fa"', LIFECYCLE)
        self.assertIn("downgradeRejected = $true", LIFECYCLE)
        self.assertIn("sameVersionReinstall = $true", LIFECYCLE)
        self.assertIn("silentDowngradeRejected = $true", LIFECYCLE)
        self.assertIn("downgradeProtectionPassed = $true", LIFECYCLE)
        self.assertIn("currentRemainedInstalledAfterDowngradeProbe = $true", LIFECYCLE)
        self.assertIn("NSIS downgrade guard expected exit code 2", LIFECYCLE)
        self.assertIn("Assert-UserDataSentinel", LIFECYCLE)
        self.assertIn("userDataSentinelPreservedAcrossTransitionsAndUninstall", LIFECYCLE)
        self.assertIn("promotedModelEmbedded -ne $false", LIFECYCLE)
        self.assertIn("Get-LiveBlockUninstallEntries", LIFECYCLE)
        self.assertIn('"/i"', LIFECYCLE)
        self.assertIn('"/x"', LIFECYCLE)
        self.assertIn('ArgumentList "/S"', LIFECYCLE)
        self.assertIn("Invoke-BoundedLaunch", LIFECYCLE)
        self.assertIn("Test-BuildOnlyRuntimePayload", LIFECYCLE)
        self.assertIn("@($keyring.keys).Count -ne 0", LIFECYCLE)
        self.assertIn("[Runtime.InteropServices.NativeLibrary]::Load", LIFECYCLE)
        self.assertIn("emptyDevelopmentKeyringVerified = $true", LIFECYCLE)
        self.assertIn("packagedOnnxRuntimeLoadable = $true", LIFECYCLE)
        self.assertNotIn("outputObservationWaitSeconds", LIFECYCLE)
        self.assertIn("Stop-Process", LIFECYCLE)
        self.assertIn("Wait-Removed", LIFECYCLE)
        self.assertIn("windows-build-only-msi-installed", LIFECYCLE)
        self.assertIn("windows-build-only-nsis-installed", LIFECYCLE)
        self.assertIn("release_evidence.py verify", LIFECYCLE)
        self.assertIn("productionModelAndTrustRoots = $false", LIFECYCLE)
        self.assertIn("signedAndTimestamped = $false", LIFECYCLE)
        self.assertIn("hardwareCertification = $false", LIFECYCLE)
        self.assertIn("finally", LIFECYCLE)

    def test_custom_nsis_template_is_pinned_with_only_downgrade_guard_delta(self):
        self.assertEqual(
            CONFIG["bundle"]["windows"]["nsis"]["template"],
            "nsis/installer.nsi",
        )
        self.assertEqual(NSIS_TEMPLATE.count(NSIS_GUARD_START), 1)
        self.assertEqual(NSIS_TEMPLATE.count(NSIS_GUARD_END), 1)
        start = NSIS_TEMPLATE.index(NSIS_GUARD_START)
        end = NSIS_TEMPLATE.index(NSIS_GUARD_END, start) + len(NSIS_GUARD_END)
        upstream = NSIS_TEMPLATE[:start] + NSIS_TEMPLATE[end:]
        self.assertEqual(hashlib.sha256(upstream).hexdigest(), NSIS_UPSTREAM_SHA256)
        guard = NSIS_TEMPLATE[start:end].decode()
        self.assertIn('ReadRegStr $R7 SHCTX "${UNINSTKEY}" "DisplayVersion"', guard)
        self.assertIn('nsis_tauri_utils::SemverCompare "${VERSION}" $R7', guard)
        self.assertIn("SetErrorLevel 2", guard)
        self.assertIn("Abort", guard)
        self.assertIn("malformed", guard)

    def test_tauri_and_ci_build_both_unsigned_installer_formats(self):
        self.assertEqual(CONFIG["build"]["beforeBuildCommand"], "cd ../_shared-frontend && npm run build")
        self.assertIn("npm ci", SCRIPT)
        self.assertIn("native CLI module locks itself", SCRIPT)
        self.assertEqual(CONFIG["bundle"]["targets"], ["msi", "nsis"])
        self.assertFalse(CONFIG["bundle"]["windows"]["allowDowngrades"])
        self.assertIn("release_windows.ps1 -Mode BuildOnly", CI)
        self.assertIn("-BuildOnlyFixtureVersion 0.0.9", CI)
        self.assertIn("-PreviousEvidenceDir tools/runs/ci-windows-previous", CI)
        self.assertIn("windows-build-only-package-evidence", CI)
        self.assertIn("package-inventory.json", CI)
        self.assertIn("package.sha256", CI)
        self.assertIn("msi-payload-inventory.json", CI)
        self.assertIn("msi-extracted/**", CI)
        self.assertIn("test_windows_package_lifecycle.ps1", CI)
        self.assertIn("lifecycle/**", CI)
        self.assertIn("if: always()", CI)
        self.assertIn('"load-dynamic"', CARGO_TOML)
        self.assertNotIn('"download-binaries"', CARGO_TOML)


if __name__ == "__main__":
    unittest.main()
