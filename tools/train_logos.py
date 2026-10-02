#!/usr/bin/env python3
"""Fine-tune a YOLOv8 model on a logo / ad-detection dataset, then export to CoreML.

  Usage:
    tools/.venv/bin/python tools/train_logos.py --data path/to/data.yaml
    tools/.venv/bin/python tools/train_logos.py --data data.yaml --epochs 50 --imgsz 640 --device mps
    tools/.venv/bin/python tools/train_logos.py --data data.yaml --model yolov8s.pt --install

`data.yaml` is a standard ultralytics YOLO data config — see tools/README.md
for sources and the expected directory layout.

Auto-detects the best available device:  mps  →  cuda  →  cpu.
After training finishes, exports the best checkpoint to a CoreML .mlpackage.
With `--install`, drops it into Sources/yolov8n.mlpackage (with a .bak).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def detect_device(requested: str) -> str:
    if requested != "auto":
        return requested
    try:
        import torch  # type: ignore
    except ImportError:
        return "cpu"
    if torch.backends.mps.is_available():
        return "mps"
    if torch.cuda.is_available():
        return "cuda"
    return "cpu"


def train(args) -> Path:
    try:
        from ultralytics import YOLO  # type: ignore
    except ImportError as e:
        sys.exit(f"ultralytics not installed. Run tools/setup_env.sh first. ({e})")

    data_path = Path(args.data).expanduser().resolve()
    if not data_path.exists():
        sys.exit(f"Dataset config not found: {data_path}")

    device = detect_device(args.device)
    print(f"→ Training {args.model} on {data_path}")
    print(f"   epochs={args.epochs}  imgsz={args.imgsz}  batch={args.batch}  device={device}")

    model = YOLO(args.model)
    results = model.train(
        data=str(data_path),
        epochs=args.epochs,
        imgsz=args.imgsz,
        batch=args.batch,
        device=device,
        project=str(REPO_ROOT / "tools" / "runs"),
        name=args.run_name,
        exist_ok=False,
        patience=args.patience,
        plots=True,
        **({"fliplr": 0.0, "mosaic": 0.0, "hsv_h": 0.0, "hsv_s": 0.0,
            "hsv_v": 0.0, "translate": 0.05, "scale": 0.15} if args.screen_ads else {}),
    )

    save_dir = Path(results.save_dir) if hasattr(results, "save_dir") else None
    if save_dir is None:
        sys.exit("Training finished but save_dir is unknown — cannot locate best.pt.")
    best = save_dir / "weights" / "best.pt"
    if not best.exists():
        # Fallback: last.pt is always written.
        best = save_dir / "weights" / "last.pt"
    if not best.exists():
        sys.exit(f"No checkpoint found in {save_dir / 'weights'}.")
    print(f"✓ Best checkpoint: {best}")
    return best


def evaluate(best: Path, args) -> dict:
    """Record independent holdout results when the dataset provides a test split."""
    import yaml
    from ultralytics import YOLO
    data = yaml.safe_load(Path(args.data).read_text())
    split = "test" if data.get("test") else "val"
    metrics = YOLO(str(best)).val(data=str(Path(args.data).resolve()), split=split,
                                 imgsz=args.imgsz, batch=args.batch,
                                 device=detect_device(args.device), plots=True,
                                 project=str(best.parent.parent), name="evaluation")
    report = {"checkpoint": str(best), "dataset": str(Path(args.data).resolve()),
              "split": split, "precision": float(metrics.box.mp),
              "recall": float(metrics.box.mr), "mAP50": float(metrics.box.map50),
              "mAP50_95": float(metrics.box.map),
              "scope": "Only classes and visual domains represented in the labeled dataset."}
    (best.parent.parent / "evaluation.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("--data", required=True, type=Path, help="Path to YOLO data.yaml")
    parser.add_argument("--model", default="yolov8n.pt",
                        help="Base weights to fine-tune (yolov8n.pt is fastest)")
    parser.add_argument("--epochs", type=int, default=50)
    parser.add_argument("--imgsz", type=int, default=640)
    parser.add_argument("--batch", type=int, default=16)
    parser.add_argument("--device", default="auto",
                        choices=["auto", "mps", "cuda", "cpu"])
    parser.add_argument("--run-name", default="logo-finetune")
    parser.add_argument("--patience", type=int, default=15,
                        help="Early-stopping patience (0 disables)")
    parser.add_argument("--install", action="store_true",
                        help="After export, install into Sources/yolov8n.mlpackage")
    parser.add_argument("--screen-ads", action="store_true", help="Keep screenshot text upright and disable mosaic/color augmentation")
    parser.add_argument("--min-precision", type=float, default=0.9, help="Minimum measured precision before --install")
    parser.add_argument("--min-recall", type=float, default=0.8, help="Minimum measured recall before --install")
    parser.add_argument("--no-int8", dest="int8", action="store_false", default=True)
    parser.add_argument("--no-nms", dest="nms", action="store_false", default=True)
    args = parser.parse_args()

    best_pt = train(args)
    report = evaluate(best_pt, args)
    if args.install and (report["precision"] < args.min_precision or report["recall"] < args.min_recall):
        sys.exit("Candidate failed installation thresholds; checkpoint and evaluation were retained.")

    # Hand off to the exporter(s). One training run feeds every platform:
    #   - macOS: CoreML .mlpackage (for Vision/CoreML) + ONNX (portability)
    #   - Windows/Linux: ONNX only (coremltools is macOS-only; CoreML is a no-op)
    sys.path.insert(0, str(REPO_ROOT / "tools"))

    if sys.platform == "darwin":
        print()
        print("-> Exporting to CoreML (+ ONNX) ...")
        from export_to_coreml import (  # type: ignore
            export as export_coreml,
            install_into_repo as install_coreml,
        )

        mlpackage = export_coreml(best_pt, int8=args.int8, nms=args.nms)
        # ORT's NMS op isn't always available; the Rust detector does its own NMS.
        from export_onnx import export as export_onnx, install_into_repo as install_onnx  # type: ignore

        onnx = export_onnx(best_pt, opset=12, simplify=True, nms=False, half=False, dynamic=False)
        if args.install:
            install_coreml(mlpackage)
            install_onnx(onnx)
    else:
        print()
        print("-> Exporting to ONNX (Windows/Linux ONNX Runtime) ...")
        from export_onnx import export as export_onnx, install_into_repo as install_onnx  # type: ignore

        onnx = export_onnx(best_pt, opset=12, simplify=True, nms=False, half=False, dynamic=False)
        if args.install:
            install_onnx(onnx)

    print()
    print("OK Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
