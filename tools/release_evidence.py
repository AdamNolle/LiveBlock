#!/usr/bin/env python3
"""Create and verify deterministic release inventories and dependency SBOM evidence.

This tool does not sign packages or claim platform certification. It inventories an
already-staged artifact tree and emits build-input dependency evidence suitable for
CI preservation and later signed-package verification.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import sys
import uuid
from collections import Counter, defaultdict
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

INVENTORY_SCHEMA = 1
INVENTORY_HASH = "sha256-release-inventory-v1"
SBOM_SPEC = "1.5"
ALLOWED_LICENSE_IDS = {
    "0BSD", "Apache-2.0", "BSD-1-Clause", "BSD-2-Clause", "BSD-3-Clause",
    "BSL-1.0", "CC0-1.0", "CDLA-Permissive-2.0", "ISC", "MIT", "MIT-0", "NCSA", "OFL-1.1",
    "Unicode-3.0", "Unlicense", "Zlib",
}
REVIEW_LICENSE_PREFIXES = ("LGPL-", "MPL-")
RESTRICTED_LICENSE_PREFIXES = ("AGPL-", "GPL-", "SSPL-", "BUSL-")
ALLOWED_LICENSE_EXCEPTIONS = {"LLVM-exception"}


def _write_json_exclusive(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    except BaseException:
        path.unlink(missing_ok=True)
        raise


def _validate_relative_path(raw: str) -> str:
    path = PurePosixPath(raw)
    if not raw or path.is_absolute() or "\\" in raw or ".." in path.parts:
        raise ValueError(f"unsafe inventory path: {raw!r}")
    normalized = path.as_posix()
    if normalized in {"", "."} or normalized != raw:
        raise ValueError(f"non-canonical inventory path: {raw!r}")
    return normalized


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while block := handle.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def _regular_file_entry(path: Path, relative: str) -> dict[str, Any]:
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError(f"artifact tree contains non-regular file: {path}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        if (
            not stat.S_ISREG(opened.st_mode)
            or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino)
        ):
            raise ValueError(f"artifact file changed while opening: {path}")
        digest = hashlib.sha256()
        verification_digest = hashlib.sha256()
        with os.fdopen(descriptor, "rb", closefd=False) as handle:
            while block := handle.read(1024 * 1024):
                digest.update(block)
            handle.seek(0)
            while block := handle.read(1024 * 1024):
                verification_digest.update(block)
        descriptor_after = os.fstat(descriptor)
        path_after = path.lstat()
        if (
            (descriptor_after.st_dev, descriptor_after.st_ino, descriptor_after.st_mode,
             descriptor_after.st_size, descriptor_after.st_mtime_ns)
            != (opened.st_dev, opened.st_ino, opened.st_mode, opened.st_size, opened.st_mtime_ns)
            or digest.digest() != verification_digest.digest()
            or not stat.S_ISREG(path_after.st_mode)
            or (path_after.st_dev, path_after.st_ino) != (opened.st_dev, opened.st_ino)
        ):
            raise ValueError(f"artifact file changed while hashing: {path}")
        return {
            "path": relative,
            "type": "file",
            "mode": f"{stat.S_IMODE(opened.st_mode):04o}",
            "size": opened.st_size,
            "sha256": digest.hexdigest(),
        }
    finally:
        os.close(descriptor)


def _scan_regular_tree(root: Path) -> list[dict[str, Any]]:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("artifact root must be a non-symlink directory")
    entries: list[dict[str, Any]] = []
    for current, directories, files in os.walk(root, topdown=True, followlinks=False):
        current_path = Path(current)
        for name in sorted(directories):
            candidate = current_path / name
            mode = candidate.lstat().st_mode
            if stat.S_ISLNK(mode) or not stat.S_ISDIR(mode):
                raise ValueError(f"artifact tree contains unsafe directory: {candidate}")
            relative = _validate_relative_path(candidate.relative_to(root).as_posix())
            entries.append({
                "path": relative,
                "type": "directory",
                "mode": f"{stat.S_IMODE(mode):04o}",
            })
        directories.sort()
        for name in sorted(files):
            candidate = current_path / name
            relative = _validate_relative_path(candidate.relative_to(root).as_posix())
            entries.append(_regular_file_entry(candidate, relative))
    entries.sort(key=lambda item: item["path"])
    if not entries:
        raise ValueError("artifact tree is empty")
    return entries


def _aggregate_inventory(entries: Iterable[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    digest.update(b"LiveBlock release inventory v1\0")
    for entry in entries:
        path = entry["path"].encode("utf-8")
        digest.update(len(path).to_bytes(8, "big"))
        digest.update(path)
        digest.update(b"D" if entry["type"] == "directory" else b"F")
        digest.update(int(entry["mode"], 8).to_bytes(4, "big"))
        if entry["type"] == "file":
            file_hash = bytes.fromhex(entry["sha256"])
            digest.update(int(entry["size"]).to_bytes(8, "big"))
            digest.update(file_hash)
    return digest.hexdigest()


def create_inventory(
    root: Path,
    *,
    platform: str,
    artifact_type: str,
    commit: str,
    required_paths: Iterable[str] = (),
) -> dict[str, Any]:
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ValueError("commit must be a full lowercase 40-character Git SHA")
    if not platform or not artifact_type:
        raise ValueError("platform and artifact type must be nonempty")
    required = sorted({_validate_relative_path(path) for path in required_paths})
    entries = _scan_regular_tree(root)
    actual = {entry["path"] for entry in entries}
    missing = sorted(set(required) - actual)
    if missing:
        raise ValueError(f"required artifact paths are missing: {', '.join(missing)}")
    return {
        "schemaVersion": INVENTORY_SCHEMA,
        "hashAlgorithm": INVENTORY_HASH,
        "platform": platform,
        "artifactType": artifact_type,
        "commit": commit,
        "requiredPaths": required,
        "entries": entries,
        "aggregateSha256": _aggregate_inventory(entries),
    }


def _validate_inventory_document(document: dict[str, Any]) -> None:
    if not isinstance(document, dict):
        raise ValueError("inventory must be a JSON object")
    expected = {
        "schemaVersion", "hashAlgorithm", "platform", "artifactType", "commit",
        "requiredPaths", "entries", "aggregateSha256",
    }
    if set(document) != expected:
        raise ValueError("inventory has missing or unknown top-level fields")
    if document["schemaVersion"] != INVENTORY_SCHEMA or document["hashAlgorithm"] != INVENTORY_HASH:
        raise ValueError("unsupported inventory schema or hash algorithm")
    for field in ("platform", "artifactType"):
        if not isinstance(document[field], str) or not document[field]:
            raise ValueError(f"inventory {field} is invalid")
    commit = document["commit"]
    if not isinstance(commit, str) or len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ValueError("inventory commit is invalid")
    aggregate = document["aggregateSha256"]
    if not isinstance(aggregate, str) or len(aggregate) != 64:
        raise ValueError("inventory aggregate hash is invalid")
    try:
        bytes.fromhex(aggregate)
    except ValueError as error:
        raise ValueError("inventory aggregate hash is invalid") from error
    if not isinstance(document["requiredPaths"], list) or not isinstance(document["entries"], list):
        raise ValueError("inventory paths and entries must be arrays")
    paths = []
    for entry in document["entries"]:
        if not isinstance(entry, dict):
            raise ValueError("inventory entry must be an object")
        entry_type = entry.get("type")
        expected_entry = {"path", "type", "mode"}
        if entry_type == "file":
            expected_entry.update({"size", "sha256"})
        elif entry_type != "directory":
            raise ValueError("inventory entry type is invalid")
        if set(entry) != expected_entry:
            raise ValueError("inventory entry has invalid fields")
        paths.append(_validate_relative_path(entry["path"]))
        mode = entry["mode"]
        if not isinstance(mode, str) or len(mode) != 4 or any(character not in "01234567" for character in mode):
            raise ValueError("inventory entry mode is invalid")
        if entry_type == "file":
            if not isinstance(entry["size"], int) or isinstance(entry["size"], bool) or entry["size"] < 0:
                raise ValueError("inventory entry size is invalid")
            if not isinstance(entry["sha256"], str) or len(entry["sha256"]) != 64:
                raise ValueError("inventory entry hash is invalid")
            try:
                bytes.fromhex(entry["sha256"])
            except ValueError as error:
                raise ValueError("inventory entry hash is invalid") from error
    if paths != sorted(paths) or len(paths) != len(set(paths)) or not paths:
        raise ValueError("inventory entries must be nonempty, unique, and sorted")
    required = [_validate_relative_path(path) for path in document["requiredPaths"]]
    if required != sorted(set(required)):
        raise ValueError("required paths must be unique and sorted")
    if not set(required).issubset(paths):
        raise ValueError("required path is absent from inventory")
    if _aggregate_inventory(document["entries"]) != document["aggregateSha256"]:
        raise ValueError("inventory aggregate hash is invalid")


def verify_inventory(root: Path, document: dict[str, Any]) -> None:
    _validate_inventory_document(document)
    current = _scan_regular_tree(root)
    if current != document["entries"]:
        raise ValueError("artifact tree does not match inventory")
    if _aggregate_inventory(current) != document["aggregateSha256"]:
        raise ValueError("artifact aggregate fingerprint does not match")


def _cargo_graph_sha256(metadata: dict[str, Any]) -> str:
    """Hash the normalized component set without checkout-specific Cargo paths."""
    if not isinstance(metadata, dict) or not isinstance(metadata.get("packages"), list):
        raise ValueError("Cargo metadata must contain a packages array")
    if not all(isinstance(package, dict) for package in metadata["packages"]):
        raise ValueError("Cargo metadata packages must be objects")
    packages = sorted(
        {
            (
                str(package["name"]),
                str(package["version"]),
                str(package.get("source") or "workspace"),
                str(package.get("license") or "NOASSERTION"),
            )
            for package in metadata["packages"]
        }
    )
    payload = json.dumps(packages, separators=(",", ":"), ensure_ascii=True).encode()
    return hashlib.sha256(b"LiveBlock cargo dependency graph v1\0" + payload).hexdigest()


def _npm_name(path_key: str, package: dict[str, Any]) -> str:
    if package.get("name"):
        return str(package["name"])
    marker = "node_modules/"
    tail = path_key.rsplit(marker, 1)[-1]
    parts = tail.split("/")
    return "/".join(parts[:2]) if parts[0].startswith("@") else parts[0]


def _license_tokens(expression: str) -> list[str]:
    expression = expression.strip()
    tokens: list[str] = []
    position = 0
    pattern = re.compile(r"\s*(AND\b|OR\b|WITH\b|[()/]|[A-Za-z0-9][A-Za-z0-9.+-]*)")
    while position < len(expression):
        match = pattern.match(expression, position)
        if not match:
            raise ValueError(f"unsupported license expression syntax: {expression}")
        token = match.group(1)
        tokens.append("OR" if token == "/" else token)
        position = match.end()
    if not tokens:
        raise ValueError("empty license expression")
    return tokens


def _license_leaf(identifier: str) -> tuple[int, bool, bool, bool]:
    if identifier in ALLOWED_LICENSE_IDS:
        return 0, False, False, False
    if identifier.startswith(REVIEW_LICENSE_PREFIXES):
        return 1, True, False, False
    if identifier.startswith(RESTRICTED_LICENSE_PREFIXES):
        return 2, False, True, False
    return 2, False, False, True


def _parse_license_expression(expression: str) -> tuple[int, bool, bool, bool, list[str]]:
    """Return rank, review/restricted/unknown presence, and normalized tokens."""
    tokens = _license_tokens(expression)
    index = 0

    def factor() -> tuple[int, bool, bool, bool]:
        nonlocal index
        if index >= len(tokens):
            raise ValueError("incomplete license expression")
        if tokens[index] == "(":
            index += 1
            value = or_expression()
            if index >= len(tokens) or tokens[index] != ")":
                raise ValueError("unbalanced license expression")
            index += 1
            return value
        identifier = tokens[index]
        if identifier in {"AND", "OR", "WITH", ")"}:
            raise ValueError("invalid license expression operand")
        index += 1
        value = _license_leaf(identifier)
        if index < len(tokens) and tokens[index] == "WITH":
            index += 1
            if index >= len(tokens) or tokens[index] not in ALLOWED_LICENSE_EXCEPTIONS:
                raise ValueError("unknown or missing SPDX license exception")
            index += 1
        return value

    def and_expression() -> tuple[int, bool, bool, bool]:
        nonlocal index
        rank, needs_review, has_restricted, has_unknown = factor()
        while index < len(tokens) and tokens[index] == "AND":
            index += 1
            right_rank, right_review, right_restricted, right_unknown = factor()
            rank = max(rank, right_rank)
            needs_review = needs_review or right_review
            has_restricted = has_restricted or right_restricted
            has_unknown = has_unknown or right_unknown
        return rank, needs_review, has_restricted, has_unknown

    def or_expression() -> tuple[int, bool, bool, bool]:
        nonlocal index
        rank, needs_review, has_restricted, has_unknown = and_expression()
        while index < len(tokens) and tokens[index] == "OR":
            index += 1
            right_rank, right_review, right_restricted, right_unknown = and_expression()
            rank = min(rank, right_rank)
            needs_review = needs_review or right_review
            has_restricted = has_restricted or right_restricted
            has_unknown = has_unknown or right_unknown
        return rank, needs_review, has_restricted, has_unknown

    result, review, restricted, unknown = or_expression()
    if index != len(tokens):
        raise ValueError("unexpected license expression token")
    return result, review, restricted, unknown, tokens


def _license_choice(license_name: str) -> dict[str, Any]:
    try:
        _rank, _review, _restricted, unknown, tokens = _parse_license_expression(license_name)
    except ValueError:
        return {"license": {"name": license_name}}
    if unknown:
        return {"license": {"name": license_name}}
    normalized = " ".join(tokens).replace("( ", "(").replace(" )", ")")
    if len(tokens) == 1:
        return {"license": {"id": tokens[0]}}
    return {"expression": normalized}


def _component(component_type: str, name: str, version: str, license_name: str, purl: str, identity: str) -> dict[str, Any]:
    reference = hashlib.sha256(identity.encode()).hexdigest()
    return {
        "type": component_type,
        "bom-ref": f"urn:liveblock:dependency:{reference}",
        "name": name,
        "version": version,
        "licenses": [_license_choice(license_name)],
        "purl": purl,
    }


def _license_status(license_name: str, first_party: bool) -> tuple[str, str | None]:
    if first_party and license_name.strip() in {"", "NOASSERTION"}:
        return "allowed", None
    if not license_name.strip() or license_name.strip() == "NOASSERTION":
        return "denied", "third-party dependency has no declared license"
    try:
        rank, has_review_license, has_restricted, has_unknown, _tokens = _parse_license_expression(license_name)
    except ValueError as error:
        return "denied", f"unknown or malformed license expression: {error}"
    if has_unknown:
        return "denied", f"license expression contains an unknown SPDX identifier: {license_name}"
    if has_restricted:
        return "denied", f"license expression contains a restricted SPDX identifier: {license_name}"
    if rank == 2:
        return "denied", f"license expression has no approved distribution choice: {license_name}"
    if has_review_license:
        return "review", f"distribution obligations require review: {license_name}"
    return "allowed", None


def _load_license_decisions(path: Path | None) -> tuple[dict[str, dict[str, str]], str | None]:
    if path is None:
        return {}, None
    raw = path.read_bytes()
    document = json.loads(raw)
    if not isinstance(document, dict) or set(document) != {"schemaVersion", "decisions"}:
        raise ValueError("license decisions must contain only schemaVersion and decisions")
    if document["schemaVersion"] != 1 or not isinstance(document["decisions"], list):
        raise ValueError("license decisions must use schema 1 with a decisions array")
    decisions: dict[str, dict[str, str]] = {}
    required = {"purl", "declaredExpression", "selectedLicense", "rationale"}
    for item in document["decisions"]:
        if not isinstance(item, dict) or set(item) != required:
            raise ValueError("each license decision must contain exactly purl, declaredExpression, selectedLicense, and rationale")
        normalized = {key: str(item[key]).strip() for key in required}
        if any(not value for value in normalized.values()):
            raise ValueError("license decision values must be nonempty strings")
        purl = normalized["purl"]
        if not purl.startswith(("pkg:cargo/", "pkg:npm/")) or purl in decisions:
            raise ValueError(f"invalid or duplicate license-decision purl: {purl}")
        decisions[purl] = normalized
    return decisions, hashlib.sha256(raw).hexdigest()


def create_sbom(
    cargo_metadata_paths: Iterable[Path],
    npm_lock: Path,
    *,
    commit: str,
    license_decisions: Path | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ValueError("commit must be a full lowercase 40-character Git SHA")
    components: dict[tuple[str, str, str, str], dict[str, Any]] = {}
    contexts: defaultdict[tuple[str, str, str, str], set[str]] = defaultdict(set)
    declared_licenses: dict[tuple[str, str, str, str], str] = {}
    first_party_keys: set[tuple[str, str, str, str]] = set()
    input_hashes: dict[str, str] = {}
    decisions, decisions_sha256 = _load_license_decisions(license_decisions)
    used_decisions: set[str] = set()
    license_selections: list[dict[str, str]] = []
    if decisions_sha256 is not None:
        input_hashes["licenseDecisionsSha256"] = decisions_sha256

    for metadata_path in cargo_metadata_paths:
        metadata = json.loads(metadata_path.read_text())
        if not isinstance(metadata, dict) or not isinstance(metadata.get("packages"), list):
            raise ValueError(f"Cargo metadata must contain a packages array: {metadata_path}")
        context = metadata_path.stem
        input_hashes[f"cargo:{context}:componentSetSha256"] = _cargo_graph_sha256(metadata)
        for package in metadata["packages"]:
            if not isinstance(package, dict):
                raise ValueError(f"Cargo metadata package must be an object: {metadata_path}")
            name, version = str(package["name"]), str(package["version"])
            first_party = package.get("source") is None and name.startswith("liveblock")
            license_name = str(package.get("license") or ("MIT" if first_party else "NOASSERTION"))
            source = str(package.get("source") or f"workspace:{name}")
            key = ("cargo", name, version, source)
            identity = "\0".join(key)
            existing_license = declared_licenses.get(key)
            if existing_license is not None and existing_license != license_name:
                raise ValueError(f"conflicting licenses for dependency identity {identity}")
            declared_licenses[key] = license_name
            components.setdefault(key, _component(
                "library", name, version, license_name,
                f"pkg:cargo/{name}@{version}", identity,
            ))
            contexts[key].add(context)
            if first_party:
                first_party_keys.add(key)

    lock = json.loads(npm_lock.read_text())
    if not isinstance(lock, dict) or not isinstance(lock.get("packages"), dict):
        raise ValueError("npm lockfile must contain a packages object")
    input_hashes["npm:shared-frontend:lockfileSha256"] = _sha256_file(npm_lock)
    for path_key, package in lock["packages"].items():
        if not path_key or "node_modules/" not in path_key:
            continue
        if not isinstance(package, dict):
            raise ValueError(f"npm lockfile package must be an object: {path_key}")
        name, version = _npm_name(path_key, package), str(package.get("version", ""))
        if not version:
            raise ValueError(f"npm dependency {name} has no locked version")
        license_name = str(package.get("license") or "NOASSERTION")
        source = str(package.get("resolved") or package.get("integrity") or f"lockpath:{path_key}")
        key = ("npm", name, version, source)
        identity = "\0".join(key)
        existing_license = declared_licenses.get(key)
        if existing_license is not None and existing_license != license_name:
            raise ValueError(f"conflicting licenses for dependency identity {identity}")
        declared_licenses[key] = license_name
        component = components.setdefault(key, _component(
            "library", name, version, license_name,
            f"pkg:npm/{name.replace('@', '%40')}@{version}", identity,
        ))
        integrity = package.get("integrity")
        if integrity and not any(
            item.get("name") == "liveblock:npmIntegrity"
            for item in component.get("properties", [])
        ):
            component.setdefault("properties", []).append({
                "name": "liveblock:npmIntegrity", "value": str(integrity),
            })
        contexts[key].add("shared-frontend-build")

    ordered = []
    denied, review = [], []
    license_counts: Counter[str] = Counter()
    for key in sorted(components, key=lambda item: (item[1], item[2], item[0], item[3])):
        component = components[key]
        component.setdefault("properties", []).append({
            "name": "liveblock:dependencyContexts",
            "value": ",".join(sorted(contexts[key])),
        })
        component["properties"].sort(key=lambda item: item["name"])
        license_name = declared_licenses[key]
        license_counts[license_name] += 1
        purl = component["purl"]
        decision = decisions.get(purl)
        effective_license = license_name
        if decision is not None:
            if decision["declaredExpression"] != license_name:
                raise ValueError(f"license decision expression does not match metadata for {purl}")
            _rank, _review, restricted, unknown, tokens = _parse_license_expression(license_name)
            selected = decision["selectedLicense"]
            if "OR" not in tokens or selected not in tokens:
                raise ValueError(f"license decision must select a declared OR branch for {purl}")
            if selected not in ALLOWED_LICENSE_IDS:
                raise ValueError(f"license decision must select an approved permissive SPDX identifier for {purl}")
            if restricted or unknown:
                raise ValueError(f"license decisions cannot override restricted or unknown terms for {purl}")
            effective_license = selected
            used_decisions.add(purl)
            component.setdefault("properties", []).append({
                "name": "liveblock:selectedLicense", "value": selected,
            })
            license_selections.append({
                "purl": purl,
                "declaredExpression": license_name,
                "selectedLicense": selected,
                "rationale": decision["rationale"],
            })
        status, reason = _license_status(effective_license, key in first_party_keys)
        finding = {
            "name": component["name"], "version": component["version"],
            "purl": purl, "license": license_name, "reason": reason,
        }
        if status == "denied":
            denied.append(finding)
        elif status == "review":
            review.append(finding)
        component["properties"].sort(key=lambda item: item["name"])
        ordered.append(component)

    unused_decisions = sorted(set(decisions) - used_decisions)
    if unused_decisions:
        raise ValueError(f"license decisions did not match dependency components: {', '.join(unused_decisions)}")
    license_selections.sort(key=lambda item: item["purl"])

    identity_payload = json.dumps({"commit": commit, "components": ordered}, sort_keys=True).encode()
    serial = uuid.uuid5(uuid.NAMESPACE_URL, hashlib.sha256(identity_payload).hexdigest())
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": SBOM_SPEC,
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": "LiveBlock", "version": "0.1.0"},
            "properties": [
                {"name": "liveblock:commit", "value": commit},
                {"name": "liveblock:evidenceBoundary", "value": "build-input dependencies; not signing or hardware certification"},
            ],
        },
        "components": ordered,
    }
    report = {
        "schemaVersion": 1,
        "commit": commit,
        "passed": not denied,
        "failures": denied,
        "reviewRequired": review,
        "licenseSelections": license_selections,
        "componentCount": len(ordered),
        "licenseCounts": dict(sorted(license_counts.items())),
        "inputFingerprints": dict(sorted(input_hashes.items())),
        "excluded": [
            "tools/requirements.txt is a source-training environment and is forbidden from production packages",
            "operating-system shared libraries and drivers are platform prerequisites, not bundled components",
            "Apple system frameworks are provided by macOS and no Swift Package Manager dependencies are used",
        ],
    }
    return sbom, report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    inventory_parser = subparsers.add_parser("inventory")
    inventory_parser.add_argument("--root", type=Path, required=True)
    inventory_parser.add_argument("--output", type=Path, required=True)
    inventory_parser.add_argument("--platform", required=True)
    inventory_parser.add_argument("--artifact-type", required=True)
    inventory_parser.add_argument("--commit", required=True)
    inventory_parser.add_argument("--require", action="append", default=[])

    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--root", type=Path, required=True)
    verify_parser.add_argument("--manifest", type=Path, required=True)

    sbom_parser = subparsers.add_parser("sbom")
    sbom_parser.add_argument("--cargo-metadata", type=Path, action="append", required=True)
    sbom_parser.add_argument("--npm-lock", type=Path, required=True)
    sbom_parser.add_argument("--output", type=Path, required=True)
    sbom_parser.add_argument("--license-report", type=Path, required=True)
    sbom_parser.add_argument("--license-decisions", type=Path)
    sbom_parser.add_argument("--commit", required=True)

    arguments = parser.parse_args()
    try:
        if arguments.command == "inventory":
            document = create_inventory(
                arguments.root,
                platform=arguments.platform,
                artifact_type=arguments.artifact_type,
                commit=arguments.commit,
                required_paths=arguments.require,
            )
            _write_json_exclusive(arguments.output, document)
        elif arguments.command == "verify":
            verify_inventory(arguments.root, json.loads(arguments.manifest.read_text()))
        else:
            sbom, report = create_sbom(
                arguments.cargo_metadata,
                arguments.npm_lock,
                commit=arguments.commit,
                license_decisions=arguments.license_decisions,
            )
            _write_json_exclusive(arguments.output, sbom)
            _write_json_exclusive(arguments.license_report, report)
            if not report["passed"]:
                print("release evidence failed: dependency license policy failed", file=sys.stderr)
                return 1
    except (OSError, ValueError, KeyError, TypeError, AttributeError, json.JSONDecodeError) as error:
        print(f"release evidence failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
