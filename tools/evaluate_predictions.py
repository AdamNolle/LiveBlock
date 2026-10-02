#!/usr/bin/env python3
"""Evaluate deployment predictions against YOLO labels at a fixed IoU threshold."""
from __future__ import annotations
import argparse
import json
from pathlib import Path


def iou(a: dict, b: dict) -> float:
    intersection = max(0, min(a["x"]+a["width"], b["x"]+b["width"])-max(a["x"], b["x"])) * max(0, min(a["y"]+a["height"], b["y"]+b["height"])-max(a["y"], b["y"]))
    union = a["width"]*a["height"] + b["width"]*b["height"] - intersection
    return intersection / union if union > 0 else 0


def score(predictions: list[dict], labels: list[dict], threshold=0.5) -> dict:
    used = set(); true = 0
    for prediction in sorted(predictions, key=lambda p: p.get("confidence", 1), reverse=True):
        candidates = [(iou(prediction, label), i) for i, label in enumerate(labels)
                      if i not in used and prediction["class"] == label["class"]]
        overlap, match = max(candidates, default=(0, -1))
        if overlap >= threshold:
            true += 1; used.add(match)
    return {"tp": true, "fp": len(predictions)-true, "fn": len(labels)-true}


def evaluate(predictions: dict, labels_dir: Path, names: list[str]) -> dict:
    counts = {"tp": 0, "fp": 0, "fn": 0}
    files = sorted(labels_dir.glob("*.txt"))
    for path in files:
        labels = []
        for row in path.read_text().splitlines():
            cls, x, y, width, height = map(float, row.split())
            labels.append({"class": names[int(cls)], "x": x-width/2, "y": y-height/2, "width": width, "height": height})
        result = score(predictions.get(path.stem, []), labels)
        for key in counts: counts[key] += result[key]
    if not files: raise ValueError("No holdout label files")
    return {"images": len(files), "confidence_threshold": 0.5, "matching_iou": 0.5, **counts,
            "precision": counts["tp"] / max(1, counts["tp"]+counts["fp"]),
            "recall": counts["tp"] / max(1, counts["tp"]+counts["fn"])}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--predictions", type=Path, required=True)
    parser.add_argument("--labels", type=Path, required=True)
    parser.add_argument("--names", nargs="+", default=["ad"])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = evaluate(json.loads(args.predictions.read_text()), args.labels, args.names)
    args.output.write_text(json.dumps(report, indent=2)+"\n"); print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__": raise SystemExit(main())
