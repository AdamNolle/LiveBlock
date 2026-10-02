#!/usr/bin/env python3
"""Create or verify the closed, offline Windows staged-update descriptor.

Authenticode verification remains a Windows API operation performed by
verify_windows_application_update.ps1. This helper binds the exact local bundle
bytes and rejects schema/path drift without downloading anything.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
CHANNEL = "stable-staged-nsis"
PLATFORM = "windows-x86_64"
REPOSITORY = "AdamNolle/LiveBlock"
DESCRIPTOR_NAME = "windows-application-update.json"
SIGNATURE_NAME = "windows-application-update.p7s"
ARTIFACT_KEYS = (
    "installer",
    "sbom",
    "licenseReport",
    "obligationReport",
    "licenseDecisions",
    "mplSourceOffer",
    "mplLicense",
)
TOP_KEYS = {
    "schemaVersion",
    "channel",
    "platform",
    "version",
    "gitCommit",
    "source",
    "authenticode",
    "descriptorSignature",
    "artifacts",
}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")


class UpdateContractError(ValueError):
    pass


def _require_exact_keys(value: Any, expected: set[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise UpdateContractError(f"{name} must be an object")
    actual = set(value)
    if actual != expected:
        raise UpdateContractError(
            f"{name} fields must be exactly {sorted(expected)}; found {sorted(actual)}"
        )
    return value


def _require_string(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value:
        raise UpdateContractError(f"{name} must be a nonempty string")
    return value


def _require_sha256(value: Any, name: str) -> str:
    value = _require_string(value, name)
    if not SHA256_RE.fullmatch(value):
        raise UpdateContractError(f"{name} must be lowercase SHA-256")
    return value


def _validate_file_record(value: Any, name: str) -> dict[str, Any]:
    record = _require_exact_keys(value, {"fileName", "size", "sha256"}, name)
    file_name = _require_string(record["fileName"], f"{name}.fileName")
    if file_name in {".", ".."} or Path(file_name).name != file_name or "/" in file_name or "\\" in file_name:
        raise UpdateContractError(f"{name}.fileName must be an immediate basename")
    size = record["size"]
    if isinstance(size, bool) or not isinstance(size, int) or size < 1:
        raise UpdateContractError(f"{name}.size must be a positive integer")
    _require_sha256(record["sha256"], f"{name}.sha256")
    return record


def validate_descriptor(data: Any, *, expected_certificate_sha256: str | None = None) -> dict[str, Any]:
    root = _require_exact_keys(data, TOP_KEYS, "descriptor")
    if type(root["schemaVersion"]) is not int or root["schemaVersion"] != SCHEMA_VERSION:
        raise UpdateContractError(f"unsupported schemaVersion: {root['schemaVersion']!r}")
    if root["channel"] != CHANNEL or root["platform"] != PLATFORM:
        raise UpdateContractError("channel/platform drift")
    version = _require_string(root["version"], "version")
    if not VERSION_RE.fullmatch(version):
        raise UpdateContractError("version must be a stable three-part numeric version")
    commit = _require_string(root["gitCommit"], "gitCommit")
    if not COMMIT_RE.fullmatch(commit):
        raise UpdateContractError("gitCommit must be a lowercase full Git SHA")

    source = _require_exact_keys(root["source"], {"type", "repository", "tag"}, "source")
    if source != {
        "type": "github-release",
        "repository": REPOSITORY,
        "tag": f"desktop-v{version}",
    }:
        raise UpdateContractError("source must be the exact version-bound LiveBlock GitHub release")

    descriptor_signature = _require_exact_keys(
        root["descriptorSignature"], {"fileName", "format"}, "descriptorSignature"
    )
    if descriptor_signature != {
        "fileName": SIGNATURE_NAME,
        "format": "cms-detached-sha256",
    }:
        raise UpdateContractError("descriptorSignature contract drift")

    authenticode = _require_exact_keys(
        root["authenticode"],
        {"publisherSubject", "certificateSha256", "timestampCertificateSha256"},
        "authenticode",
    )
    _require_string(authenticode["publisherSubject"], "authenticode.publisherSubject")
    certificate = _require_sha256(
        authenticode["certificateSha256"], "authenticode.certificateSha256"
    )
    _require_sha256(
        authenticode["timestampCertificateSha256"],
        "authenticode.timestampCertificateSha256",
    )
    if expected_certificate_sha256 is not None:
        expected = _require_sha256(expected_certificate_sha256, "expected certificate SHA-256")
        if certificate != expected:
            raise UpdateContractError("descriptor signer certificate does not match the trusted expected certificate")

    artifacts = _require_exact_keys(root["artifacts"], set(ARTIFACT_KEYS), "artifacts")
    names: set[str] = set()
    for key in ARTIFACT_KEYS:
        record = _validate_file_record(artifacts[key], f"artifacts.{key}")
        if record["fileName"] in names:
            raise UpdateContractError("artifact fileName values must be unique")
        names.add(record["fileName"])
    if not artifacts["installer"]["fileName"].lower().endswith(".exe"):
        raise UpdateContractError("installer must be the NSIS .exe")
    return root


def _hash_regular_immediate_file(root: Path, file_name: str) -> tuple[int, str]:
    root_stat = os.lstat(root)
    if stat.S_ISLNK(root_stat.st_mode) or not stat.S_ISDIR(root_stat.st_mode):
        raise UpdateContractError("bundle root must be a non-symlink directory")
    path = root / file_name
    before_path = os.lstat(path)
    if stat.S_ISLNK(before_path.st_mode) or not stat.S_ISREG(before_path.st_mode):
        raise UpdateContractError(f"bundle member must be a regular non-symlink file: {file_name}")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path, flags)
    try:
        before_fd = os.fstat(fd)
        if not stat.S_ISREG(before_fd.st_mode):
            raise UpdateContractError(f"bundle member descriptor is not regular: {file_name}")
        digest = hashlib.sha256()
        size = 0
        while True:
            chunk = os.read(fd, 1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
            size += len(chunk)
        after_fd = os.fstat(fd)
    finally:
        os.close(fd)
    after_path = os.lstat(path)
    identities = {
        (before_path.st_dev, before_path.st_ino),
        (before_fd.st_dev, before_fd.st_ino),
        (after_fd.st_dev, after_fd.st_ino),
        (after_path.st_dev, after_path.st_ino),
    }
    if len(identities) != 1 or before_fd.st_size != after_fd.st_size or size != after_fd.st_size:
        raise UpdateContractError(f"bundle member changed while hashing: {file_name}")
    return size, digest.hexdigest()


def _read_regular_immediate_file(root: Path, file_name: str, maximum: int) -> bytes:
    root_stat = os.lstat(root)
    if stat.S_ISLNK(root_stat.st_mode) or not stat.S_ISDIR(root_stat.st_mode):
        raise UpdateContractError("bundle root must be a non-symlink directory")
    path = root / file_name
    before_path = os.lstat(path)
    if stat.S_ISLNK(before_path.st_mode) or not stat.S_ISREG(before_path.st_mode):
        raise UpdateContractError(f"bundle member must be a regular non-symlink file: {file_name}")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path, flags)
    try:
        before_fd = os.fstat(fd)
        if not stat.S_ISREG(before_fd.st_mode) or before_fd.st_size > maximum:
            raise UpdateContractError(f"bundle member is not regular or exceeds its size limit: {file_name}")
        chunks: list[bytes] = []
        size = 0
        while True:
            chunk = os.read(fd, min(64 * 1024, maximum + 1 - size))
            if not chunk:
                break
            chunks.append(chunk)
            size += len(chunk)
            if size > maximum:
                raise UpdateContractError(f"bundle member exceeds its size limit: {file_name}")
        after_fd = os.fstat(fd)
    finally:
        os.close(fd)
    after_path = os.lstat(path)
    identities = {
        (before_path.st_dev, before_path.st_ino),
        (before_fd.st_dev, before_fd.st_ino),
        (after_fd.st_dev, after_fd.st_ino),
        (after_path.st_dev, after_path.st_ino),
    }
    if len(identities) != 1 or before_fd.st_size != after_fd.st_size or size != after_fd.st_size:
        raise UpdateContractError(f"bundle member changed while reading: {file_name}")
    return b"".join(chunks)


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise UpdateContractError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def _record(path: Path) -> dict[str, Any]:
    size, digest = _hash_regular_immediate_file(path.parent, path.name)
    return {"fileName": path.name, "size": size, "sha256": digest}


def _bundle_root(value: str | Path) -> Path:
    candidate = Path(value).absolute()
    metadata = os.lstat(candidate)
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise UpdateContractError("bundle root must be a non-symlink directory")
    return candidate.resolve(strict=True)


def _bundle_member(bundle: Path, value: str, name: str) -> Path:
    candidate = Path(value).absolute()
    metadata = os.lstat(candidate)
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise UpdateContractError(f"{name} must be a regular non-symlink file")
    path = candidate.resolve(strict=True)
    if path.parent != bundle:
        raise UpdateContractError(f"{name} must be an immediate file in the bundle root")
    return path


def create_descriptor(args: argparse.Namespace) -> None:
    bundle = _bundle_root(args.bundle)
    output = bundle / DESCRIPTOR_NAME
    if output.exists() or output.is_symlink():
        raise UpdateContractError(f"descriptor already exists: {output}")
    artifacts: dict[str, Any] = {}
    for key in ARTIFACT_KEYS:
        path = _bundle_member(bundle, getattr(args, key), key)
        artifacts[key] = _record(path)
    descriptor = {
        "schemaVersion": SCHEMA_VERSION,
        "channel": CHANNEL,
        "platform": PLATFORM,
        "version": args.version,
        "gitCommit": args.commit,
        "source": {
            "type": "github-release",
            "repository": REPOSITORY,
            "tag": f"desktop-v{args.version}",
        },
        "descriptorSignature": {
            "fileName": SIGNATURE_NAME,
            "format": "cms-detached-sha256",
        },
        "authenticode": {
            "publisherSubject": args.publisher_subject,
            "certificateSha256": args.certificate_sha256,
            "timestampCertificateSha256": args.timestamp_certificate_sha256,
        },
        "artifacts": artifacts,
    }
    validate_descriptor(descriptor, expected_certificate_sha256=args.certificate_sha256)
    payload = (json.dumps(descriptor, indent=2, sort_keys=True) + "\n").encode()
    fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        written = 0
        while written < len(payload):
            count = os.write(fd, payload[written:])
            if count <= 0:
                raise OSError("short write while creating update descriptor")
            written += count
        os.fsync(fd)
    finally:
        os.close(fd)


def verify_bundle(bundle: Path, expected_certificate_sha256: str) -> dict[str, Any]:
    bundle = _bundle_root(bundle)
    descriptor_bytes = _read_regular_immediate_file(bundle, DESCRIPTOR_NAME, 1024 * 1024)
    data = json.loads(descriptor_bytes, object_pairs_hook=_reject_duplicate_keys)
    descriptor = validate_descriptor(
        data, expected_certificate_sha256=expected_certificate_sha256
    )
    signature_bytes = _read_regular_immediate_file(bundle, SIGNATURE_NAME, 1024 * 1024)
    if not signature_bytes:
        raise UpdateContractError("detached CMS signature must not be empty")
    expected_names = {DESCRIPTOR_NAME, SIGNATURE_NAME}
    for key in ARTIFACT_KEYS:
        record = descriptor["artifacts"][key]
        expected_names.add(record["fileName"])
        actual_size, actual_hash = _hash_regular_immediate_file(bundle, record["fileName"])
        if actual_size != record["size"] or actual_hash != record["sha256"]:
            raise UpdateContractError(f"bundle member does not match descriptor: {record['fileName']}")
    actual_names = {entry.name for entry in os.scandir(bundle)}
    if actual_names != expected_names:
        raise UpdateContractError(
            f"bundle contents must be exactly {sorted(expected_names)}; found {sorted(actual_names)}"
        )
    return descriptor


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    create = subparsers.add_parser("create")
    create.add_argument("--bundle", required=True)
    create.add_argument("--version", required=True)
    create.add_argument("--commit", required=True)
    create.add_argument("--publisher-subject", required=True)
    create.add_argument("--certificate-sha256", required=True)
    create.add_argument("--timestamp-certificate-sha256", required=True)
    for key in ARTIFACT_KEYS:
        create.add_argument(f"--{re.sub(r'(?<!^)(?=[A-Z])', '-', key).lower()}", dest=key, required=True)
    verify = subparsers.add_parser("verify")
    verify.add_argument("--bundle", required=True)
    verify.add_argument("--expected-certificate-sha256", required=True)
    return parser


def main() -> int:
    args = _parser().parse_args()
    try:
        if args.command == "create":
            create_descriptor(args)
        else:
            verify_bundle(Path(args.bundle), args.expected_certificate_sha256)
    except (OSError, UnicodeError, json.JSONDecodeError, UpdateContractError) as exc:
        print(f"Windows application update verification failed: {exc}")
        return 1
    print(f"Windows application update {args.command} passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
