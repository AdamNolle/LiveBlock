#!/usr/bin/env python3
"""Validate that a CoreML detector is safe and compatible with LiveBlock."""
from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import coremltools as ct
from PIL import Image

EXPECTED = ["Logo", "Ad banner", "Sponsored"]


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("cannot calculate percentile of empty values")
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    weight = position - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def validate(path: Path, *, benchmark_runs: int = 0, warmup_runs: int = 3) -> dict:
    spec = ct.utils.load_spec(str(path))
    if spec.WhichOneof("Type") != "pipeline" or len(spec.pipeline.models) < 2:
        raise ValueError("detector must be a CoreML pipeline with embedded NMS")
    nms_model = spec.pipeline.models[-1]
    if nms_model.WhichOneof("Type") != "nonMaximumSuppression":
        raise ValueError("last pipeline stage must be nonMaximumSuppression")
    labels = list(nms_model.nonMaximumSuppression.stringClassLabels.vector)
    if labels[:len(EXPECTED)] != EXPECTED:
        raise ValueError(f"first labels must be {EXPECTED}, got {labels[:len(EXPECTED)]}")
    padding = labels[len(EXPECTED):]
    if any(label != str(index) for index, label in enumerate(padding, len(EXPECTED))):
        raise ValueError("unexpected non-runtime labels; only numeric CoreML padding is allowed")

    model = ct.models.MLModel(str(path))
    input_image = Image.new("RGB", (640, 640), "white")
    result = model.predict({"image": input_image})
    confidence = result["confidence"]
    if confidence.ndim != 2 or confidence.shape[1] != len(labels):
        raise ValueError(f"confidence width {confidence.shape} does not match {len(labels)} labels")
    if confidence.shape[1] > len(EXPECTED) and confidence[:, len(EXPECTED):].max(initial=0) > 0:
        raise ValueError("numeric padding classes produced non-zero confidence")
    validation = {
        "confidence_width": int(confidence.shape[1]),
        "core_classes": EXPECTED,
        "padding_classes": len(padding),
        "smoke_detections": int(confidence.shape[0]),
    }
    if benchmark_runs > 0:
        for _ in range(max(0, warmup_runs)):
            model.predict({"image": input_image})
        timings = []
        for _ in range(benchmark_runs):
            started = time.perf_counter()
            model.predict({"image": input_image})
            timings.append((time.perf_counter() - started) * 1_000)
        validation["latency"] = {
            "image_size": [640, 640],
            "mean_ms": sum(timings) / len(timings),
            "p50_ms": percentile(timings, 0.50),
            "p95_ms": percentile(timings, 0.95),
            "runs": benchmark_runs,
            "warmup_runs": max(0, warmup_runs),
        }
    return validation


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--benchmark-runs", type=int, default=0)
    parser.add_argument("--warmup-runs", type=int, default=3)
    parser.add_argument("--max-p95-ms", type=float)
    args = parser.parse_args()
    if args.benchmark_runs < 0 or args.warmup_runs < 0:
        parser.error("benchmark and warmup runs must be non-negative")
    if args.max_p95_ms is not None and args.benchmark_runs == 0:
        parser.error("--max-p95-ms requires --benchmark-runs")
    result = validate(args.model,
                      benchmark_runs=args.benchmark_runs,
                      warmup_runs=args.warmup_runs)
    rendered = json.dumps(result, indent=2, sort_keys=True)
    print(rendered)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n")
    if args.max_p95_ms is not None and result["latency"]["p95_ms"] > args.max_p95_ms:
        print(
            f"FAIL: CoreML p95 {result['latency']['p95_ms']:.2f} ms "
            f"> budget {args.max_p95_ms:.2f} ms"
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
