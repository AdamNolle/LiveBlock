#!/usr/bin/env python3
"""Run LiveBlock's all-or-nothing detector promotion gate.

The command builds a human-reviewed, leakage-safe corpus, exports held-out
fixtures, compares candidate quality with a baseline, validates CoreML shape and
labels, and benchmarks candidate/baseline CoreML latency. It never installs a
model; successful verification is still followed by an explicit install step.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import traceback
from pathlib import Path

from corpus.build_sports_corpus import (REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
                                        REQUIRED_PROMOTION_PLACEMENTS,
                                        REQUIRED_PROMOTION_PRESERVATION_KINDS,
                                        build)
from corpus.export_eval_fixtures import export as export_fixtures
from eval.run_eval import evaluate
from validate_coreml_detector import validate as validate_coreml

GATE_SCHEMA = 5
REQUIRED_MIN_PRECISION = 0.50
REQUIRED_MIN_RECALL = 0.50
REQUIRED_MIN_PLACEMENT_RECALL = 0.50
REQUIRED_MAX_FALSE_POSITIVES = 10
REQUIRED_MAX_P95_MS = 10.0


def artifact_sha256(path: Path) -> str:
    """Hash a file or directory tree including stable relative paths."""
    digest = hashlib.sha256()
    if path.is_file():
        digest.update(path.read_bytes())
        return digest.hexdigest()
    if not path.is_dir():
        raise FileNotFoundError(path)
    for item in sorted(candidate for candidate in path.rglob("*") if candidate.is_file()):
        digest.update(str(item.relative_to(path)).encode())
        digest.update(b"\0")
        with item.open("rb") as handle:
            while chunk := handle.read(1024 * 1024):
                digest.update(chunk)
    return digest.hexdigest()


def gate_code_artifacts() -> dict[str, str]:
    root = Path(__file__).resolve().parent
    paths = {
        "verify_promotion.py": root / "verify_promotion.py",
        "corpus/build_sports_corpus.py": root / "corpus" / "build_sports_corpus.py",
        "corpus/export_eval_fixtures.py": root / "corpus" / "export_eval_fixtures.py",
        "eval/run_eval.py": root / "eval" / "run_eval.py",
        "validate_coreml_detector.py": root / "validate_coreml_detector.py",
    }
    return {name: artifact_sha256(path) for name, path in paths.items()}


def validate_required_facets(placements, negative_placements, preservation_kinds) -> None:
    requested = {
        "placement": set(placements),
        "negative-placement": set(negative_placements),
        "preservation-kind": set(preservation_kinds),
    }
    required = {
        "placement": set(REQUIRED_PROMOTION_PLACEMENTS),
        "negative-placement": set(REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
        "preservation-kind": set(REQUIRED_PROMOTION_PRESERVATION_KINDS),
    }
    missing = {
        kind: sorted(values - requested[kind])
        for kind, values in required.items() if values - requested[kind]
    }
    if missing:
        raise ValueError(f"promotion configuration missing required facets: {missing}")


def validate_gate_limits(*, min_precision: float, min_recall: float,
                         min_placement_recall: float, max_false_positives: int,
                         max_p95_ms: float) -> None:
    failures = []
    if min_precision < REQUIRED_MIN_PRECISION:
        failures.append(f"min_precision must be >= {REQUIRED_MIN_PRECISION}")
    if min_recall < REQUIRED_MIN_RECALL:
        failures.append(f"min_recall must be >= {REQUIRED_MIN_RECALL}")
    if min_placement_recall < REQUIRED_MIN_PLACEMENT_RECALL:
        failures.append(f"min_placement_recall must be >= {REQUIRED_MIN_PLACEMENT_RECALL}")
    if max_false_positives > REQUIRED_MAX_FALSE_POSITIVES:
        failures.append(f"max_false_positives must be <= {REQUIRED_MAX_FALSE_POSITIVES}")
    if max_p95_ms > REQUIRED_MAX_P95_MS:
        failures.append(f"max_p95_ms must be <= {REQUIRED_MAX_P95_MS}")
    if failures:
        raise ValueError("promotion configuration weakens required limits: " + "; ".join(failures))


def compare_quality(candidate: dict, baseline: dict, *, min_precision: float,
                    min_recall: float, min_placement_recall: float,
                    placements: list[str], max_false_positives: int) -> list[str]:
    failures = []
    if candidate["precision"] < min_precision:
        failures.append(f"precision {candidate['precision']:.3f} < {min_precision:.3f}")
    if candidate["recall"] < min_recall:
        failures.append(f"recall {candidate['recall']:.3f} < {min_recall:.3f}")
    if candidate["precision"] < baseline["precision"]:
        failures.append("precision regressed versus baseline")
    if candidate["recall"] < baseline["recall"]:
        failures.append("recall regressed versus baseline")
    if candidate["fp"] > max_false_positives:
        failures.append(f"false positives {candidate['fp']} > {max_false_positives}")
    for placement in placements:
        candidate_slice = candidate.get("per_placement", {}).get(placement)
        baseline_slice = baseline.get("per_placement", {}).get(placement)
        if candidate_slice is None:
            failures.append(f"candidate has no held-out {placement} truths")
        elif candidate_slice["recall"] < min_placement_recall:
            failures.append(
                f"{placement} recall {candidate_slice['recall']:.3f} < {min_placement_recall:.3f}"
            )
        elif baseline_slice is not None and candidate_slice["recall"] < baseline_slice["recall"]:
            failures.append(f"{placement} recall regressed versus baseline")
    for placement in REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS:
        candidate_metrics = candidate.get("negative_placements", {}).get(placement)
        if candidate_metrics is None:
            failures.append(f"candidate has no held-out negative placement {placement}")
        elif candidate_metrics["false_positives"] > 0:
            failures.append(f"{placement} hard-negative false positives must be zero")
    for kind in REQUIRED_PROMOTION_PRESERVATION_KINDS:
        candidate_metrics = candidate.get("preservation", {}).get(kind)
        if candidate_metrics is None:
            failures.append(f"candidate has no held-out preservation kind {kind}")
        elif candidate_metrics["false_positives"] > 0:
            failures.append(f"{kind} preservation false positives must be zero")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pool", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--candidate-model", required=True, help="candidate .pt used for quality evaluation")
    parser.add_argument("--baseline-model", required=True, help="baseline .pt used on the same fixtures")
    parser.add_argument("--candidate-coreml", type=Path, required=True)
    parser.add_argument("--baseline-coreml", type=Path, required=True)
    parser.add_argument("--placement", action="append", default=[])
    parser.add_argument("--negative-placement", action="append", default=[])
    parser.add_argument("--preservation-kind", action="append", default=[])
    parser.add_argument("--min-precision", type=float, default=REQUIRED_MIN_PRECISION)
    parser.add_argument("--min-recall", type=float, default=REQUIRED_MIN_RECALL)
    parser.add_argument("--min-placement-recall", type=float,
                        default=REQUIRED_MIN_PLACEMENT_RECALL)
    parser.add_argument("--max-false-positives", type=int,
                        default=REQUIRED_MAX_FALSE_POSITIVES)
    parser.add_argument("--max-p95-ms", type=float, default=REQUIRED_MAX_P95_MS)
    parser.add_argument("--score", type=float, default=0.25)
    parser.add_argument("--benchmark-runs", type=int, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    report: dict = {
        "passed": False,
        "failures": [],
        "config": vars(args).copy(),
        "gate": {
            "schema": GATE_SCHEMA,
            "code_artifacts": gate_code_artifacts(),
        },
    }
    report["config"] = {key: str(value) if isinstance(value, Path) else value
                        for key, value in report["config"].items()}
    try:
        validate_required_facets(args.placement, args.negative_placement,
                                 args.preservation_kind)
        validate_gate_limits(
            min_precision=args.min_precision, min_recall=args.min_recall,
            min_placement_recall=args.min_placement_recall,
            max_false_positives=args.max_false_positives, max_p95_ms=args.max_p95_ms,
        )
        report["artifacts"] = {
            key: {"path": str(path), "sha256": artifact_sha256(path)}
            for key, path in {
                "baseline_coreml": args.baseline_coreml,
                "baseline_model": Path(args.baseline_model),
                "candidate_coreml": args.candidate_coreml,
                "candidate_model": Path(args.candidate_model),
                "pool": args.pool,
            }.items()
        }
        placements = set(args.placement)
        report["corpus"] = build(
            args.pool, args.corpus, 0.15, 0.15,
            review_methods={"human"},
            required_test_placements=placements,
            stratify_placements=placements,
            stratify_preservation_kinds=set(args.preservation_kind),
            stratify_negative_placements=set(args.negative_placement),
        )
        report["artifacts"]["corpus"] = {
            "path": str(args.corpus), "sha256": artifact_sha256(args.corpus),
        }
        report["fixtures"] = export_fixtures(args.corpus, args.fixtures, "test")
        report["artifacts"]["fixtures"] = {
            "path": str(args.fixtures), "sha256": artifact_sha256(args.fixtures),
        }
        candidate = evaluate(args.candidate_model, str(args.fixtures), score_threshold=args.score)
        baseline = evaluate(args.baseline_model, str(args.fixtures), score_threshold=args.score)
        report["quality"] = {"candidate": candidate, "baseline": baseline}
        report["failures"].extend(compare_quality(
            candidate, baseline,
            min_precision=args.min_precision,
            min_recall=args.min_recall,
            min_placement_recall=args.min_placement_recall,
            placements=args.placement,
            max_false_positives=args.max_false_positives,
        ))
        candidate_coreml = validate_coreml(
            args.candidate_coreml, benchmark_runs=args.benchmark_runs, warmup_runs=5
        )
        baseline_coreml = validate_coreml(
            args.baseline_coreml, benchmark_runs=args.benchmark_runs, warmup_runs=5
        )
        report["coreml"] = {"candidate": candidate_coreml, "baseline": baseline_coreml}
        candidate_p95 = candidate_coreml["latency"]["p95_ms"]
        baseline_p95 = baseline_coreml["latency"]["p95_ms"]
        if candidate_p95 > args.max_p95_ms:
            report["failures"].append(
                f"CoreML p95 {candidate_p95:.2f} ms > budget {args.max_p95_ms:.2f} ms"
            )
        if candidate_p95 > baseline_p95:
            report["failures"].append("CoreML p95 latency regressed versus baseline")
        report["passed"] = not report["failures"]
    except Exception as error:  # Preserve a machine-readable blocked report.
        report["failures"].append(str(error))
        report["failed_stage_trace"] = traceback.format_exc()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
