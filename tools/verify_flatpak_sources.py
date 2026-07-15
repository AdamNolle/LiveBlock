#!/usr/bin/env python3
"""Verify committed Flatpak Cargo/npm sources against their lockfiles."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import tomllib
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
FLATPAK_DIR = ROOT / "platform/linux/flatpak"
METADATA_PATH = FLATPAK_DIR / "generated-sources.lock.json"
MANIFEST_PATH = FLATPAK_DIR / "com.adamnolle.LiveBlock.json"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _closed_object(value: Any, expected: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"{context} has missing or unknown fields")
    return value


def _resolve_input(relative: str) -> Path:
    if not isinstance(relative, str) or not relative:
        raise ValueError("generated-source input path is invalid")
    path = (FLATPAK_DIR / relative).resolve()
    try:
        path.relative_to(ROOT)
    except ValueError as error:
        raise ValueError("generated-source input escapes the repository") from error
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"generated-source input is not a regular file: {relative}")
    return path


def _validate_metadata() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    metadata = _closed_object(
        json.loads(METADATA_PATH.read_text()),
        {"schemaVersion", "generator", "cargo", "node"},
        "generated-source metadata",
    )
    if metadata["schemaVersion"] != 1:
        raise ValueError("unsupported generated-source metadata schema")
    generator = _closed_object(
        metadata["generator"], {"repository", "commit"}, "generator metadata"
    )
    if generator["repository"] != "https://github.com/flatpak/flatpak-builder-tools.git":
        raise ValueError("unexpected Flatpak generator repository")
    commit = generator["commit"]
    if not isinstance(commit, str) or len(commit) != 40 or any(
        character not in "0123456789abcdef" for character in commit
    ):
        raise ValueError("Flatpak generator commit is invalid")

    sections = []
    for name in ("cargo", "node"):
        section = _closed_object(
            metadata[name],
            {"input", "inputSha256", "output", "outputSha256", "command"},
            f"{name} generated-source metadata",
        )
        input_path = _resolve_input(section["input"])
        output = section["output"]
        if not isinstance(output, str) or Path(output).name != output:
            raise ValueError(f"{name} generated-source output path is invalid")
        output_path = FLATPAK_DIR / output
        if not output_path.is_file() or output_path.is_symlink():
            raise ValueError(f"{name} generated-source output is not a regular file")
        for field, actual in (
            ("inputSha256", _sha256(input_path)),
            ("outputSha256", _sha256(output_path)),
        ):
            expected = section[field]
            if not isinstance(expected, str) or expected != actual:
                raise ValueError(f"{name} {field} does not match committed bytes")
        if not isinstance(section["command"], str) or not section["command"]:
            raise ValueError(f"{name} generator command is invalid")
        sections.append((section, input_path, output_path))
    return metadata, sections[0], sections[1]


def _validate_cargo(lock_path: Path, sources_path: Path) -> None:
    lock = tomllib.loads(lock_path.read_text())
    packages = lock.get("package")
    if not isinstance(packages, list):
        raise ValueError("Cargo lockfile packages are invalid")
    expected: dict[tuple[str, str], str] = {}
    for package in packages:
        source = package.get("source", "")
        if not source:
            continue
        if not str(source).startswith("registry+"):
            raise ValueError(f"unsupported non-registry Cargo source: {source}")
        name, version, checksum = (
            package.get("name"),
            package.get("version"),
            package.get("checksum"),
        )
        if not all(isinstance(value, str) and value for value in (name, version, checksum)):
            raise ValueError("registry Cargo package lacks name/version/checksum")
        key = (name, version)
        if key in expected and expected[key] != checksum:
            raise ValueError(f"conflicting Cargo checksum for {name} {version}")
        expected[key] = checksum

    sources = json.loads(sources_path.read_text())
    if not isinstance(sources, list) or not sources:
        raise ValueError("Cargo Flatpak sources must be a nonempty array")
    archives: dict[tuple[str, str], dict[str, Any]] = {}
    checksums: dict[tuple[str, str], dict[str, Any]] = {}
    config_count = 0
    for source in sources:
        if not isinstance(source, dict):
            raise ValueError("Cargo Flatpak source must be an object")
        if source.get("type") == "archive":
            destination = source.get("dest", "")
            prefix = "cargo/vendor/"
            if not isinstance(destination, str) or not destination.startswith(prefix):
                raise ValueError("Cargo archive destination is invalid")
            suffix = destination[len(prefix) :]
            matches = [key for key in expected if suffix == f"{key[0]}-{key[1]}"]
            if len(matches) != 1 or matches[0] in archives:
                raise ValueError(f"unexpected or duplicate Cargo archive destination: {destination}")
            archives[matches[0]] = source
        elif source.get("type") == "inline" and source.get("dest-filename") == ".cargo-checksum.json":
            destination = source.get("dest", "")
            prefix = "cargo/vendor/"
            suffix = destination[len(prefix) :] if isinstance(destination, str) and destination.startswith(prefix) else ""
            matches = [key for key in expected if suffix == f"{key[0]}-{key[1]}"]
            if len(matches) != 1 or matches[0] in checksums:
                raise ValueError(f"unexpected or duplicate Cargo checksum destination: {destination}")
            checksums[matches[0]] = source
        elif source.get("type") == "inline" and source.get("dest") == "cargo" and source.get("dest-filename") == "config":
            config_count += 1
            contents = source.get("contents", "")
            if 'directory = "cargo/vendor"' not in contents or 'replace-with = "vendored-sources"' not in contents:
                raise ValueError("Cargo vendor config is invalid")
        else:
            raise ValueError("unexpected Cargo generated source entry")

    if set(archives) != set(expected) or set(checksums) != set(expected) or config_count != 1:
        raise ValueError("Cargo generated sources do not cover the lockfile exactly")
    for (name, version), checksum in expected.items():
        archive = archives[(name, version)]
        expected_url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
        if archive != {
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": expected_url,
            "sha256": checksum,
            "dest": f"cargo/vendor/{name}-{version}",
        }:
            raise ValueError(f"Cargo archive metadata drift for {name} {version}")
        checksum_document = json.loads(checksums[(name, version)].get("contents", ""))
        if checksum_document != {"package": checksum, "files": {}}:
            raise ValueError(f"Cargo checksum metadata drift for {name} {version}")


def _integrity(value: str) -> tuple[str, str]:
    if not isinstance(value, str) or "-" not in value:
        raise ValueError("npm integrity is invalid")
    algorithm, encoded = value.split("-", 1)
    if algorithm not in {"sha512", "sha256", "sha1"}:
        raise ValueError(f"unsupported npm integrity algorithm: {algorithm}")
    try:
        digest = base64.b64decode(encoded, validate=True).hex()
    except (ValueError, TypeError) as error:
        raise ValueError("npm integrity is invalid") from error
    return algorithm, digest


def _validate_node(lock_path: Path, sources_path: Path) -> None:
    lock = json.loads(lock_path.read_text())
    if lock.get("lockfileVersion") != 3 or not isinstance(lock.get("packages"), dict):
        raise ValueError("npm lockfile version/packages are invalid")
    expected: dict[str, tuple[str, str]] = {}
    for path, package in lock["packages"].items():
        if not path:
            continue
        resolved, integrity = package.get("resolved"), package.get("integrity")
        if not isinstance(resolved, str) or not isinstance(integrity, str):
            raise ValueError(f"npm package lacks resolved/integrity: {path}")
        parsed = urlparse(resolved)
        if parsed.scheme != "https" or parsed.netloc != "registry.npmjs.org":
            raise ValueError(f"npm package URL is not the pinned registry: {resolved}")
        if resolved in expected and expected[resolved] != _integrity(integrity):
            raise ValueError(f"conflicting npm integrity for {resolved}")
        expected[resolved] = _integrity(integrity)

    sources = json.loads(sources_path.read_text())
    if not isinstance(sources, list) or not sources:
        raise ValueError("Node Flatpak sources must be a nonempty array")
    files_by_url: dict[str, list[dict[str, Any]]] = {}
    shell_count = 0
    for source in sources:
        if not isinstance(source, dict):
            raise ValueError("Node Flatpak source must be an object")
        source_type = source.get("type")
        if source_type in {"file", "archive"}:
            url = source.get("url")
            parsed = urlparse(url) if isinstance(url, str) else None
            if parsed is None or parsed.scheme != "https":
                raise ValueError("Node generated download source must use HTTPS")
            hash_fields = [name for name in ("sha512", "sha256", "sha1") if name in source]
            if len(hash_fields) != 1:
                raise ValueError("Node generated download source must have exactly one supported hash")
            if source_type == "file":
                files_by_url.setdefault(url, []).append(source)
        elif source_type == "inline":
            if not isinstance(source.get("contents"), str):
                raise ValueError("Node inline source contents are invalid")
        elif source_type in {"shell", "script"}:
            if source_type == "shell":
                shell_count += 1
            commands = source.get("commands")
            if not isinstance(commands, list) or not commands or not all(isinstance(command, str) for command in commands):
                raise ValueError("Node generated command source is invalid")
            if any("curl " in command or "wget " in command for command in commands):
                raise ValueError("Node generated command source contains a network command")
        else:
            raise ValueError("unexpected Node generated source entry")

    for url, (algorithm, digest) in expected.items():
        matches = [source for source in files_by_url.get(url, []) if source.get(algorithm) == digest]
        if len(matches) != 1:
            raise ValueError(f"Node generated sources do not uniquely cover {url}")
    if shell_count == 0:
        raise ValueError("Node generated sources lack architecture-selecting shell entries")


def _validate_manifest(cargo_output: str, node_output: str) -> None:
    manifest = json.loads(MANIFEST_PATH.read_text())
    module = next(
        candidate
        for candidate in manifest["modules"]
        if isinstance(candidate, dict) and candidate.get("name") == "liveblock-linux"
    )
    sources = module["sources"]
    if cargo_output not in sources or node_output not in sources:
        raise ValueError("Flatpak manifest does not include committed generated sources")
    environment = manifest["build-options"]["env"]
    expected_environment = {
        "LIBCLANG_PATH": "/usr/lib/sdk/llvm18/lib",
        "CARGO_HOME": "/run/build/liveblock-linux/cargo",
        "CARGO_NET_OFFLINE": "true",
        "XDG_CACHE_HOME": "/run/build/liveblock-linux/flatpak-node/cache",
        "npm_config_cache": "/run/build/liveblock-linux/flatpak-node/npm-cache",
        "npm_config_offline": "true",
        "LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING": "1",
        "RUSTFLAGS": "--remap-path-prefix =../",
    }
    if environment != expected_environment:
        raise ValueError("Flatpak offline build environment drifted")
    if "--share=network" in manifest.get("finish-args", []):
        raise ValueError("Flatpak runtime must not have network permission")
    commands = "\n".join(module["build-commands"])
    if "npm ci --offline" not in commands or "tauri build --no-bundle --ci" not in commands:
        raise ValueError("Flatpak offline build commands drifted")


def verify() -> None:
    _, cargo, node = _validate_metadata()
    cargo_section, cargo_input, cargo_output = cargo
    node_section, node_input, node_output = node
    _validate_cargo(cargo_input, cargo_output)
    _validate_node(node_input, node_output)
    _validate_manifest(cargo_section["output"], node_section["output"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    try:
        verify()
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"Flatpak generated-source verification failed: {error}")
        return 1
    print("Flatpak generated sources match Cargo/npm lockfiles and offline manifest")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
