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
        exist_ok=True,
        patience=args.patience,
        plots=True,
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
    parser.add_argument("--no-int8", dest="int8", action="store_false", default=True)
    parser.add_argument("--no-nms", dest="nms", action="store_false", default=True)
    args = parser.parse_args()

    best_pt = train(args)

    # Hand off to the exporter for the CoreML conversion + optional install.
    print()
    print("→ Exporting to CoreML…")
    sys.path.insert(0, str(REPO_ROOT / "tools"))
    from export_to_coreml import export, install_into_repo  # type: ignore

    mlpackage = export(best_pt, int8=args.int8, nms=args.nms)
    if args.install:
        install_into_repo(mlpackage)
    print()
    print("✓ Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
