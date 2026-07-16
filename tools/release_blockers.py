#!/usr/bin/env python3
"""Build a conservative, machine-readable assessment of automatable release blockers.

This diagnostic never authorizes installation or release. Passing promotion evidence is
accepted only after this process reruns the complete current schema-5 gate.
"""
from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import os
import re
import stat
import subprocess
from pathlib import Path
from typing import Any

from promotion_contract import (
    GATE_SCHEMA,
    artifact_sha256,
    gate_code_artifacts,
    rerun_promotion_gate,
    validate_gate_limits,
    validate_required_facets,
)
from verify_windows_directml_readiness import verify as verify_directml

SCHEMA_VERSION = 1
COMMIT_RE = re.compile(r"^[0-9a-f]{40}(?:[0-9a-f]{24})?$")
BOUNDARY = "diagnostic only; never authorizes model installation, signing, publication, or release"
EXTERNAL_PREREQUISITES = [
    "accountable-dependency-approval",
    "production-signing-and-publication",
    "real-device-platform-validation",
]
KEYRINGS = (
    ("macos", "Sources/Resources/trusted-model-keys.json"),
    ("windows", "platform/windows/src-tauri/resources/trusted-model-keys.json"),
    ("linux", "platform/linux/src-tauri/resources/trusted-model-keys.json"),
)
PROMOTION_TOP_KEYS = {
    "passed",
    "failures",
    "config",
    "gate",
    "artifacts",
    "corpus",
    "fixtures",
    "quality",
    "coreml",
    "failed_stage_trace",
}
PROMOTION_REQUIRED_KEYS = {"passed", "failures", "config", "gate"}
PROMOTION_CONFIG_KEYS = {
    "pool",
    "corpus",
    "fixtures",
    "candidate_model",
    "baseline_model",
    "candidate_coreml",
    "baseline_coreml",
    "placement",
    "negative_placement",
    "preservation_kind",
    "min_precision",
    "min_recall",
    "min_placement_recall",
    "max_false_positives",
    "max_p95_ms",
    "score",
    "benchmark_runs",
    "output",
    "exclusive_output",
}
PROMOTION_CONFIG_REQUIRED = PROMOTION_CONFIG_KEYS - {"exclusive_output"}
PROMOTION_ARTIFACT_KEYS = {
    "baseline_coreml",
    "baseline_model",
    "candidate_coreml",
    "candidate_model",
    "pool",
    "corpus",
    "fixtures",
}


class AssessmentError(ValueError):
    pass


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise AssessmentError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def _regular_bytes(path: Path, maximum: int = 32 * 1024 * 1024) -> bytes:
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or stat.S_ISLNK(before.st_mode) or before.st_size > maximum:
        raise AssessmentError(f"expected bounded regular non-symlink file: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0))
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
                raise AssessmentError(f"file exceeded size limit while reading: {path}")
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
        raise AssessmentError(f"file changed while being read: {path}")
    return b"".join(chunks)


def _load_json(path: Path) -> tuple[Any, bytes]:
    payload = _regular_bytes(path)
    return json.loads(payload, object_pairs_hook=_reject_duplicate_keys), payload


def _sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _nullable_human_gate(reason: str, *, status_sha256: str | None = None) -> dict[str, Any]:
    return {
        "status": "blocked",
        "reasonCode": reason,
        "approved": None,
        "pending": None,
        "excluded": None,
        "missing": None,
        "plannedGroupCount": None,
        "remainingCandidateDeficitCount": None,
        "planSha256": None,
        "reviewStateSha256": None,
        "statusSha256": status_sha256,
    }


def assess_human_review(root: Path) -> dict[str, Any]:
    pool = root / "tools/datasets/sports-ads/pool"
    plan_path = root / "tools/datasets/sports-ads/human-review-plan.json"
    status_path = root / "tools/datasets/sports-ads/human-review-plan-status.json"
    required = (pool, plan_path, status_path)
    if any(not path.exists() or path.is_symlink() for path in required):
        return _nullable_human_gate("human-review-input-missing")
    status_payload: bytes | None = None
    try:
        # Keep clean hosted release-contract checks lightweight: the corpus module
        # imports Pillow, which is needed only when ignored local review inputs exist.
        from corpus.review_labels import load_plan_paths, review_plan_status

        plan, _plan_payload = _load_json(plan_path)
        if not isinstance(plan, dict) or set(plan) != {
            "remaining_deficits",
            "selected_group_count",
            "selected_groups",
            "target_groups_per_facet",
        }:
            raise AssessmentError("human review plan has unknown or missing fields")
        groups = plan["selected_groups"]
        if not isinstance(groups, list) or any(
            not isinstance(item, dict)
            or set(item) != {"covers", "path", "source_group"}
            or not isinstance(item["path"], str)
            or not item["path"]
            or not isinstance(item["covers"], list)
            or any(not isinstance(value, str) or not value for value in item["covers"])
            or not isinstance(item["source_group"], str)
            or not item["source_group"]
            for item in groups
        ):
            raise AssessmentError("human review plan groups are not closed")
        for candidate in pool.rglob("*"):
            mode = candidate.lstat().st_mode
            if stat.S_ISLNK(mode) or not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise AssessmentError("human review pool contains a link or special file")
        stored, status_payload = _load_json(status_path)
        allowed_paths = load_plan_paths(plan_path)
        current = review_plan_status(pool, allowed_paths, plan_path)
        if stored != current:
            gate = _nullable_human_gate(
                "human-review-input-mismatch", status_sha256=_sha256(status_payload)
            )
            gate["planSha256"] = current.get("plan_sha256")
            gate["reviewStateSha256"] = current.get("review_state_sha256")
            return gate
        if not isinstance(stored, dict) or set(stored) != {
            "counts",
            "items",
            "planned_group_count",
            "remaining_candidate_deficits",
            "review_state_sha256",
            "plan_sha256",
        }:
            raise AssessmentError("human review status has unknown or missing fields")
        counts = stored["counts"]
        if not isinstance(counts, dict) or set(counts) != {"approved", "excluded", "missing", "pending"}:
            raise AssessmentError("human review counts are not closed")
        if any(type(counts[name]) is not int or counts[name] < 0 for name in counts):
            raise AssessmentError("human review counts must be nonnegative integers")
        planned = stored["planned_group_count"]
        deficits = stored["remaining_candidate_deficits"]
        if type(planned) is not int or planned < 1 or not isinstance(deficits, dict):
            raise AssessmentError("human review plan summary is invalid")
        if sum(counts.values()) != planned or not isinstance(stored["items"], list):
            raise AssessmentError("human review counts do not cover the plan")
        complete = (
            counts["approved"] == planned
            and counts["pending"] == 0
            and counts["excluded"] == 0
            and counts["missing"] == 0
            and not deficits
        )
        return {
            "status": "satisfied" if complete else "blocked",
            "reasonCode": "human-review-complete" if complete else "human-review-incomplete",
            "approved": counts["approved"],
            "pending": counts["pending"],
            "excluded": counts["excluded"],
            "missing": counts["missing"],
            "plannedGroupCount": planned,
            "remainingCandidateDeficitCount": len(deficits),
            "planSha256": stored["plan_sha256"],
            "reviewStateSha256": stored["review_state_sha256"],
            "statusSha256": _sha256(status_payload),
        }
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError, AssessmentError, ValueError):
        return _nullable_human_gate(
            "human-review-input-invalid",
            status_sha256=_sha256(status_payload) if status_payload is not None else None,
        )


def _promotion_placeholder(reason: str, *, payload: bytes | None = None) -> dict[str, Any]:
    return {
        "status": "blocked",
        "reasonCode": reason,
        "reportSha256": _sha256(payload) if payload is not None else None,
        "reportPassed": None,
        "gateCodeCurrent": None,
        "failureCount": None,
    }


def _validate_promotion_report(root: Path, report: Any) -> tuple[bool, bool, int]:
    if not isinstance(report, dict) or not PROMOTION_REQUIRED_KEYS <= set(report) <= PROMOTION_TOP_KEYS:
        raise AssessmentError("promotion report has unknown or missing top-level fields")
    if type(report["passed"]) is not bool:
        raise AssessmentError("promotion passed must be Boolean")
    failures = report["failures"]
    if not isinstance(failures, list) or any(not isinstance(item, str) or not item for item in failures):
        raise AssessmentError("promotion failures must be nonempty strings")
    if report["passed"] == bool(failures):
        raise AssessmentError("promotion pass/failure state is inconsistent")
    gate = report["gate"]
    if not isinstance(gate, dict) or set(gate) != {"schema", "code_artifacts"}:
        raise AssessmentError("promotion gate metadata is not closed")
    if type(gate["schema"]) is not int or gate["schema"] != GATE_SCHEMA:
        raise AssessmentError("promotion gate schema is not current")
    if not isinstance(gate["code_artifacts"], dict):
        raise AssessmentError("promotion gate code fingerprints are invalid")
    gate_current = gate["code_artifacts"] == gate_code_artifacts()

    config = report["config"]
    if not isinstance(config, dict) or not PROMOTION_CONFIG_REQUIRED <= set(config) <= PROMOTION_CONFIG_KEYS:
        raise AssessmentError("promotion configuration has unknown or missing fields")
    validate_required_facets(
        config["placement"], config["negative_placement"], config["preservation_kind"]
    )
    validate_gate_limits(
        min_precision=config["min_precision"],
        min_recall=config["min_recall"],
        min_placement_recall=config["min_placement_recall"],
        max_false_positives=config["max_false_positives"],
        max_p95_ms=config["max_p95_ms"],
    )

    artifacts = report.get("artifacts", {})
    if not isinstance(artifacts, dict) or not set(artifacts) <= PROMOTION_ARTIFACT_KEYS:
        raise AssessmentError("promotion artifact bindings are invalid")
    if gate_current:
        for name, item in artifacts.items():
            if not isinstance(item, dict) or set(item) != {"path", "sha256"}:
                raise AssessmentError(f"promotion artifact binding is not closed: {name}")
            relative = item["path"]
            if not isinstance(relative, str) or not relative or Path(relative).is_absolute():
                raise AssessmentError(f"promotion artifact path is invalid: {name}")
            candidate = (root / relative).resolve(strict=True)
            candidate.relative_to(root)
            if artifact_sha256(candidate) != item["sha256"]:
                raise AssessmentError(f"promotion artifact hash mismatch: {name}")
    return report["passed"], gate_current, len(failures)


def assess_promotion(root: Path, report_path: Path, fresh_report_path: Path) -> dict[str, Any]:
    if not report_path.exists() or report_path.is_symlink():
        return _promotion_placeholder("promotion-report-missing")
    payload: bytes | None = None
    try:
        report, payload = _load_json(report_path)
        passed, gate_current, failure_count = _validate_promotion_report(root, report)
        if not gate_current:
            return {
                "status": "blocked",
                "reasonCode": "promotion-report-stale",
                "reportSha256": _sha256(payload),
                "reportPassed": passed,
                "gateCodeCurrent": False,
                "failureCount": failure_count,
            }
        if not passed:
            return {
                "status": "blocked",
                "reasonCode": "promotion-report-failed",
                "reportSha256": _sha256(payload),
                "reportPassed": False,
                "gateCodeCurrent": True,
                "failureCount": failure_count,
            }
        if fresh_report_path.exists() or fresh_report_path.is_symlink():
            return {
                "status": "blocked",
                "reasonCode": "promotion-fresh-rerun-required",
                "reportSha256": _sha256(payload),
                "reportPassed": True,
                "gateCodeCurrent": True,
                "failureCount": 0,
            }
        try:
            rerun_promotion_gate(report_path, fresh_report_path)
        except (OSError, ValueError):
            fresh_payload = None
            fresh_passed: bool | None = None
            fresh_current: bool | None = None
            fresh_failure_count: int | None = None
            try:
                fresh, fresh_payload = _load_json(fresh_report_path)
                fresh_passed, fresh_current, fresh_failure_count = _validate_promotion_report(root, fresh)
            except (OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError, AssessmentError, ValueError):
                pass
            return {
                "status": "blocked",
                "reasonCode": "promotion-fresh-rerun-failed",
                "reportSha256": _sha256(fresh_payload) if fresh_payload is not None else _sha256(payload),
                "reportPassed": fresh_passed,
                "gateCodeCurrent": fresh_current,
                "failureCount": fresh_failure_count,
            }
        fresh, fresh_payload = _load_json(fresh_report_path)
        fresh_passed, fresh_current, fresh_failure_count = _validate_promotion_report(root, fresh)
        if not fresh_passed or not fresh_current or fresh_failure_count:
            raise AssessmentError("fresh promotion rerun did not produce current passing evidence")
        return {
            "status": "satisfied",
            "reasonCode": "promotion-passed-current-rerun",
            "reportSha256": _sha256(fresh_payload),
            "reportPassed": True,
            "gateCodeCurrent": True,
            "failureCount": 0,
        }
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError, AssessmentError, ValueError):
        return _promotion_placeholder("promotion-report-invalid", payload=payload)


def _keyring_count(document: Any) -> int:
    if (
        not isinstance(document, dict)
        or set(document) != {"schemaVersion", "keys"}
        or type(document["schemaVersion"]) is not int
        or document["schemaVersion"] != 1
        or not isinstance(document["keys"], list)
    ):
        raise AssessmentError("keyring is not closed schema 1")
    seen: set[str] = set()
    for entry in document["keys"]:
        if not isinstance(entry, dict) or set(entry) != {"keyId", "publicKeyBase64"}:
            raise AssessmentError("keyring entry is not closed")
        key_id = entry["keyId"]
        encoded = entry["publicKeyBase64"]
        if not isinstance(key_id, str) or not key_id or key_id in seen or not isinstance(encoded, str):
            raise AssessmentError("keyring identity or public key is invalid")
        try:
            raw = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError) as error:
            raise AssessmentError("keyring public key is not canonical base64") from error
        if len(raw) != 32 or base64.b64encode(raw).decode("ascii") != encoded:
            raise AssessmentError("keyring public key must encode exactly 32 bytes")
        seen.add(key_id)
    return len(document["keys"])


def assess_keyrings(root: Path) -> dict[str, Any]:
    entries: list[dict[str, Any]] = []
    reason_rank = {
        "production-keyrings-nonempty": 0,
        "production-keyring-empty": 1,
        "production-keyring-missing": 2,
        "production-keyring-invalid": 3,
    }
    reason = "production-keyrings-nonempty"
    for platform_name, relative in KEYRINGS:
        path = root / relative
        entry = {"platform": platform_name, "path": relative, "sha256": None, "keyCount": None}
        candidate_reason = "production-keyrings-nonempty"
        if not path.exists() or path.is_symlink():
            candidate_reason = "production-keyring-missing"
            if reason_rank[candidate_reason] > reason_rank[reason]:
                reason = candidate_reason
            entries.append(entry)
            continue
        payload: bytes | None = None
        try:
            document, payload = _load_json(path)
            entry["sha256"] = _sha256(payload)
            entry["keyCount"] = _keyring_count(document)
            if entry["keyCount"] == 0:
                candidate_reason = "production-keyring-empty"
        except (OSError, UnicodeError, json.JSONDecodeError, TypeError, AssessmentError, ValueError):
            entry["sha256"] = _sha256(payload) if payload is not None else None
            candidate_reason = "production-keyring-invalid"
        if reason_rank[candidate_reason] > reason_rank[reason]:
            reason = candidate_reason
        entries.append(entry)
    satisfied = reason == "production-keyrings-nonempty"
    return {
        "status": "satisfied" if satisfied else "blocked",
        "reasonCode": reason,
        "keyrings": entries,
    }


def assess_directml(root: Path) -> dict[str, Any]:
    readiness = root / "platform/windows/directml-transport-readiness.json"
    payload: bytes | None = None
    try:
        payload = _regular_bytes(readiness)
        summary = verify_directml(root, readiness)
        if _regular_bytes(readiness) != payload:
            raise AssessmentError("DirectML readiness changed during verification")
        ready = summary["status"] == "ready" and summary["passedGateCount"] == summary["requiredGateCount"]
        return {
            "status": "satisfied" if ready else "blocked",
            "reasonCode": "directml-ready" if ready else "directml-deferred",
            "readinessSha256": _sha256(payload),
            "passedGateCount": summary["passedGateCount"],
            "requiredGateCount": summary["requiredGateCount"],
            "currentInputTransport": summary["currentInputTransport"],
        }
    except (OSError, UnicodeError, json.JSONDecodeError, TypeError, ValueError):
        return {
            "status": "blocked",
            "reasonCode": "directml-readiness-invalid",
            "readinessSha256": _sha256(payload) if payload is not None else None,
            "passedGateCount": None,
            "requiredGateCount": None,
            "currentInputTransport": None,
        }


def assess(root: Path, *, commit: str, promotion_report: Path, fresh_promotion_report: Path) -> dict[str, Any]:
    root = root.resolve(strict=True)
    if not COMMIT_RE.fullmatch(commit):
        raise AssessmentError("project commit must be a full lowercase Git object ID")
    promotion_report = promotion_report if promotion_report.is_absolute() else root / promotion_report
    fresh_promotion_report = (
        fresh_promotion_report if fresh_promotion_report.is_absolute() else root / fresh_promotion_report
    )
    gates = {
        "humanReview": assess_human_review(root),
        "schema5Promotion": assess_promotion(root, promotion_report, fresh_promotion_report),
        "productionModelTrust": assess_keyrings(root),
        "directmlTextureTransport": assess_directml(root),
    }
    gate_ids = {
        "humanReview": "human-review",
        "schema5Promotion": "schema-5-promotion",
        "productionModelTrust": "production-model-trust",
        "directmlTextureTransport": "directml-texture-transport",
    }
    blockers = [gate_ids[name] for name, gate in gates.items() if gate["status"] != "satisfied"]
    clear = not blockers
    return {
        "schemaVersion": SCHEMA_VERSION,
        "status": "external-review-required" if clear else "blocked",
        "automatablePrerequisitesSatisfied": clear,
        "projectCommit": commit,
        "blockerIds": blockers,
        "gates": gates,
        "externalPrerequisitesNotAssessed": EXTERNAL_PREREQUISITES,
        "boundary": BOUNDARY,
    }


def _write_exclusive(path: Path, document: dict[str, Any]) -> None:
    payload = (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
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


def _git(root: Path, *arguments: str) -> str:
    return subprocess.run(
        ["git", *arguments], cwd=root, check=True, text=True, stdout=subprocess.PIPE
    ).stdout.strip()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--promotion-report", type=Path, default=Path("tools/runs/promotion-gate-current.json"))
    parser.add_argument(
        "--fresh-promotion-report",
        type=Path,
        default=Path("tools/runs/release-blocker-assessment/fresh-promotion-report.json"),
    )
    parser.add_argument("--commit")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--require-clean-git", action="store_true")
    parser.add_argument("--require-clear", action="store_true")
    arguments = parser.parse_args()
    try:
        root = arguments.project_root.resolve(strict=True)
        commit = arguments.commit or _git(root, "rev-parse", "HEAD")
        if arguments.require_clean_git and _git(root, "status", "--porcelain"):
            raise AssessmentError("release blocker assessment requires a clean Git worktree")
        document = assess(
            root,
            commit=commit,
            promotion_report=arguments.promotion_report,
            fresh_promotion_report=arguments.fresh_promotion_report,
        )
        output = arguments.output if arguments.output.is_absolute() else root / arguments.output
        _write_exclusive(output, document)
    except (OSError, subprocess.CalledProcessError, AssessmentError) as error:
        print(f"release blocker assessment failed: {error}")
        return 2
    print(json.dumps(document, sort_keys=True))
    if arguments.require_clear and not document["automatablePrerequisitesSatisfied"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
