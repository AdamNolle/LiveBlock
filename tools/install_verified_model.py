#!/usr/bin/env python3
"""Atomically install only the exact CoreML candidate from a passing gate report."""
from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path

from verify_promotion import (GATE_SCHEMA, artifact_sha256, gate_code_artifacts,
                              validate_gate_limits, validate_required_facets)


def install(report_path: Path, destination: Path) -> Path:
    report = json.loads(report_path.read_text())
    if report.get("passed") is not True or report.get("failures"):
        raise ValueError("promotion report did not pass every gate")
    gate = report.get("gate", {})
    if gate.get("schema") != GATE_SCHEMA:
        raise ValueError("promotion report gate schema is missing or stale")
    if gate.get("code_artifacts") != gate_code_artifacts():
        raise ValueError("promotion gate code or dependencies changed after the report was generated")
    missing_sections = sorted(
        section for section in ("config", "corpus", "fixtures", "quality", "coreml")
        if section not in report
    )
    if missing_sections:
        raise ValueError(f"passing report is incomplete: {missing_sections}")
    validate_required_facets(report["config"].get("placement", []),
                             report["config"].get("negative_placement", []),
                             report["config"].get("preservation_kind", []))
    validate_gate_limits(
        min_precision=report["config"].get("min_precision", 0),
        min_recall=report["config"].get("min_recall", 0),
        min_placement_recall=report["config"].get("min_placement_recall", 0),
        max_false_positives=report["config"].get("max_false_positives", float("inf")),
        max_p95_ms=report["config"].get("max_p95_ms", float("inf")),
    )
    artifacts = report.get("artifacts", {})
    required_artifacts = {
        "baseline_coreml", "baseline_model", "candidate_coreml", "candidate_model",
        "corpus", "fixtures", "pool",
    }
    missing_artifacts = sorted(required_artifacts - artifacts.keys())
    if missing_artifacts:
        raise ValueError(f"passing report lacks required artifact fingerprints: {missing_artifacts}")
    for key in sorted(required_artifacts):
        artifact = artifacts[key]
        path = Path(artifact["path"])
        if report["config"].get(key) != str(path):
            raise ValueError(f"{key} path disagrees with promotion config")
        if artifact_sha256(path) != artifact.get("sha256"):
            raise ValueError(f"{key} fingerprint no longer matches the passing report")
    artifact = artifacts["candidate_coreml"]
    source = Path(artifact["path"])
    if destination.exists() and artifact_sha256(destination) == artifact["sha256"]:
        return destination

    temporary = destination.with_name(destination.name + ".installing")
    backup = destination.with_name(destination.name + ".pre-promotion")
    if temporary.exists() or backup.exists():
        raise FileExistsError("refusing to overwrite an existing install temporary/backup")
    shutil.copytree(source, temporary)
    if artifact_sha256(temporary) != artifact["sha256"]:
        shutil.rmtree(temporary)
        raise ValueError("copied candidate fingerprint mismatch")
    try:
        if destination.exists():
            os.replace(destination, backup)
        os.replace(temporary, destination)
    except Exception:
        if not destination.exists() and backup.exists():
            os.replace(backup, destination)
        if temporary.exists():
            shutil.rmtree(temporary)
        raise
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    installed = install(args.report, args.destination)
    print(f"installed verified model at {installed}")
    backup = installed.with_name(installed.name + ".pre-promotion")
    if backup.exists():
        print(f"previous model preserved at {backup}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
