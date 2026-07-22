#!/usr/bin/env python3
"""Reproducible small-object training profile for sports sponsorship ads.

This profile emphasizes tiny, rotated, perspective-skewed logos found on
jerseys, helmets, race-car liveries, and venue boards. It exports candidates
only; installation requires a complete passing schema-5 promotion report.
"""
from __future__ import annotations

import argparse
import json
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def pick_device(requested: str) -> str:
    if requested != "auto":
        return requested
    import torch
    if torch.backends.mps.is_available():
        return "mps"
    if torch.cuda.is_available():
        return "0"
    return "cpu"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", required=True)
    parser.add_argument("--model", default=str(ROOT / "models" / "yolov8n.pt"))
    parser.add_argument("--epochs", type=int, default=100)
    parser.add_argument("--imgsz", type=int, default=960,
                        help="Higher resolution preserves small jersey/livery marks")
    parser.add_argument("--batch", type=int, default=8)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--name", default=None)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--install", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.install:
        parser.error("direct installation is disabled; run verify_promotion.py and install_verified_model.py")

    from ultralytics import YOLO
    import torch
    import ultralytics

    device = pick_device(args.device)
    name = args.name or datetime.now(timezone.utc).strftime("sports-ads-%Y%m%d-%H%M%S")
    project = ROOT / "tools" / "runs"
    run_dir = project / name
    if run_dir.exists():
        raise SystemExit(f"refusing to reuse existing run directory: {run_dir}")

    resolved = {
        **vars(args),
        "data": str(Path(args.data).resolve()),
        "model": str(Path(args.model).resolve()),
        "device_resolved": device,
        "python": sys.version,
        "platform": platform.platform(),
        "torch": torch.__version__,
        "ultralytics": ultralytics.__version__,
    }
    run_dir.mkdir(parents=True)
    (run_dir / "experiment.json").write_text(json.dumps(resolved, indent=2, sort_keys=True) + "\n")

    model = YOLO(args.model)
    model.train(
        data=args.data,
        epochs=args.epochs,
        imgsz=args.imgsz,
        batch=args.batch,
        device=device,
        workers=args.workers,
        project=str(project),
        name=name,
        exist_ok=True,
        seed=args.seed,
        deterministic=True,
        # Small, skewed, partially occluded sponsor marks.
        degrees=12.0,
        translate=0.12,
        scale=0.65,
        shear=6.0,
        perspective=0.0008,
        fliplr=0.5,
        flipud=0.0,
        mosaic=1.0,
        mixup=0.1,
        copy_paste=0.1,
        close_mosaic=max(5, min(20, args.epochs // 10)),
        patience=max(20, args.epochs // 4),
        plots=True,
        save=True,
    )

    weights = run_dir / "weights" / "best.pt"
    if not weights.exists():
        weights = run_dir / "weights" / "last.pt"
    if not weights.exists():
        raise SystemExit(f"training completed without weights in {run_dir / 'weights'}")
    print(f"candidate weights: {weights}")

    print(f"Candidate only (not installed): {weights}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
