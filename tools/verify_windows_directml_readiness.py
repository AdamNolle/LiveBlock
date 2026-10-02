#!/usr/bin/env python3
"""Fail-closed verifier for the Windows DirectML texture-transport decision."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import tomllib
from pathlib import Path
from typing import Any

RUNTIME = {"onnxRuntime": "1.18.1", "ort": "2.0.0-rc.4", "ortSys": "2.0.0-rc.4"}
ONNX_RUNTIME_ARCHIVE_SHA256 = "51273348e0edc53d50a68fdddf29f142cdb9eca5c688d0233334c3295ebae595"
ORT_CHECKSUMS = {
    "ort": "86d83095ae3c1258738d70ae7a06195c94d966a8e546f0d3609dc90885fb61f5",
    "ort-sys": "0f2f6427193c808010b126bef45ebd33f8dee43770223a1200f84d3734d6c656",
}
GATES = (
    "captureAdapterIdentity",
    "d3d11On12CaptureDevice",
    "gpuPreprocessingParity",
    "ortDmlAllocationLifetime",
    "ioBindingOutputReadback",
    "fenceCancellationGeneration",
    "deviceLossCpuFallback",
    "vendorHardwareMatrix",
)
SAFETY_SEAMS = {
    "capacity-one-newest-frame",
    "worker-side-staging-map",
    "per-run-ort-termination",
    "generation-invalidation-before-cancel",
    "explicit-cpu-session-after-directml-load-failure",
    "truthful-runtime-capability",
}
TOP_KEYS = {
    "schemaVersion",
    "status",
    "decision",
    "runtime",
    "currentInputTransport",
    "confirmedSafetySeams",
    "requiredGates",
}
EVIDENCE_KEYS = {"kind", "reference", "sha256", "vendor"}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class ReadinessError(ValueError):
    pass


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ReadinessError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def _exact_object(value: Any, keys: set[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        actual = sorted(value) if isinstance(value, dict) else type(value).__name__
        raise ReadinessError(f"{name} must have exactly {sorted(keys)}; found {actual}")
    return value


def _regular_bytes(path: Path, maximum: int = 8 * 1024 * 1024) -> bytes:
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or stat.S_ISLNK(before.st_mode) or before.st_size > maximum:
        raise ReadinessError(f"expected bounded regular non-symlink file: {path}")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        chunks: list[bytes] = []
        size = 0
        while True:
            chunk = os.read(descriptor, 64 * 1024)
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                raise ReadinessError(f"file exceeded size limit while reading: {path}")
            chunks.append(chunk)
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    path_after = path.lstat()
    identities = {
        (before.st_dev, before.st_ino),
        (opened.st_dev, opened.st_ino),
        (after.st_dev, after.st_ino),
        (path_after.st_dev, path_after.st_ino),
    }
    if len(identities) != 1 or opened.st_size != after.st_size or size != after.st_size:
        raise ReadinessError(f"file changed while reading: {path}")
    return b"".join(chunks)


def _sha256(path: Path) -> str:
    return hashlib.sha256(_regular_bytes(path)).hexdigest()


def _validate_evidence(root: Path, evidence: Any, gate_name: str) -> set[str]:
    if not isinstance(evidence, list):
        raise ReadinessError(f"{gate_name}.evidence must be an array")
    vendors: set[str] = set()
    for index, item in enumerate(evidence):
        item = _exact_object(item, EVIDENCE_KEYS, f"{gate_name}.evidence[{index}]")
        if item["kind"] not in {"source-test", "windows-test", "parity-report", "hardware-run"}:
            raise ReadinessError(f"unknown evidence kind for {gate_name}")
        if item["vendor"] not in {"not-applicable", "amd", "intel", "nvidia", "cpu"}:
            raise ReadinessError(f"unknown evidence vendor for {gate_name}")
        reference = item["reference"]
        if not isinstance(reference, str) or not reference or Path(reference).is_absolute():
            raise ReadinessError(f"{gate_name} evidence reference must be a repository-relative path")
        candidate = root / reference
        candidate_metadata = candidate.lstat()
        if stat.S_ISLNK(candidate_metadata.st_mode) or not stat.S_ISREG(candidate_metadata.st_mode):
            raise ReadinessError(f"{gate_name} evidence must be a regular non-symlink file")
        path = candidate.resolve(strict=True)
        try:
            path.relative_to(root)
        except ValueError as exc:
            raise ReadinessError(f"{gate_name} evidence escapes project root") from exc
        if not isinstance(item["sha256"], str) or not SHA256_RE.fullmatch(item["sha256"]):
            raise ReadinessError(f"{gate_name} evidence SHA-256 is malformed")
        if _sha256(path) != item["sha256"]:
            raise ReadinessError(f"{gate_name} evidence hash mismatch: {reference}")
        vendors.add(item["vendor"])
    return vendors


def _locked_package(lock: dict[str, Any], name: str) -> dict[str, Any]:
    matches = [package for package in lock.get("package", []) if package.get("name") == name]
    if len(matches) != 1:
        raise ReadinessError(f"Windows Cargo.lock must contain exactly one {name}")
    return matches[0]


def verify(project_root: Path, readiness_path: Path) -> dict[str, Any]:
    root = project_root.resolve(strict=True)
    readiness_path = readiness_path.resolve(strict=True)
    try:
        readiness_path.relative_to(root)
    except ValueError as exc:
        raise ReadinessError("readiness document must remain under project root") from exc
    data = json.loads(_regular_bytes(readiness_path), object_pairs_hook=_reject_duplicate_keys)
    data = _exact_object(data, TOP_KEYS, "readiness")
    if type(data["schemaVersion"]) is not int or data["schemaVersion"] != 1:
        raise ReadinessError("unsupported readiness schema")
    if data["runtime"] != RUNTIME:
        raise ReadinessError("runtime pin drift")
    if (
        not isinstance(data["confirmedSafetySeams"], list)
        or len(data["confirmedSafetySeams"]) != len(set(data["confirmedSafetySeams"]))
        or set(data["confirmedSafetySeams"]) != SAFETY_SEAMS
    ):
        raise ReadinessError("confirmed safety seams drift")
    gates = _exact_object(data["requiredGates"], set(GATES), "requiredGates")

    passed: dict[str, bool] = {}
    vendor_coverage: set[str] = set()
    for gate_name in GATES:
        gate = _exact_object(gates[gate_name], {"passed", "evidence"}, gate_name)
        if type(gate["passed"]) is not bool:
            raise ReadinessError(f"{gate_name}.passed must be Boolean")
        vendors = _validate_evidence(root, gate["evidence"], gate_name)
        if gate["passed"] and not gate["evidence"]:
            raise ReadinessError(f"passed gate lacks evidence: {gate_name}")
        if not gate["passed"] and gate["evidence"]:
            raise ReadinessError(f"failed gate must not imply accepted evidence: {gate_name}")
        passed[gate_name] = gate["passed"]
        if gate_name == "vendorHardwareMatrix":
            vendor_coverage = vendors

    all_passed = all(passed.values())
    if data["status"] == "ready":
        if data["decision"] != "enable" or not all_passed:
            raise ReadinessError("ready transport requires enable decision and every gate")
        if data["currentInputTransport"] != "d3d12-dml-allocation-f32-rgb-nchw":
            raise ReadinessError("ready transport must name the D3D12 DML allocation path")
        if vendor_coverage != {"amd", "intel", "nvidia", "cpu"}:
            raise ReadinessError("ready transport requires AMD, Intel, NVIDIA, and CPU evidence")
    elif data["status"] == "blocked":
        if data["decision"] != "defer" or all_passed:
            raise ReadinessError("blocked transport must be deferred with at least one failed gate")
        if data["currentInputTransport"] != "cpu-uploaded-f32-rgb-nchw":
            raise ReadinessError("blocked current transport must remain CPU-uploaded NCHW")
    else:
        raise ReadinessError("unknown readiness status")

    lock = tomllib.loads(_regular_bytes(root / "platform/windows/Cargo.lock").decode())
    for name in ("ort", "ort-sys"):
        package = _locked_package(lock, name)
        if package.get("version") != RUNTIME["ort" if name == "ort" else "ortSys"]:
            raise ReadinessError(f"{name} version drift")
        if package.get("checksum") != ORT_CHECKSUMS[name]:
            raise ReadinessError(f"{name} checksum drift")

    cargo = _regular_bytes(root / "platform/windows/src-tauri/Cargo.toml").decode()
    capture = _regular_bytes(root / "platform/windows/src-tauri/src/capture.rs").decode()
    detection = _regular_bytes(root / "platform/windows/src-tauri/src/detection.rs").decode()
    main = _regular_bytes(root / "platform/windows/src-tauri/src/main.rs").decode()
    staging = _regular_bytes(root / "tools/stage_windows_onnxruntime.py").decode()
    directml_doc = _regular_bytes(root / "docs/WINDOWS_DIRECTML.md").decode()

    for required in ('VERSION = "1.18.1"', ONNX_RUNTIME_ARCHIVE_SHA256):
        if required not in staging:
            raise ReadinessError("ONNX Runtime staging pin drift")
    for required in (
        "preprocess_bgra_letterbox",
        "Array4::from_shape_vec",
        "directml_registered_cpu_uploaded_tensor",
        "with_parallel_execution(false)",
        "with_memory_pattern(false)",
    ):
        if required not in detection:
            raise ReadinessError(f"current detector transport fact missing: {required}")
    for required in ("RunOptions::new()", "run_options.terminate()", "capture_generation.fetch_add"):
        if required not in main:
            raise ReadinessError(f"cancellation safety seam missing: {required}")

    source = cargo + capture + detection
    d3d12_feature_presence = (
        "Win32_Graphics_Direct3D12" in cargo,
        "Win32_Graphics_Direct3D11on12" in cargo,
    )
    interop_symbol_presence = (
        "D3D11On12CreateDevice" in capture,
        "AdapterLuid" in capture,
    )
    has_d3d12_feature = all(d3d12_feature_presence)
    has_interop = all(interop_symbol_presence)
    has_dml_allocation = "CreateGPUAllocationFromD3DResource" in source
    has_io_binding = "create_binding()" in detection and "run_with_options" in detection
    if data["status"] == "ready":
        if not all((has_d3d12_feature, has_interop, has_dml_allocation, has_io_binding)):
            raise ReadinessError("ready status conflicts with missing D3D12/DML source implementation")
        if "blocked (0/8 readiness gates)" in main:
            raise ReadinessError("ready status conflicts with blocked runtime capability language")
    else:
        for required in ("D3D11CreateDevice(", "D3D_DRIVER_TYPE_HARDWARE", "MiscFlags: 0"):
            if required not in capture:
                raise ReadinessError(f"blocked capture fact missing: {required}")
        if any((*d3d12_feature_presence, *interop_symbol_presence, has_dml_allocation, has_io_binding)):
            raise ReadinessError("source gained a partial texture path; reassess every gate before merging")
        if "blocked (0/8 readiness gates)" not in main:
            raise ReadinessError("blocked runtime capability limitation is missing")
        for required in (
            "plain D3D11 device",
            "unshared",
            "not a safe incremental patch",
            "leave the texture-transport checklist item open",
        ):
            if required not in directml_doc:
                raise ReadinessError(f"DirectML decision documentation missing: {required}")

    fingerprints = {
        str(path.relative_to(root)): _sha256(path)
        for path in (
            readiness_path,
            root / "platform/windows/Cargo.lock",
            root / "platform/windows/src-tauri/Cargo.toml",
            root / "platform/windows/src-tauri/src/capture.rs",
            root / "platform/windows/src-tauri/src/detection.rs",
            root / "platform/windows/src-tauri/src/main.rs",
            root / "docs/WINDOWS_DIRECTML.md",
        )
    }
    return {
        "schemaVersion": 1,
        "status": data["status"],
        "decision": data["decision"],
        "currentInputTransport": data["currentInputTransport"],
        "passedGateCount": sum(passed.values()),
        "requiredGateCount": len(GATES),
        "sourceFingerprints": fingerprints,
        "boundary": "source/readiness verification is not physical DirectML execution evidence",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--readiness",
        type=Path,
        default=Path("platform/windows/directml-transport-readiness.json"),
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.project_root.resolve(strict=True)
    readiness = args.readiness if args.readiness.is_absolute() else root / args.readiness
    try:
        summary = verify(root, readiness)
        if args.output:
            output = args.output if args.output.is_absolute() else root / args.output
            payload = (json.dumps(summary, indent=2, sort_keys=True) + "\n").encode()
            descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
            try:
                written = 0
                while written < len(payload):
                    count = os.write(descriptor, payload[written:])
                    if count <= 0:
                        raise OSError("short write")
                    written += count
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
    except (OSError, UnicodeError, json.JSONDecodeError, tomllib.TOMLDecodeError, ReadinessError) as exc:
        print(f"Windows DirectML readiness verification failed: {exc}")
        return 1
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
