#!/usr/bin/env python3
"""Rerun schema-5 and sign only the exact freshly promoted artifact.

The supplied report provides the gate recipe, never trusted results. The
Ed25519 private seed is read from an environment variable and is never accepted
on the command line or written to disk by this tool.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from install_verified_model import validate_promotion_report_snapshot
from promotion_contract import (DEFAULT_MODEL_SIGNING_KEY_ENV, GATE_SCHEMA,
                                artifact_sha256, rerun_promotion_gate)

MODEL_MANIFEST_SCHEMA = 2
ARTIFACT_HASH_ALGORITHM = "sha256-file-or-tree-v1"
RUNTIME_CLASSES = ["Logo", "Ad banner", "Sponsored"]
DEFAULT_PRIVATE_KEY_ENV = DEFAULT_MODEL_SIGNING_KEY_ENV


def _load_private_key(environment_name: str) -> Ed25519PrivateKey:
    encoded = os.environ.get(environment_name)
    if not encoded:
        raise ValueError(f"missing Ed25519 private seed in {environment_name}")
    try:
        seed = base64.b64decode(encoded, validate=True)
    except (ValueError, TypeError) as error:
        raise ValueError(f"{environment_name} is not canonical base64") from error
    if len(seed) != 32:
        raise ValueError(f"{environment_name} must decode to exactly 32 bytes")
    return Ed25519PrivateKey.from_private_bytes(seed)


def _signing_bytes(manifest: dict) -> bytes:
    unsigned = {key: value for key, value in manifest.items() if key != "signature"}
    return json.dumps(
        unsigned, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def _runtime_contract(report: dict, artifact_format: str) -> tuple[int, int, bool]:
    if artifact_format == "coreml":
        candidate = report.get("coreml", {}).get("candidate", {})
        if candidate.get("core_classes") != RUNTIME_CLASSES:
            raise ValueError("CoreML promotion evidence has incompatible runtime classes")
        image_size = candidate.get("latency", {}).get("image_size")
        if not (
            isinstance(image_size, list)
            and len(image_size) == 2
            and all(isinstance(value, int) and value > 0 for value in image_size)
        ):
            raise ValueError("CoreML promotion evidence lacks a valid input image size")
        return image_size[0], image_size[1], True

    onnx = report.get("onnx", {})
    candidate = onnx.get("candidate", {})
    if onnx.get("parity", {}).get("passed") is not True:
        raise ValueError("ONNX promotion evidence lacks a passing parity result")
    if candidate.get("runtime_classes") != RUNTIME_CLASSES:
        raise ValueError("ONNX promotion evidence has incompatible runtime classes")
    image_size = candidate.get("input_size")
    if not (
        isinstance(image_size, list)
        and len(image_size) == 2
        and all(isinstance(value, int) and value > 0 for value in image_size)
    ):
        raise ValueError("ONNX promotion evidence lacks a valid input size")
    nms_embedded = candidate.get("nms_embedded")
    if not isinstance(nms_embedded, bool):
        raise ValueError("ONNX promotion evidence lacks an NMS contract")
    return image_size[0], image_size[1], nms_embedded


def _create_manifest_from_attested_report(
    *,
    report_path: Path,
    artifact_format: str,
    model_version: str,
    release_sequence: int,
    key_id: str,
    private_key_env: str = DEFAULT_PRIVATE_KEY_ENV,
) -> dict:
    if artifact_format not in {"coreml", "onnx"}:
        raise ValueError("artifact format must be coreml or onnx")
    if not model_version.strip() or not key_id.strip():
        raise ValueError("model version and key id must be non-empty")
    if release_sequence < 1:
        raise ValueError("release sequence must be at least 1")

    artifact_key = "candidate_coreml" if artifact_format == "coreml" else "candidate_onnx"
    report, artifact_path, report_bytes = validate_promotion_report_snapshot(
        report_path, artifact_key
    )
    if artifact_format == "coreml" and not artifact_path.is_dir():
        raise ValueError("promoted CoreML artifact must be a directory package")
    if artifact_format == "onnx" and (
        not artifact_path.is_file() or artifact_path.suffix.lower() != ".onnx"
    ):
        raise ValueError("promoted ONNX artifact must be an .onnx regular file")

    artifact_hash = artifact_sha256(artifact_path)
    if artifact_hash != report["artifacts"][artifact_key]["sha256"]:
        raise ValueError("promoted artifact fingerprint changed before signing")
    input_width, input_height, nms_embedded = _runtime_contract(report, artifact_format)

    manifest = {
        "schemaVersion": MODEL_MANIFEST_SCHEMA,
        "modelId": "liveblock-detector",
        "modelVersion": model_version,
        "artifactFormat": artifact_format,
        "artifactHashAlgorithm": ARTIFACT_HASH_ALGORITHM,
        "artifactSha256": artifact_hash,
        "runtimeClasses": RUNTIME_CLASSES,
        "inputWidth": input_width,
        "inputHeight": input_height,
        "nmsEmbedded": nms_embedded,
        "releaseSequence": release_sequence,
        "promotionGateSchema": GATE_SCHEMA,
        "promotionReportSha256": hashlib.sha256(report_bytes).hexdigest(),
        "createdAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "keyId": key_id,
        "signature": "",
    }
    private_key = _load_private_key(private_key_env)
    manifest["signature"] = base64.b64encode(
        private_key.sign(_signing_bytes(manifest))
    ).decode("ascii")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--promotion-report", type=Path, required=True)
    parser.add_argument(
        "--attested-report-output", type=Path, required=True,
        help="new path where the fresh schema-5 rerun will be preserved",
    )
    parser.add_argument("--artifact-format", choices=("coreml", "onnx"), required=True)
    parser.add_argument("--model-version", required=True)
    parser.add_argument("--release-sequence", type=int, required=True)
    parser.add_argument("--key-id", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--private-key-env",
        default=DEFAULT_PRIVATE_KEY_ENV,
        help="name of the environment variable containing a base64 32-byte seed",
    )
    args = parser.parse_args()

    rerun_promotion_gate(
        args.promotion_report,
        args.attested_report_output,
        secret_environment_names=(args.private_key_env,),
    )
    manifest = _create_manifest_from_attested_report(
        report_path=args.attested_report_output,
        artifact_format=args.artifact_format,
        model_version=args.model_version,
        release_sequence=args.release_sequence,
        key_id=args.key_id,
        private_key_env=args.private_key_env,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
        handle.write("\n")
    print(f"signed promoted {args.artifact_format} manifest at {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
