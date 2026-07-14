#!/usr/bin/env python3
"""Stage the pinned official ONNX Runtime DirectML DLL for Windows packaging."""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import stat
import struct
import tempfile
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

VERSION = "1.18.1"
URL = f"https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.DirectML/{VERSION}"
ARCHIVE_SHA256 = "51273348e0edc53d50a68fdddf29f142cdb9eca5c688d0233334c3295ebae595"
DLL_MEMBER = "runtimes/win-x64/native/onnxruntime.dll"
NOTICE_MEMBER = "ThirdPartyNotices.txt"
LICENSE_MEMBER = "LICENSE"
ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "platform/windows/src-tauri/resources/onnxruntime.dll"
DEFAULT_NOTICES = ROOT / "platform/windows/src-tauri/resources/onnxruntime-THIRD-PARTY-NOTICES.txt"
DEFAULT_LICENSE = ROOT / "platform/windows/src-tauri/resources/onnxruntime-LICENSE.txt"


def validate_pe64_x86(data: bytes) -> None:
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("onnxruntime.dll is not a PE file")
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    if pe_offset + 26 > len(data) or data[pe_offset:pe_offset + 4] != b"PE\0\0":
        raise ValueError("onnxruntime.dll has an invalid PE header")
    machine = struct.unpack_from("<H", data, pe_offset + 4)[0]
    optional_magic = struct.unpack_from("<H", data, pe_offset + 24)[0]
    if machine != 0x8664 or optional_magic != 0x20B:
        raise ValueError("onnxruntime.dll must be x86_64 PE32+")


def _validated_member(archive: zipfile.ZipFile, name: str) -> bytes:
    matches = [item for item in archive.infolist() if item.filename == name]
    if len(matches) != 1:
        raise ValueError(f"archive must contain exactly one {name}")
    item = matches[0]
    mode = (item.external_attr >> 16) & 0o170000
    if mode == 0o120000 or item.is_dir():
        raise ValueError(f"archive member is not a regular file: {name}")
    data = archive.read(item)
    if not data:
        raise ValueError(f"archive member is empty: {name}")
    return data


def stage_archive(
    archive_path: Path,
    output: Path,
    notices_output: Path,
    license_output: Path,
    *,
    expected_sha256: str = ARCHIVE_SHA256,
) -> None:
    before = archive_path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("ONNX Runtime DirectML archive must be a regular file")
    descriptor = os.open(archive_path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
            raise ValueError("ONNX Runtime DirectML archive changed while opening")
        digest = hashlib.sha256()
        with os.fdopen(descriptor, "rb", closefd=False) as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
            if digest.hexdigest() != expected_sha256:
                raise ValueError("ONNX Runtime DirectML archive SHA-256 mismatch")
            handle.seek(0)
            with zipfile.ZipFile(handle) as archive:
                for item in archive.infolist():
                    parts = PurePosixPath(item.filename).parts
                    if item.filename.startswith("/") or ".." in parts:
                        raise ValueError("archive contains an unsafe member path")
                dll = _validated_member(archive, DLL_MEMBER)
                notices = _validated_member(archive, NOTICE_MEMBER)
                license_text = _validated_member(archive, LICENSE_MEMBER)
        descriptor_after = os.fstat(descriptor)
        path_after = archive_path.lstat()
        if (
            (descriptor_after.st_dev, descriptor_after.st_ino, descriptor_after.st_mode,
             descriptor_after.st_size, descriptor_after.st_mtime_ns)
            != (opened.st_dev, opened.st_ino, opened.st_mode, opened.st_size, opened.st_mtime_ns)
            or not stat.S_ISREG(path_after.st_mode)
            or (path_after.st_dev, path_after.st_ino) != (opened.st_dev, opened.st_ino)
        ):
            raise ValueError("ONNX Runtime DirectML archive changed while parsing")
    finally:
        os.close(descriptor)
    validate_pe64_x86(dll)
    destinations = ((output, dll), (notices_output, notices), (license_output, license_text))
    if any(path.exists() or path.is_symlink() for path, _ in destinations):
        raise FileExistsError("Windows runtime staging destinations must not already exist")
    created: list[Path] = []
    try:
        for path, data in destinations:
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as handle:
                handle.write(data)
                handle.flush()
                os.fsync(handle.fileno())
            created.append(path)
    except Exception:
        for path in created:
            path.unlink(missing_ok=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--notices-output", type=Path, default=DEFAULT_NOTICES)
    parser.add_argument("--license-output", type=Path, default=DEFAULT_LICENSE)
    args = parser.parse_args()
    temporary: Path | None = None
    archive = args.archive
    try:
        if archive is None:
            with tempfile.NamedTemporaryFile(prefix="liveblock-ort-dml-", suffix=".nupkg", delete=False) as handle:
                temporary = Path(handle.name)
            with urllib.request.urlopen(URL) as response, temporary.open("wb") as output:
                shutil.copyfileobj(response, output)
            archive = temporary
        stage_archive(archive.resolve(), args.output.resolve(), args.notices_output.resolve(), args.license_output.resolve())
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(f"staged ONNX Runtime DirectML {VERSION}: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
