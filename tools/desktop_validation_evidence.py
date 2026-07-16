#!/usr/bin/env python3
"""Create and update privacy-minimized desktop validation evidence directories."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import stat
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

RESULTS = {"pass", "fail", "blocked"}
PLATFORMS = {"macos", "windows", "linux"}
EVIDENCE_STATES = {
    "verified-real-device",
    "verified-hosted-runner",
    "build-only",
    "blocked-credentials",
    "blocked-hardware",
    "unsupported-by-platform",
}
BLOCKED_EVIDENCE_STATES = {
    "blocked-credentials",
    "blocked-hardware",
    "unsupported-by-platform",
}
ENVIRONMENT_FIELDS = {
    "schemaVersion",
    "createdAt",
    "platform",
    "gitCommit",
    "gitDirty",
    "hostSystem",
    "hostRelease",
    "machine",
    "osBuild",
    "hardware",
    "gpu",
    "displaysAndScaling",
    "packageSha256",
    "evidenceState",
}
VERIFIED_HOST_SYSTEMS = {"macos": "Darwin", "windows": "Windows", "linux": "Linux"}
RESULT_FIELDS = {
    "scenario",
    "result",
    "startedAt",
    "finishedAt",
    "recordedAt",
    "operator",
    "notes",
    "evidence",
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def validate_timestamp(value: str) -> datetime:
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("scenario timestamps must include a timezone")
    return parsed


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, text=True, stdout=subprocess.PIPE
    ).stdout.strip()


def write_new_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def replace_json(path: Path, value: object) -> None:
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def init_run(args: argparse.Namespace) -> None:
    dirty = bool(git("status", "--porcelain"))
    if args.package_sha256 and (
        len(args.package_sha256) != 64
        or any(character not in "0123456789abcdefABCDEF" for character in args.package_sha256)
    ):
        raise ValueError("package SHA-256 must contain exactly 64 hexadecimal characters")
    if dirty and not args.allow_dirty:
        raise ValueError("validation evidence requires a clean git worktree")
    if dirty and args.evidence_state != "build-only":
        raise ValueError("dirty worktrees may only create explicitly build-only evidence")
    host_system = platform.system()
    if args.evidence_state.startswith("verified-") and host_system != VERIFIED_HOST_SYSTEMS[
        args.platform
    ]:
        raise ValueError("verified evidence must run on the declared native platform")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "screenshots").mkdir(mode=0o700)
    environment = {
        "schemaVersion": 1,
        "createdAt": utc_now(),
        "platform": args.platform,
        "gitCommit": git("rev-parse", "HEAD"),
        "gitDirty": dirty,
        "hostSystem": host_system,
        "hostRelease": platform.release(),
        "machine": platform.machine(),
        "osBuild": args.os_build or platform.release(),
        "hardware": args.hardware or platform.machine(),
        "gpu": args.gpu,
        "displaysAndScaling": args.displays,
        "packageSha256": args.package_sha256.lower() if args.package_sha256 else None,
        "evidenceState": args.evidence_state,
    }
    write_new_json(output / "environment.json", environment)
    write_new_json(output / "scenario-results.json", {"schemaVersion": 1, "results": []})
    print(output)


def read_json_regular(path: Path, label: str) -> object:
    try:
        mode = os.lstat(path).st_mode
    except FileNotFoundError as error:
        raise ValueError(f"{label} is missing") from error
    if not stat.S_ISREG(mode):
        raise ValueError(f"{label} must be an immediate regular non-symlink file")
    return json.loads(path.read_text(encoding="utf-8"))


def load_environment(run_dir: Path) -> dict[str, object]:
    document = read_json_regular(run_dir / "environment.json", "environment.json")
    if not isinstance(document, dict):
        raise ValueError("environment.json must contain an object")
    if set(document) != ENVIRONMENT_FIELDS or document.get("schemaVersion") != 1:
        raise ValueError("environment.json has unknown fields or schema")
    if document.get("platform") not in PLATFORMS:
        raise ValueError("environment.json has an invalid platform")
    if document.get("evidenceState") not in EVIDENCE_STATES:
        raise ValueError("environment.json has an invalid evidence state")
    if not isinstance(document.get("gitDirty"), bool):
        raise ValueError("environment.json gitDirty must be Boolean")
    if document["gitDirty"] and document["evidenceState"] != "build-only":
        raise ValueError("dirty environments may only contain build-only evidence")
    for field in (
        "gitCommit",
        "hostSystem",
        "hostRelease",
        "machine",
        "osBuild",
        "hardware",
        "gpu",
        "displaysAndScaling",
    ):
        if not isinstance(document.get(field), str) or not document[field].strip():
            raise ValueError(f"environment.json {field} must be a nonempty string")
    if str(document["evidenceState"]).startswith("verified-") and document[
        "hostSystem"
    ] != VERIFIED_HOST_SYSTEMS[str(document["platform"])]:
        raise ValueError("verified evidence must run on the declared native platform")
    commit = str(document["gitCommit"])
    if len(commit) not in {40, 64} or any(character not in "0123456789abcdef" for character in commit):
        raise ValueError("environment.json gitCommit must be a full lowercase Git object ID")
    package_hash = document.get("packageSha256")
    if package_hash is not None and (
        not isinstance(package_hash, str)
        or len(package_hash) != 64
        or any(character not in "0123456789abcdef" for character in package_hash)
    ):
        raise ValueError("environment.json packageSha256 must be null or lowercase SHA-256")
    validate_timestamp(str(document.get("createdAt", "")))
    return document


def confined_evidence(run_dir: Path, value: str) -> str:
    if not value or Path(value).is_absolute():
        raise ValueError("evidence path must be a nonempty relative path")
    supplied = run_dir / value
    relative_supplied = supplied.relative_to(run_dir)
    current = run_dir
    for part in relative_supplied.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f"evidence must not traverse a symlink: {value}")
    candidate = supplied.resolve()
    try:
        relative = candidate.relative_to(run_dir)
    except ValueError as error:
        raise ValueError(f"evidence escapes run directory: {value}") from error
    if not candidate.is_file():
        raise ValueError(f"evidence is not a regular file: {value}")
    if candidate.stat().st_size == 0:
        raise ValueError(f"evidence is empty: {value}")
    return relative.as_posix()


def validate_results_document(
    run_dir: Path, document: object, evidence_state: str
) -> dict[str, object]:
    if (
        not isinstance(document, dict)
        or set(document) != {"schemaVersion", "results"}
        or document.get("schemaVersion") != 1
        or not isinstance(document.get("results"), list)
    ):
        raise ValueError("scenario-results.json has unknown fields or schema")
    for entry in document["results"]:
        if not isinstance(entry, dict) or set(entry) != RESULT_FIELDS:
            raise ValueError("scenario result has unknown or missing fields")
        if entry.get("result") not in RESULTS:
            raise ValueError("scenario result is invalid")
        if not isinstance(entry.get("scenario"), str) or not entry["scenario"].strip():
            raise ValueError("scenario must be nonempty")
        if not isinstance(entry.get("operator"), str) or not entry["operator"].strip():
            raise ValueError("operator must be nonempty")
        if not isinstance(entry.get("notes"), str):
            raise ValueError("notes must be a string")
        started = validate_timestamp(str(entry.get("startedAt", "")))
        finished = validate_timestamp(str(entry.get("finishedAt", "")))
        recorded = validate_timestamp(str(entry.get("recordedAt", "")))
        if finished < started:
            raise ValueError("scenario finishedAt precedes startedAt")
        if recorded < finished:
            raise ValueError("scenario recordedAt precedes finishedAt")
        if evidence_state in BLOCKED_EVIDENCE_STATES and entry["result"] != "blocked":
            raise ValueError("blocked/unsupported evidence states cannot record pass or fail")
        if entry["result"] == "blocked" and not entry["notes"].strip():
            raise ValueError("blocked scenarios require an explanatory note")
        if not isinstance(entry.get("evidence"), list) or not all(
            isinstance(value, str) for value in entry["evidence"]
        ):
            raise ValueError("scenario evidence must be a list of paths")
        normalized = [confined_evidence(run_dir, value) for value in entry["evidence"]]
        if normalized != entry["evidence"]:
            raise ValueError("scenario evidence paths must be canonical")
        if len(normalized) != len(set(normalized)):
            raise ValueError("scenario evidence paths must be unique")
        if entry["result"] == "pass" and not normalized:
            raise ValueError("passing scenarios require at least one nonempty evidence file")
    return document


def record_result(args: argparse.Namespace) -> None:
    run_dir = args.run_dir.resolve()
    if (run_dir / "artifacts.sha256").exists():
        raise ValueError("sealed validation evidence cannot be modified")
    environment = load_environment(run_dir)
    path = run_dir / "scenario-results.json"
    document = read_json_regular(path, "scenario-results.json")
    validate_results_document(run_dir, document, str(environment["evidenceState"]))
    entry = {
        "scenario": args.scenario,
        "result": args.result,
        "startedAt": args.started_at,
        "finishedAt": args.finished_at,
        "recordedAt": utc_now(),
        "operator": args.operator,
        "notes": args.notes,
        "evidence": [confined_evidence(run_dir, value) for value in args.evidence],
    }
    document["results"].append(entry)
    validate_results_document(run_dir, document, str(environment["evidenceState"]))
    replace_json(path, document)


def hash_run(args: argparse.Namespace) -> None:
    run_dir = args.run_dir.resolve()
    output = run_dir / "artifacts.sha256"
    environment = load_environment(run_dir)
    results = read_json_regular(run_dir / "scenario-results.json", "scenario-results.json")
    validate_results_document(run_dir, results, str(environment["evidenceState"]))
    if not results["results"]:
        raise ValueError("cannot seal validation evidence without scenario results")
    entries: list[str] = []
    for path in sorted(run_dir.rglob("*")):
        if path == output:
            continue
        mode = os.lstat(path).st_mode
        if stat.S_ISLNK(mode):
            raise ValueError(f"validation evidence contains a symlink: {path.relative_to(run_dir)}")
        if stat.S_ISDIR(mode):
            continue
        if not stat.S_ISREG(mode):
            raise ValueError(f"validation evidence contains a special file: {path.relative_to(run_dir)}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        entries.append(f"{digest}  {path.relative_to(run_dir).as_posix()}")
    fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write("\n".join(entries) + "\n")
        handle.flush()
        os.fsync(handle.fileno())
    print(output)


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    initialize = commands.add_parser("init")
    initialize.add_argument("--platform", choices=sorted(PLATFORMS), required=True)
    initialize.add_argument("--evidence-state", choices=sorted(EVIDENCE_STATES), required=True)
    initialize.add_argument("--output", type=Path, required=True)
    initialize.add_argument("--os-build")
    initialize.add_argument("--hardware")
    initialize.add_argument("--gpu", default="not-recorded")
    initialize.add_argument("--displays", default="not-recorded")
    initialize.add_argument("--package-sha256")
    initialize.add_argument("--allow-dirty", action="store_true", help=argparse.SUPPRESS)
    initialize.set_defaults(function=init_run)

    record = commands.add_parser("record")
    record.add_argument("--run-dir", type=Path, required=True)
    record.add_argument("--scenario", required=True)
    record.add_argument("--result", choices=sorted(RESULTS), required=True)
    record.add_argument("--operator", required=True)
    record.add_argument("--started-at", required=True)
    record.add_argument("--finished-at", required=True)
    record.add_argument("--notes", default="")
    record.add_argument("--evidence", action="append", default=[])
    record.set_defaults(function=record_result)

    hashes = commands.add_parser("hashes")
    hashes.add_argument("--run-dir", type=Path, required=True)
    hashes.set_defaults(function=hash_run)
    return root


def main() -> None:
    args = parser().parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
