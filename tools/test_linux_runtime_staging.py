from __future__ import annotations

import hashlib
import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import stage_linux_onnxruntime as staging


def fake_elf(machine: int) -> bytes:
    header = bytearray(64)
    header[:4] = b"\x7fELF"
    header[4] = 2
    header[5] = 1
    header[16:18] = (3).to_bytes(2, "little")
    header[18:20] = machine.to_bytes(2, "little")
    return bytes(header) + b"runtime"


def add_file(archive: tarfile.TarFile, name: str, contents: bytes, *, symlink: bool = False) -> None:
    member = tarfile.TarInfo(name)
    if symlink:
        member.type = tarfile.SYMTYPE
        member.linkname = "elsewhere"
        member.size = 0
        archive.addfile(member)
        return
    member.size = len(contents)
    member.mode = 0o644
    archive.addfile(member, io.BytesIO(contents))


class LinuxRuntimeStagingTests(unittest.TestCase):
    def make_archive(self, root: Path, spec: staging.RuntimeSpec, *, library_symlink=False, machine=None, name="runtime.tgz") -> Path:
        path = root / name
        prefix = spec.prefix
        with tarfile.open(path, "w:gz") as archive:
            add_file(
                archive,
                f"{prefix}/lib/libonnxruntime.so.{staging.VERSION}",
                fake_elf(spec.elf_machine if machine is None else machine),
                symlink=library_symlink,
            )
            add_file(archive, f"{prefix}/LICENSE", b"MIT license\n")
            add_file(archive, f"{prefix}/ThirdPartyNotices.txt", b"Dependency notices\n")
            add_file(archive, f"{prefix}/VERSION_NUMBER", f"{staging.VERSION}\n".encode())
        return path

    def stage(self, archive: Path, destination: Path, spec: staging.RuntimeSpec, *, replace=False):
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        return staging.stage_archive(
            archive,
            destination,
            spec,
            expected_sha256=digest,
            replace=replace,
        )

    def test_stages_regular_runtime_and_complete_notice_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            archive = self.make_archive(root, spec)
            destination = root / "onnxruntime"
            manifest = self.stage(archive, destination, spec)

            self.assertEqual(manifest["architecture"], "x86_64")
            self.assertTrue((destination / "libonnxruntime.so").is_file())
            self.assertFalse((destination / "libonnxruntime.so").is_symlink())
            self.assertEqual((destination / "libonnxruntime.so").stat().st_mode & 0o777, 0o755)
            notice = (destination / "THIRD-PARTY-NOTICES.txt").read_text()
            self.assertIn("MIT license", notice)
            self.assertIn("Dependency notices", notice)
            recorded = json.loads((destination / "STAGING-MANIFEST.json").read_text())
            self.assertEqual(recorded, manifest)

    def test_rejects_archive_hash_mismatch_without_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            archive = self.make_archive(root, spec)
            destination = root / "onnxruntime"
            with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                staging.stage_archive(archive, destination, spec, expected_sha256="0" * 64)
            self.assertFalse(destination.exists())

    def test_rejects_symlink_library_and_wrong_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            symlink_archive = self.make_archive(root, spec, library_symlink=True)
            with self.assertRaisesRegex(RuntimeError, "regular file"):
                self.stage(symlink_archive, root / "symlink-output", spec)

            wrong_archive = self.make_archive(root, spec, machine=183)
            with self.assertRaisesRegex(RuntimeError, "target x86_64"):
                self.stage(wrong_archive, root / "wrong-output", spec)

    def test_archive_path_swap_cannot_replace_verified_descriptor(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            archive = self.make_archive(root, spec, name="verified.tgz")
            expected_hash = hashlib.sha256(archive.read_bytes()).hexdigest()
            attacker = self.make_archive(root, spec, machine=183, name="attacker.tgz")
            real_tar_open = tarfile.open

            def swap_path_after_hash(*args, **kwargs):
                os.replace(attacker, archive)
                return real_tar_open(*args, **kwargs)

            with mock.patch.object(tarfile, "open", side_effect=swap_path_after_hash):
                manifest = staging.stage_archive(
                    archive,
                    root / "output",
                    spec,
                    expected_sha256=expected_hash,
                )
            self.assertEqual(manifest["archiveSha256"], expected_hash)
            self.assertEqual(
                int.from_bytes((root / "output/libonnxruntime.so").read_bytes()[18:20], "little"),
                62,
            )

    def test_failed_replacement_rolls_back_previous_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            archive = self.make_archive(root, spec)
            destination = root / "onnxruntime"
            self.stage(archive, destination, spec)
            before = (destination / "STAGING-MANIFEST.json").read_bytes()
            real_replace = os.replace

            def fail_staging_install(source, target):
                if ".stage-" in Path(source).name and Path(target).name == destination.name:
                    raise OSError("injected install failure")
                return real_replace(source, target)

            with mock.patch.object(os, "replace", side_effect=fail_staging_install):
                with self.assertRaisesRegex(OSError, "injected"):
                    self.stage(archive, destination, spec, replace=True)
            self.assertEqual((destination / "STAGING-MANIFEST.json").read_bytes(), before)
            self.assertFalse((root / ".onnxruntime.previous").exists())

    def test_existing_destination_requires_explicit_replace_and_rejects_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = staging.SPECS["x86_64"]
            archive = self.make_archive(root, spec)
            destination = root / "onnxruntime"
            self.stage(archive, destination, spec)
            with self.assertRaisesRegex(RuntimeError, "--replace"):
                self.stage(archive, destination, spec)
            self.stage(archive, destination, spec, replace=True)

            os.symlink(destination, root / "runtime-link")
            with self.assertRaisesRegex(RuntimeError, "non-symlink"):
                self.stage(archive, root / "runtime-link", spec, replace=True)

            os.symlink(archive, root / "archive-link.tgz")
            with self.assertRaisesRegex(RuntimeError, "non-symlink ONNX Runtime archive"):
                self.stage(root / "archive-link.tgz", root / "linked-archive-output", spec)


if __name__ == "__main__":
    unittest.main()
