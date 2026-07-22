import argparse
import json
import tempfile
import unittest
from pathlib import Path

from windows_application_update import (
    ARTIFACT_KEYS,
    DESCRIPTOR_NAME,
    SIGNATURE_NAME,
    UpdateContractError,
    create_descriptor,
    verify_bundle,
)

ROOT = Path(__file__).resolve().parents[1]
POWERSHELL = (ROOT / "tools/verify_windows_application_update.ps1").read_text()
DOC = (ROOT / "docs/WINDOWS_RELEASE.md").read_text()

CERTIFICATE = "1" * 64
TIMESTAMP_CERTIFICATE = "2" * 64
COMMIT = "a" * 40


class WindowsApplicationUpdateTests(unittest.TestCase):
    def make_bundle(self, root: Path) -> Path:
        bundle = root / "bundle"
        bundle.mkdir()
        names = {
            "installer": "LiveBlock_0.1.0_x64-setup.exe",
            "sbom": "liveblock.cdx.json",
            "licenseReport": "dependency-licenses.json",
            "obligationReport": "dependency-obligations.json",
            "licenseDecisions": "dependency-license-decisions.json",
            "mplSourceOffer": "mpl-source-offer.json",
            "mplLicense": "MPL-2.0.txt",
        }
        for index, key in enumerate(ARTIFACT_KEYS, start=1):
            (bundle / names[key]).write_bytes(f"{key}:{index}\n".encode())
        args = argparse.Namespace(
            bundle=str(bundle),
            version="0.1.0",
            commit=COMMIT,
            publisher_subject="CN=LiveBlock Test Publisher",
            certificate_sha256=CERTIFICATE,
            timestamp_certificate_sha256=TIMESTAMP_CERTIFICATE,
            **{key: str(bundle / names[key]) for key in ARTIFACT_KEYS},
        )
        create_descriptor(args)
        (bundle / SIGNATURE_NAME).write_bytes(b"test-only detached CMS placeholder")
        return bundle

    def test_create_and_verify_closed_bundle(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = self.make_bundle(Path(temporary))
            descriptor = verify_bundle(bundle, CERTIFICATE)
            self.assertEqual(descriptor["channel"], "stable-staged-nsis")
            self.assertEqual(descriptor["source"]["tag"], "desktop-v0.1.0")
            self.assertEqual(set(descriptor["artifacts"]), set(ARTIFACT_KEYS))
            self.assertEqual(
                {entry.name for entry in bundle.iterdir()},
                {DESCRIPTOR_NAME, SIGNATURE_NAME}
                | {record["fileName"] for record in descriptor["artifacts"].values()},
            )

    def test_tamper_wrong_certificate_and_extra_file_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = self.make_bundle(Path(temporary))
            descriptor = json.loads((bundle / DESCRIPTOR_NAME).read_text())
            installer = bundle / descriptor["artifacts"]["installer"]["fileName"]
            installer.write_bytes(b"changed")
            with self.assertRaisesRegex(UpdateContractError, "does not match descriptor"):
                verify_bundle(bundle, CERTIFICATE)

        with tempfile.TemporaryDirectory() as temporary:
            bundle = self.make_bundle(Path(temporary))
            with self.assertRaisesRegex(UpdateContractError, "trusted expected certificate"):
                verify_bundle(bundle, "3" * 64)
            (bundle / "unexpected.txt").write_text("unexpected")
            with self.assertRaisesRegex(UpdateContractError, "contents must be exactly"):
                verify_bundle(bundle, CERTIFICATE)

    def test_future_schema_unknown_fields_and_path_names_fail(self):
        mutations = (
            lambda value: value.update(schemaVersion=2),
            lambda value: value.update(untrusted=True),
            lambda value: value["artifacts"]["installer"].update(fileName="nested/setup.exe"),
        )
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                with tempfile.TemporaryDirectory() as temporary:
                    bundle = self.make_bundle(Path(temporary))
                    path = bundle / DESCRIPTOR_NAME
                    descriptor = json.loads(path.read_text())
                    mutate(descriptor)
                    path.write_text(json.dumps(descriptor))
                    with self.assertRaises(UpdateContractError):
                        verify_bundle(bundle, CERTIFICATE)

    def test_symlink_member_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = self.make_bundle(root)
            descriptor = json.loads((bundle / DESCRIPTOR_NAME).read_text())
            member = bundle / descriptor["artifacts"]["mplLicense"]["fileName"]
            outside = root / "outside.txt"
            outside.write_bytes(member.read_bytes())
            member.unlink()
            member.symlink_to(outside)
            with self.assertRaisesRegex(UpdateContractError, "regular non-symlink"):
                verify_bundle(bundle, CERTIFICATE)

    def test_windows_verifier_is_offline_authenticode_and_version_gated(self):
        self.assertIn("SignedCms", POWERSHELL)
        self.assertIn("CheckSignature($true)", POWERSHELL)
        self.assertIn("SignerInfos.Count -ne 1", POWERSHELL)
        self.assertIn("Get-AuthenticodeSignature", POWERSHELL)
        self.assertIn('Status -ne "Valid"', POWERSHELL)
        self.assertIn("TimeStamperCertificate", POWERSHELL)
        self.assertIn("1.3.6.1.5.5.7.3.3", POWERSHELL)
        self.assertIn("FileVersionInfo", POWERSHELL)
        self.assertIn("Authenticode-covered installer file version", POWERSHELL)
        self.assertIn("ExpectedSignerCertificateSha256", POWERSHELL)
        self.assertIn("candidate -le $current", POWERSHELL)
        self.assertIn("No network request", POWERSHELL)
        for forbidden in ("Invoke-WebRequest", "Invoke-RestMethod", "Start-BitsTransfer"):
            self.assertNotIn(forbidden, POWERSHELL)
        self.assertIn("stable staged NSIS", DOC)
        self.assertIn("does not auto-download", DOC)


if __name__ == "__main__":
    unittest.main()
