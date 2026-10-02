#!/usr/bin/env python3
"""Validate the strict schema-1 public model keyring used by release builds."""
from __future__ import annotations

import argparse
import base64
import binascii
import json
from pathlib import Path


def validate(path: Path, *, allow_empty: bool = False) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError("model keyring must be a regular file")
    document = json.loads(path.read_bytes())
    if not isinstance(document, dict) or set(document) != {"schemaVersion", "keys"}:
        raise ValueError("model keyring has unknown or missing fields")
    if type(document["schemaVersion"]) is not int or document["schemaVersion"] != 1:
        raise ValueError("model keyring schemaVersion must be integer 1")
    entries = document["keys"]
    if not isinstance(entries, list) or (not entries and not allow_empty):
        raise ValueError("production model keyring must contain at least one key")
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"keyId", "publicKeyBase64"}:
            raise ValueError("model keyring entry has unknown or missing fields")
        key_id = entry["keyId"]
        encoded = entry["publicKeyBase64"]
        if not isinstance(key_id, str) or not key_id or key_id in seen:
            raise ValueError("model keyring keyId is empty or duplicated")
        if not isinstance(encoded, str):
            raise ValueError("model keyring public key must be base64")
        try:
            raw = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError) as error:
            raise ValueError("model keyring public key must be canonical base64") from error
        if len(raw) != 32 or base64.b64encode(raw).decode("ascii") != encoded:
            raise ValueError("model keyring public key must encode exactly 32 bytes")
        seen.add(key_id)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keyring", type=Path, required=True)
    parser.add_argument("--allow-empty", action="store_true")
    args = parser.parse_args()
    validate(args.keyring, allow_empty=args.allow_empty)
    print(f"validated model keyring: {args.keyring}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
