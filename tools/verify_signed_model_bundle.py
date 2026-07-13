#!/usr/bin/env python3
"""Verify one offline CoreML update/release bundle against its embedded public ring."""
from __future__ import annotations

import argparse
import base64
import json
from datetime import datetime
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from promotion_contract import GATE_SCHEMA, artifact_sha256
from sign_model_manifest import MODEL_MANIFEST_SCHEMA, _signing_bytes
from validate_model_keyring import validate as validate_keyring

MANIFEST_KEYS = {
    "schemaVersion", "modelId", "modelVersion", "artifactFormat",
    "artifactHashAlgorithm", "artifactSha256", "runtimeClasses", "inputWidth",
    "inputHeight", "nmsEmbedded", "releaseSequence", "promotionGateSchema",
    "promotionReportSha256", "createdAt", "keyId", "signature",
}


def verify(bundle: Path) -> dict:
    artifact = bundle / "liveblock-detector.mlmodelc"
    manifest_path = bundle / "liveblock-detector.manifest.json"
    keyring_path = bundle / "trusted-model-keys.json"
    validate_keyring(keyring_path)
    if artifact.is_symlink() or not artifact.is_dir():
        raise ValueError("bundle requires a regular precompiled CoreML directory")
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise ValueError("bundle requires a regular model manifest")
    manifest = json.loads(manifest_path.read_bytes())
    if not isinstance(manifest, dict) or set(manifest) != MANIFEST_KEYS:
        raise ValueError("model manifest has unknown or missing fields")
    expected = {
        "schemaVersion": MODEL_MANIFEST_SCHEMA,
        "artifactFormat": "coreml",
        "artifactHashAlgorithm": "sha256-file-or-tree-v1",
        "runtimeClasses": ["Logo", "Ad banner", "Sponsored"],
        "promotionGateSchema": GATE_SCHEMA,
    }
    for key, value in expected.items():
        if manifest.get(key) != value:
            raise ValueError(f"model manifest has invalid {key}")
    for key in ("modelId", "modelVersion", "createdAt", "keyId", "signature"):
        if not isinstance(manifest.get(key), str) or not manifest[key]:
            raise ValueError(f"model manifest has invalid {key}")
    try:
        created_at = datetime.fromisoformat(manifest["createdAt"].replace("Z", "+00:00"))
        if created_at.tzinfo is None:
            raise ValueError("timezone missing")
    except ValueError as error:
        raise ValueError("model manifest has invalid createdAt") from error
    if type(manifest.get("nmsEmbedded")) is not bool:
        raise ValueError("model manifest has invalid nmsEmbedded")
    for key in ("inputWidth", "inputHeight", "releaseSequence"):
        if type(manifest.get(key)) is not int or manifest[key] < 1:
            raise ValueError(f"model manifest has invalid {key}")
    for key in ("artifactSha256", "promotionReportSha256"):
        value = manifest.get(key)
        if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError(f"model manifest has invalid {key}")
    if artifact_sha256(artifact) != manifest["artifactSha256"]:
        raise ValueError("compiled CoreML artifact fingerprint mismatch")
    keyring = json.loads(keyring_path.read_bytes())
    keys = {entry["keyId"]: entry["publicKeyBase64"] for entry in keyring["keys"]}
    encoded_key = keys.get(manifest.get("keyId"))
    if encoded_key is None:
        raise ValueError("manifest keyId is not trusted")
    try:
        signature = base64.b64decode(manifest["signature"], validate=True)
        public_key = base64.b64decode(encoded_key, validate=True)
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, _signing_bytes(manifest))
    except Exception as error:
        raise ValueError("model manifest signature verification failed") from error
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    args = parser.parse_args()
    manifest = verify(args.bundle)
    print(
        f"verified signed CoreML bundle: model={manifest['modelId']} "
        f"version={manifest['modelVersion']} sequence={manifest['releaseSequence']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
