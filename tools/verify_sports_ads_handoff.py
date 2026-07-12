#!/usr/bin/env python3
"""Rerunnable full validation for the currently blocked sports-ad handoff."""
from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

from corpus.review_labels import (REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                                  REQUIRED_PROMOTION_PLACEMENTS,
                                  REQUIRED_PROMOTION_PRESERVATION_KINDS)

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "tools" / "runs" / "sports-ads-handoff-verification"
PYTHON = ROOT / "tools" / ".venv" / "bin" / "python"


def run(name: str, command: list[str], env: dict[str, str] | None = None) -> int:
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    with (ARTIFACTS / f"{name}.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                                stderr=subprocess.STDOUT, check=False)
    print(f"{name}: exit {result.returncode} ({ARTIFACTS / f'{name}.log'})")
    return result.returncode


def environment_snapshot() -> dict[str, str]:
    requirements = ROOT / "tools" / "requirements.txt"
    return {
        "machine": platform.machine(),
        "macos": platform.mac_ver()[0],
        "python_executable": sys.executable,
        "python_version": platform.python_version(),
        "requirements_sha256": hashlib.sha256(requirements.read_bytes()).hexdigest(),
        "xcode": subprocess.check_output(["xcodebuild", "-version"], text=True).strip(),
    }


def main() -> int:
    env = os.environ.copy()
    env["PYTHONPATH"] = "tools:tools/eval"
    checks: dict[str, object] = {}
    environment = environment_snapshot()
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    (ARTIFACTS / "environment.json").write_text(
        json.dumps(environment, indent=2, sort_keys=True) + "\n"
    )

    checks["python_tests"] = run("python-tests", [
        str(PYTHON), "-m", "pytest", "-q", "tools/test_corpus.py",
        "tools/eval/test_eval.py", "tools/test_build_openvocab.py",
    ], env)
    checks["macos_tests"] = run("macos-tests", [
        "xcodebuild", "-project", "LiveBlock.xcodeproj", "-scheme", "LiveBlock",
        "-destination", "platform=macOS", "test", "CODE_SIGNING_ALLOWED=NO",
    ], env)

    status_path = ROOT / "tools/datasets/sports-ads/human-review-plan-status.json"
    with status_path.open("w") as status_output:
        status_result = subprocess.run([
            str(PYTHON), "tools/corpus/review_labels.py",
            "--pool", "tools/datasets/sports-ads/pool",
            "--plan-file", "tools/datasets/sports-ads/human-review-plan.json",
            "--plan-status",
        ], cwd=ROOT, env=env, stdout=status_output, stderr=subprocess.PIPE,
           text=True, check=False)
    checks["review_status"] = status_result.returncode
    status = json.loads(status_path.read_text()) if status_result.returncode == 0 else {}
    checks["review_counts"] = status.get("counts")
    checks["remaining_candidate_deficits"] = status.get("remaining_candidate_deficits")
    if status.get("counts", {}).get("missing") != 0:
        checks["review_status"] = 1

    gate_command = [
        str(PYTHON), "tools/verify_promotion.py",
        "--pool", "tools/datasets/sports-ads/pool",
        "--corpus", "tools/datasets/sports-ads/yolo-promotion-human-v1",
        "--fixtures", "tools/datasets/sports-ads/eval-promotion-human-v1",
        "--candidate-model", "tools/runs/sports-ads-real-v5/weights/best.pt",
        "--baseline-model", "Sources/liveblock-detector.mlpackage",
        "--candidate-coreml", "tools/runs/sports-ads-real-v5/weights/best.mlpackage",
        "--baseline-coreml", "Sources/liveblock-detector.mlpackage",
    ]
    for placement in REQUIRED_PROMOTION_PLACEMENTS:
        gate_command += ["--placement", placement]
    for placement in REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS:
        gate_command += ["--negative-placement", placement]
    for kind in REQUIRED_PROMOTION_PRESERVATION_KINDS:
        gate_command += ["--preservation-kind", kind]
    gate_command += [
        "--min-precision", "0.5", "--min-recall", "0.5",
        "--max-false-positives", "10", "--max-p95-ms", "10",
        "--score", "0.25", "--benchmark-runs", "30",
        "--output", "tools/runs/promotion-gate-current.json",
    ]
    gate_exit = run("promotion-gate", gate_command, env)
    report = json.loads((ROOT / "tools/runs/promotion-gate-current.json").read_text())
    expected_blocker = (
        gate_exit == 1 and report.get("passed") is False
        and any("no reviewed" in failure for failure in report.get("failures", []))
    )
    checks["promotion_gate_exit"] = gate_exit
    checks["promotion_blocked_on_missing_human_review"] = expected_blocker

    passed = all(checks[key] == 0 for key in ("python_tests", "macos_tests", "review_status")) and expected_blocker
    summary = {
        "passed": passed,
        "working_directory": str(ROOT),
        "environment": environment,
        "checks": checks,
    }
    (ARTIFACTS / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
