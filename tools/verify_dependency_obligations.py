#!/usr/bin/env python3
"""Verify selected SPDX choices and prepared MPL source-availability evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tomllib
from pathlib import Path
from typing import Any

PURL = re.compile(r"^pkg:cargo/([A-Za-z0-9_.+-]+)@([A-Za-z0-9_.+-]+)$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


def _strict_object(value: Any, keys: set[str], description: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{description} must contain exactly {', '.join(sorted(keys))}")
    return value


def _regular_bytes(path: Path) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"required evidence must be a regular non-symlink file: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        data = b""
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            data += chunk
        after = os.fstat(descriptor)
        if (before.st_dev, before.st_ino, before.st_size) != (after.st_dev, after.st_ino, after.st_size):
            raise ValueError(f"evidence changed while being read: {path}")
        return data
    finally:
        os.close(descriptor)


def _write_exclusive(path: Path, document: dict[str, Any]) -> None:
    payload = (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        os.write(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def verify(
    license_report_path: Path,
    decisions_path: Path,
    source_offer_path: Path,
    cargo_locks: list[Path],
) -> dict[str, Any]:
    report_bytes = _regular_bytes(license_report_path)
    report = json.loads(report_bytes)
    if report.get("schemaVersion") != 1 or report.get("passed") is not True:
        raise ValueError("dependency license report must be passing schema 1 evidence")

    decisions_bytes = _regular_bytes(decisions_path)
    decisions = _strict_object(json.loads(decisions_bytes), {"schemaVersion", "decisions"}, "license decisions")
    if decisions["schemaVersion"] != 1 or not isinstance(decisions["decisions"], list):
        raise ValueError("license decisions must use schema 1 with an array")
    decision_purls = [item.get("purl") for item in decisions["decisions"] if isinstance(item, dict)]
    selections = report.get("licenseSelections", [])
    selected_purls = [item.get("purl") for item in selections if isinstance(item, dict)]
    if (
        decisions["decisions"] != selections
        or decision_purls != selected_purls
        or len(set(decision_purls)) != len(decision_purls)
    ):
        raise ValueError("generated license selections do not exactly match ordered decisions")

    offer_bytes = _regular_bytes(source_offer_path)
    offer = _strict_object(json.loads(offer_bytes), {"schemaVersion", "licenseText", "sourceOffers"}, "MPL source offer")
    if offer["schemaVersion"] != 1 or not isinstance(offer["sourceOffers"], list):
        raise ValueError("MPL source offer must use schema 1 with a sourceOffers array")
    license_text = _strict_object(
        offer["licenseText"], {"spdxId", "path", "sha256", "source"}, "licenseText",
    )
    if license_text["spdxId"] != "MPL-2.0" or license_text["path"] != "MPL-2.0.txt":
        raise ValueError("source offer must bind the canonical local MPL-2.0 text")
    text_path = source_offer_path.parent / license_text["path"]
    text_hash = hashlib.sha256(_regular_bytes(text_path)).hexdigest()
    if text_hash != license_text["sha256"] or not HEX64.fullmatch(str(license_text["sha256"])):
        raise ValueError("MPL-2.0 license text hash mismatch")
    if not str(license_text["source"]).startswith("https://github.com/spdx/license-list-data/blob/"):
        raise ValueError("MPL-2.0 license source must be a pinned HTTPS SPDX URL")

    locked: dict[tuple[str, str], set[str]] = {}
    lock_hashes: list[str] = []
    for lock_path in cargo_locks:
        lock_bytes = _regular_bytes(lock_path)
        lock_hashes.append(hashlib.sha256(lock_bytes).hexdigest())
        lock = tomllib.loads(lock_bytes.decode())
        for package in lock.get("package", []):
            checksum = package.get("checksum")
            if checksum:
                locked.setdefault((str(package["name"]), str(package["version"])), set()).add(str(checksum))

    offer_purls: list[str] = []
    for item in offer["sourceOffers"]:
        item = _strict_object(
            item,
            {"purl", "declaredLicense", "archiveUrl", "archiveSha256", "modifications"},
            "source offer entry",
        )
        match = PURL.fullmatch(str(item["purl"]))
        if not match:
            raise ValueError(f"invalid Cargo purl in source offer: {item['purl']}")
        name, version = match.groups()
        expected_url = f"https://crates.io/api/v1/crates/{name}/{version}/download"
        if item["archiveUrl"] != expected_url or item["declaredLicense"] != "MPL-2.0" or item["modifications"] != "none":
            raise ValueError(f"invalid MPL source-offer terms for {item['purl']}")
        checksum = str(item["archiveSha256"])
        if not HEX64.fullmatch(checksum) or checksum not in locked.get((name, version), set()):
            raise ValueError(f"source-offer checksum is not bound by Cargo.lock for {item['purl']}")
        offer_purls.append(str(item["purl"]))

    if offer_purls != sorted(offer_purls) or len(set(offer_purls)) != len(offer_purls):
        raise ValueError("MPL source offers must be unique and sorted by purl")
    review_purls = sorted(item.get("purl") for item in report.get("reviewRequired", []) if isinstance(item, dict))
    if offer_purls != review_purls:
        raise ValueError("MPL source offers must exactly cover generated review-required components")

    return {
        "schemaVersion": 1,
        "passed": True,
        "licenseReportSha256": hashlib.sha256(report_bytes).hexdigest(),
        "licenseDecisionsSha256": hashlib.sha256(decisions_bytes).hexdigest(),
        "sourceOfferSha256": hashlib.sha256(offer_bytes).hexdigest(),
        "mplLicenseTextSha256": text_hash,
        "cargoLockSha256": sorted(lock_hashes),
        "selectedLicensePurls": selected_purls,
        "preparedMplSourceOfferPurls": offer_purls,
        "boundary": "prepared licensing evidence; accountable human/legal release review remains required",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--license-report", type=Path, required=True)
    parser.add_argument("--license-decisions", type=Path, required=True)
    parser.add_argument("--source-offer", type=Path, required=True)
    parser.add_argument("--cargo-lock", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        document = verify(
            arguments.license_report,
            arguments.license_decisions,
            arguments.source_offer,
            arguments.cargo_lock,
        )
        _write_exclusive(arguments.output, document)
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"dependency obligation verification failed: {error}")
        return 1
    print(f"Verified dependency obligations: {arguments.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
