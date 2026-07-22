import hashlib
import tempfile
import unittest
import zipfile
from pathlib import Path

from stage_windows_onnxruntime import (
    DLL_MEMBER,
    LICENSE_MEMBER,
    NOTICE_MEMBER,
    stage_archive,
)


def pe64() -> bytes:
    data = bytearray(512)
    data[:2] = b"MZ"
    data[0x3C:0x40] = (0x80).to_bytes(4, "little")
    data[0x80:0x84] = b"PE\0\0"
    data[0x84:0x86] = (0x8664).to_bytes(2, "little")
    data[0x98:0x9A] = (0x20B).to_bytes(2, "little")
    return bytes(data)


class WindowsRuntimeStagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.archive = self.root / "runtime.nupkg"
        with zipfile.ZipFile(self.archive, "w") as output:
            output.writestr(DLL_MEMBER, pe64())
            output.writestr(NOTICE_MEMBER, "third-party notices\n")
            output.writestr(LICENSE_MEMBER, "MIT License\n")
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.dll = self.root / "out" / "onnxruntime.dll"
        self.notices = self.root / "out" / "notices.txt"
        self.license = self.root / "out" / "license.txt"

    def tearDown(self):
        self.temp.cleanup()

    def test_stages_exact_regular_runtime_and_notices(self):
        stage_archive(self.archive, self.dll, self.notices, self.license, expected_sha256=self.digest)
        self.assertEqual(self.dll.read_bytes(), pe64())
        self.assertTrue(self.notices.read_text().startswith("third-party"))
        self.assertEqual(self.license.read_text(), "MIT License\n")

    def test_hash_mismatch_leaves_no_outputs(self):
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            stage_archive(self.archive, self.dll, self.notices, self.license, expected_sha256="0" * 64)
        self.assertFalse(self.dll.exists())

    def test_existing_destination_fails_closed(self):
        self.dll.parent.mkdir()
        self.dll.write_bytes(b"existing")
        with self.assertRaises(FileExistsError):
            stage_archive(self.archive, self.dll, self.notices, self.license, expected_sha256=self.digest)
        self.assertEqual(self.dll.read_bytes(), b"existing")

    def test_wrong_architecture_is_rejected(self):
        with zipfile.ZipFile(self.archive, "w") as output:
            bad = bytearray(pe64())
            bad[0x84:0x86] = (0x14C).to_bytes(2, "little")
            output.writestr(DLL_MEMBER, bad)
            output.writestr(NOTICE_MEMBER, "notices")
            output.writestr(LICENSE_MEMBER, "license")
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        with self.assertRaisesRegex(ValueError, "x86_64 PE32"):
            stage_archive(self.archive, self.dll, self.notices, self.license, expected_sha256=digest)


if __name__ == "__main__":
    unittest.main()
