#!/usr/bin/env python3
"""Stage a checksum-pinned official ONNX Runtime CPU archive for Linux packaging.

This is an operator/build-time helper. The LiveBlock application never downloads
runtimes, models, code, or trust roots.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import tarfile
import tempfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

VERSION = "1.18.1"
MAX_ARCHIVE_BYTES = 100 * 1024 * 1024
MAX_MEMBER_BYTES = 64 * 1024 * 1024


@dataclass(frozen=True)
class RuntimeSpec:
    architecture: str
    asset_architecture: str
    elf_machine: int
    archive_sha256: str

    @property
    def archive_name(self) -> str:
        return f"onnxruntime-linux-{self.asset_architecture}-{VERSION}.tgz"

    @property
    def url(self) -> str:
        return (
            "https://github.com/microsoft/onnxruntime/releases/download/"
            f"v{VERSION}/{self.archive_name}"
        )

    @property
    def prefix(self) -> str:
        return f"onnxruntime-linux-{self.asset_architecture}-{VERSION}"


SPECS = {
    "x86_64": RuntimeSpec(
        architecture="x86_64",
        asset_architecture="x64",
        elf_machine=62,
        archive_sha256="a0994512ec1e1debc00c18bfc7a5f16249f6ebd6a6128ff2034464cc380ea211",
    ),
    "aarch64": RuntimeSpec(
        architecture="aarch64",
        asset_architecture="aarch64",
        elf_machine=183,
        archive_sha256="c1dcd8ab29e8d227d886b6ee415c08aea893956acf98f0758a42a84f27c02851",
    ),
}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _open_archive_once(path: Path):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise RuntimeError("open regular non-symlink ONNX Runtime archive") from error
    metadata = os.fstat(descriptor)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_size <= 0 or metadata.st_size > MAX_ARCHIVE_BYTES:
        os.close(descriptor)
        raise RuntimeError("ONNX Runtime archive must be a bounded nonempty regular non-symlink file")
    return os.fdopen(descriptor, "rb")


def download(spec: RuntimeSpec, destination: Path) -> None:
    request = urllib.request.Request(
        spec.url,
        headers={"User-Agent": "LiveBlock-release-builder/1"},
    )
    total = 0
    with urllib.request.urlopen(request, timeout=60) as response, destination.open("xb") as output:
        final_url = response.geturl()
        if not final_url.startswith("https://"):
            raise RuntimeError("ONNX Runtime download redirected outside HTTPS")
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_ARCHIVE_BYTES:
                raise RuntimeError("ONNX Runtime archive exceeds the size limit")
            output.write(chunk)
        output.flush()
        os.fsync(output.fileno())


def _read_regular_member(archive: tarfile.TarFile, name: str) -> bytes:
    try:
        member = archive.getmember(name)
    except KeyError as error:
        raise RuntimeError(f"required archive member is missing: {name}") from error
    if not member.isfile() or member.size <= 0 or member.size > MAX_MEMBER_BYTES:
        raise RuntimeError(f"archive member must be a bounded nonempty regular file: {name}")
    handle = archive.extractfile(member)
    if handle is None:
        raise RuntimeError(f"could not read archive member: {name}")
    contents = handle.read(MAX_MEMBER_BYTES + 1)
    if len(contents) != member.size:
        raise RuntimeError(f"archive member size changed while reading: {name}")
    return contents


def _validate_elf(contents: bytes, spec: RuntimeSpec) -> None:
    if (
        len(contents) < 20
        or contents[:4] != b"\x7fELF"
        or contents[4] != 2
        or contents[5] != 1
        or int.from_bytes(contents[16:18], "little") != 3
        or int.from_bytes(contents[18:20], "little") != spec.elf_machine
    ):
        raise RuntimeError(
            f"ONNX Runtime library is not a target {spec.architecture} 64-bit little-endian ELF shared object"
        )


def _fsync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _write_new(path: Path, contents: bytes, mode: int) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    try:
        with os.fdopen(descriptor, "wb", closefd=False) as handle:
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
    finally:
        os.close(descriptor)
    os.chmod(path, mode)


def stage_archive(
    archive_path: Path,
    destination: Path,
    spec: RuntimeSpec,
    *,
    expected_sha256: str | None = None,
    replace: bool = False,
) -> dict[str, object]:
    archive_path = archive_path.absolute()
    expected = expected_sha256 or spec.archive_sha256
    versioned_library = f"{spec.prefix}/lib/libonnxruntime.so.{VERSION}"
    license_name = f"{spec.prefix}/LICENSE"
    notices_name = f"{spec.prefix}/ThirdPartyNotices.txt"
    version_name = f"{spec.prefix}/VERSION_NUMBER"
    # Hash and parse the same already-open descriptor. A pathname replacement
    # after verification cannot substitute different archive bytes.
    with _open_archive_once(archive_path) as archive_handle:
        digest = hashlib.sha256()
        for chunk in iter(lambda: archive_handle.read(1024 * 1024), b""):
            digest.update(chunk)
        actual_archive_hash = digest.hexdigest()
        if actual_archive_hash != expected:
            raise RuntimeError(
                f"ONNX Runtime archive SHA-256 mismatch: expected {expected}, got {actual_archive_hash}"
            )
        archive_handle.seek(0)
        with tarfile.open(fileobj=archive_handle, mode="r:gz") as archive:
            library = _read_regular_member(archive, versioned_library)
            license_text = _read_regular_member(archive, license_name)
            third_party = _read_regular_member(archive, notices_name)
            version_bytes = _read_regular_member(archive, version_name)

    if version_bytes.decode("utf-8", errors="strict").strip() != VERSION:
        raise RuntimeError("ONNX Runtime archive version does not match the pinned version")
    license_text.decode("utf-8", errors="strict")
    third_party.decode("utf-8", errors="strict")
    _validate_elf(library, spec)

    destination_parent = destination.parent.resolve(strict=True)
    destination = destination_parent / destination.name
    backup = destination_parent / f".{destination.name}.previous"
    if backup.exists() or backup.is_symlink():
        if not destination.exists() and backup.is_dir() and not backup.is_symlink():
            os.replace(backup, destination)
            _fsync_directory(destination_parent)
        else:
            raise RuntimeError("stale runtime replacement backup requires operator review")
    if destination.exists() or destination.is_symlink():
        if destination.is_symlink() or not destination.is_dir():
            raise RuntimeError("runtime destination must be a non-symlink directory")
        if not replace:
            raise RuntimeError("runtime destination already exists; pass --replace explicitly")

    staging = Path(tempfile.mkdtemp(prefix=f".{destination.name}.stage-", dir=destination_parent))
    try:
        notice_header = (
            f"LiveBlock packaged dependency notice\n"
            f"ONNX Runtime version: {VERSION}\n"
            f"Official archive: {spec.url}\n"
            f"Archive SHA-256: {actual_archive_hash}\n\n"
            "===== ONNX Runtime LICENSE =====\n"
        ).encode("utf-8")
        combined_notice = (
            notice_header
            + license_text
            + b"\n\n===== ONNX Runtime ThirdPartyNotices.txt =====\n"
            + third_party
        )
        files = {
            "libonnxruntime.so": (library, 0o755),
            "LICENSE.onnxruntime.txt": (license_text, 0o644),
            "ThirdPartyNotices.onnxruntime.txt": (third_party, 0o644),
            "THIRD-PARTY-NOTICES.txt": (combined_notice, 0o644),
            "VERSION_NUMBER": (version_bytes, 0o644),
        }
        hashes: dict[str, str] = {}
        for name, (contents, mode) in files.items():
            _write_new(staging / name, contents, mode)
            hashes[name] = hashlib.sha256(contents).hexdigest()
        manifest = {
            "schemaVersion": 1,
            "component": "onnxruntime",
            "version": VERSION,
            "architecture": spec.architecture,
            "sourceUrl": spec.url,
            "archiveSha256": actual_archive_hash,
            "files": dict(sorted(hashes.items())),
        }
        _write_new(
            staging / "STAGING-MANIFEST.json",
            (json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode("utf-8"),
            0o644,
        )
        directory_fd = os.open(staging, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)

        moved_previous = False
        try:
            if destination.exists():
                os.replace(destination, backup)
                moved_previous = True
                _fsync_directory(destination_parent)
            os.replace(staging, destination)
        except Exception:
            if moved_previous and not destination.exists() and backup.exists():
                os.replace(backup, destination)
                _fsync_directory(destination_parent)
            raise
        _fsync_directory(destination_parent)
        staging = None
        if moved_previous:
            shutil.rmtree(backup)
            _fsync_directory(destination_parent)
        return manifest
    finally:
        if staging is not None:
            shutil.rmtree(staging, ignore_errors=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--architecture", choices=sorted(SPECS), required=True)
    parser.add_argument("--archive", type=Path, help="Use an existing official archive")
    parser.add_argument(
        "--destination",
        type=Path,
        default=Path("platform/linux/src-tauri/resources/onnxruntime"),
    )
    parser.add_argument("--replace", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    spec = SPECS[args.architecture]
    temporary_archive: Path | None = None
    archive = args.archive
    try:
        if archive is None:
            descriptor, name = tempfile.mkstemp(prefix="liveblock-onnxruntime-", suffix=".tgz")
            os.close(descriptor)
            os.unlink(name)
            temporary_archive = Path(name)
            download(spec, temporary_archive)
            archive = temporary_archive
        manifest = stage_archive(
            archive,
            args.destination,
            spec,
            replace=args.replace,
        )
        print(json.dumps(manifest, sort_keys=True, indent=2))
        return 0
    finally:
        if temporary_archive is not None:
            temporary_archive.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
