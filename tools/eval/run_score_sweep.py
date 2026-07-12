#!/usr/bin/env python3
"""Evaluate fixed score thresholds on one held-out fixture set.

This intentionally reports every requested threshold and selects best F1 without
changing any promotion floor. It is diagnostic evidence, not a promotion gate.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from run_eval import evaluate


def f1(precision: float, recall: float) -> float:
    return 2 * precision * recall / (precision + recall) if precision + recall else 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--score", action="append", type=float, required=True)
    parser.add_argument("--iou", type=float, default=0.5)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    scores = sorted(set(args.score))
    if any(not 0 <= score <= 1 for score in scores):
        parser.error("every --score must be between 0 and 1")

    rows = []
    for score in scores:
        metrics = evaluate(args.model, args.fixtures, args.iou, score)
        rows.append({"score": score, "f1": f1(metrics["precision"], metrics["recall"]), **metrics})
    result = {
        "best_f1": max(rows, key=lambda row: row["f1"]),
        "fixtures": args.fixtures,
        "model": args.model,
        "rows": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    rendered = json.dumps(result, indent=2, sort_keys=True)
    args.output.write_text(rendered + "\n")
    print(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
