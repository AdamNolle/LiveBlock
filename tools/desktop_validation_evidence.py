#!/usr/bin/env python3
"""Create and update privacy-minimized desktop validation evidence directories."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
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
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "screenshots").mkdir(mode=0o700)
    environment = {
        "schemaVersion": 1,
        "createdAt": utc_now(),
        "platform": args.platform,
        "gitCommit": git("rev-parse", "HEAD"),
        "gitDirty": dirty,
        "hostSystem": platform.system(),
        "hostRelease": platform.release(),
        "machine": platform.machine(),
        "osBuild": args.os_build or platform.release(),
        "hardware": args.hardware or platform.machine(),
        "gpu": args.gpu,
        "displaysAndScaling": args.displays,
        "packageSha256": args.package_sha256,
        "evidenceState": args.evidence_state,
    }
    write_new_json(output / "environment.json", environment)
    write_new_json(output / "scenario-results.json", {"schemaVersion": 1, "results": []})
    print(output)


def confined_evidence(run_dir: Path, value: str) -> str:
    supplied = run_dir / value
    if supplied.is_symlink():
        raise ValueError(f"evidence must not be a symlink: {value}")
    candidate = supplied.resolve()
    try:
        relative = candidate.relative_to(run_dir)
    except ValueError as error:
        raise ValueError(f"evidence escapes run directory: {value}") from error
    if not candidate.is_file():
        raise ValueError(f"evidence is not a regular file: {value}")
    return relative.as_posix()


def record_result(args: argparse.Namespace) -> None:
    run_dir = args.run_dir.resolve()
    if (run_dir / "artifacts.sha256").exists():
        raise ValueError("sealed validation evidence cannot be modified")
    path = run_dir / "scenario-results.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        set(document) != {"schemaVersion", "results"}
        or document["schemaVersion"] != 1
        or not isinstance(document["results"], list)
    ):
        raise ValueError("scenario-results.json has unknown fields or schema")
    evidence = [confined_evidence(run_dir, value) for value in args.evidence]
    started = validate_timestamp(args.started_at)
    finished = validate_timestamp(args.finished_at)
    if finished < started:
        raise ValueError("scenario finishedAt precedes startedAt")
    document["results"].append(
        {
            "scenario": args.scenario,
            "result": args.result,
            "startedAt": args.started_at,
            "finishedAt": args.finished_at,
            "recordedAt": utc_now(),
            "operator": args.operator,
            "notes": args.notes,
            "evidence": evidence,
        }
    )
    replace_json(path, document)


def hash_run(args: argparse.Namespace) -> None:
    run_dir = args.run_dir.resolve()
    output = run_dir / "artifacts.sha256"
    results = json.loads((run_dir / "scenario-results.json").read_text(encoding="utf-8"))
    if not isinstance(results.get("results"), list) or not results["results"]:
        raise ValueError("cannot seal validation evidence without scenario results")
    entries: list[str] = []
    for path in sorted(run_dir.rglob("*")):
        if path == output or path.is_symlink() or not path.is_file():
            continue
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
