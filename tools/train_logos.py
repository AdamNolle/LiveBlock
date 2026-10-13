#!/usr/bin/env python3
"""Fine-tune a YOLOv8 model on a logo / ad-detection dataset, then export to CoreML.

  Usage:
    tools/.venv/bin/python tools/train_logos.py --data path/to/data.yaml
    tools/.venv/bin/python tools/train_logos.py --data data.yaml --epochs 50 --imgsz 640 --device mps
    tools/.venv/bin/python tools/train_logos.py --data data.yaml --model yolov8s.pt --install
    tools/.venv/bin/python tools/train_logos.py --data data.yaml --checkpoint tools/runs/run/weights/best.pt --install

`data.yaml` is a standard ultralytics YOLO data config — see tools/README.md
for sources and the expected directory layout.

Auto-detects the best available device:  mps  →  cuda  →  cpu.
After training finishes, exports the best checkpoint to a CoreML .mlpackage.
With `--install`, drops it into Sources/yolov8n.mlpackage (with a .bak).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import shutil
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


def evaluate(best_pt: Path, args) -> dict:
    """Score the selected checkpoint on the held-out validation split."""
    from ultralytics import YOLO  # type: ignore

    model = YOLO(str(best_pt))
    metrics = model.val(
        data=str(Path(args.data).expanduser().resolve()),
        split="val",
        imgsz=args.imgsz,
        batch=args.batch,
        device=detect_device(args.device),
        project=str(REPO_ROOT / "tools" / "runs"),
        name=f"{args.run_name}-validation",
        exist_ok=True,
        plots=True,
    )
    box = getattr(metrics, "box", None)
    class_names = {int(index): str(name) for index, name in model.names.items()}
    val_counts: dict[int, int] = {}
    val_label_dir = Path(args.data).expanduser().resolve().parent / "labels" / "val"
    if val_label_dir.is_dir():
        for label_file in val_label_dir.glob("*.txt"):
            for line in label_file.read_text(encoding="utf-8").splitlines():
                if line.strip():
                    class_id = int(line.split()[0])
                    val_counts[class_id] = val_counts.get(class_id, 0) + 1
    class_metrics = {}
    if box is not None:
        for position, class_id in enumerate(getattr(box, "ap_class_index", [])):
            class_id = int(class_id)
            name = class_names.get(class_id, str(class_id))
            class_metrics[name] = {
                "precision": float(box.p[position]),
                "recall": float(box.r[position]),
                "map50": float(box.ap50[position]),
                "validation_boxes": val_counts.get(class_id, 0),
            }

    def strong(name: str, *, precision: float = 0.85, recall: float = 0.0) -> bool:
        metric = class_metrics.get(name, {})
        return (metric.get("validation_boxes", 0) >= 5
                and metric.get("precision", 0) >= precision
                and metric.get("recall", 0) >= recall
                and metric.get("map50", 0) >= args.min_map50)

    # Sponsor marks can overlap team insignia and numbers. Enable automated
    # removal only if both keep classes have held-out recall strong enough to
    # protect them; otherwise the model still serves labeling suggestions.
    auto_block_labels = []
    if strong("ad"):
        auto_block_labels.append("ad")
    if (strong("sponsor_remove", precision=0.90)
            and strong("team_keep", recall=0.90)
            and strong("number_keep", recall=0.90)):
        auto_block_labels.append("sponsor_remove")

    report = {
        "precision": float(getattr(box, "mp", float("nan"))),
        "recall": float(getattr(box, "mr", float("nan"))),
        "map50": float(getattr(box, "map50", float("nan"))),
        "map50_95": float(getattr(box, "map", float("nan"))),
        "min_precision": args.min_precision,
        "min_map50": args.min_map50,
        "dataset": str(Path(args.data).expanduser().resolve()),
        "input_size": args.imgsz,
        "classes": [class_names[index] for index in sorted(class_names)],
        "class_metrics": class_metrics,
        "auto_block_labels": auto_block_labels,
    }
    report["passed"] = (
        math.isfinite(report["precision"])
        and math.isfinite(report["map50"])
        and report["precision"] >= args.min_precision
        and report["map50"] >= args.min_map50
    )
    report_path = best_pt.parent.parent / "quality.json"
    def safe_json(value):
        if isinstance(value, float) and not math.isfinite(value):
            return None
        if isinstance(value, dict):
            return {key: safe_json(item) for key, item in value.items()}
        if isinstance(value, list):
            return [safe_json(item) for item in value]
        return value

    serializable = safe_json(report)
    report_path.write_text(json.dumps(serializable, indent=2, allow_nan=False) + "\n",
                           encoding="utf-8")
    print(f"Validation: precision={report['precision']:.3f}, mAP@50={report['map50']:.3f}")
    allowed = auto_block_labels if report["passed"] else []
    print("Eligible auto-block classes: " + (", ".join(allowed) or "none; labeling suggestions only"))
    print(f"Quality report: {report_path}")
    return report


def publish_runtime_manifest(quality: dict) -> None:
    """Publish class names and validated policy for the installed ONNX model."""
    import lb_paths  # type: ignore

    model_dir = lb_paths.models_dir()
    model_dir.mkdir(parents=True, exist_ok=True)
    onnx = model_dir / "yolov8n.onnx"
    onnx_digest = hashlib.sha256(onnx.read_bytes()).hexdigest() if onnx.exists() else None
    manifest = {
        "schema_version": 1,
        "validated": bool(quality["passed"]),
        "classes": quality["classes"],
        # Failed-quality models can provide labeling suggestions, but must
        # never authorize automatic erasure.
        "auto_block_labels": quality["auto_block_labels"] if quality["passed"] else [],
        "precision": quality["precision"],
        "map50": quality["map50"],
        "onnx_sha256": onnx_digest,
        "input_size": quality["input_size"],
    }
    manifest_tmp = model_dir / "yolov8n.manifest.tmp"
    manifest_tmp.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    manifest_tmp.replace(model_dir / "yolov8n.manifest.json")


def install_runtime_coreml(mlpackage: Path, quality: dict) -> None:
    """Publish a validated CoreML model for the running macOS app."""
    import lb_paths  # type: ignore

    model_dir = lb_paths.models_dir()
    model_dir.mkdir(parents=True, exist_ok=True)
    target = model_dir / "yolov8n.mlpackage"
    staging = model_dir / "yolov8n.staging.mlpackage"
    backup = model_dir / "yolov8n.previous.mlpackage"
    if staging.exists():
        shutil.rmtree(staging)
    shutil.copytree(mlpackage, staging)
    if backup.exists():
        shutil.rmtree(backup)
    if target.exists():
        target.rename(backup)
    staging.rename(target)
    publish_runtime_manifest(quality)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("--data", required=True, type=Path, help="Path to YOLO data.yaml")
    parser.add_argument("--model", default="yolov8n.pt",
                        help="Base weights to fine-tune (yolov8n.pt is fastest)")
    parser.add_argument("--checkpoint", type=Path,
                        help="Evaluate/export an existing trained checkpoint without training again")
    parser.add_argument("--epochs", type=int, default=50)
    parser.add_argument("--imgsz", type=int, default=640)
    parser.add_argument("--batch", type=int, default=16)
    parser.add_argument("--device", default="auto",
                        choices=["auto", "mps", "cuda", "cpu"])
    parser.add_argument("--run-name", default="logo-finetune")
    parser.add_argument("--patience", type=int, default=15,
                        help="Early-stopping patience (0 disables)")
    parser.add_argument("--min-precision", type=float, default=0.80,
                        help="Minimum validation precision before --install (default 0.80)")
    parser.add_argument("--min-map50", type=float, default=0.50,
                        help="Minimum validation mAP@50 before --install (default 0.50)")
    parser.add_argument("--install", action="store_true",
                        help="After export, install into Sources/yolov8n.mlpackage")
    parser.add_argument("--int8", dest="int8", action="store_true", default=False,
                        help="Quantize CoreML export after validation (may change accuracy)")
    parser.add_argument("--no-int8", dest="int8", action="store_false",
                        help="Keep full-precision CoreML export (default)")
    parser.add_argument("--no-nms", dest="nms", action="store_false", default=True)
    args = parser.parse_args()

    if not (0 <= args.min_precision <= 1 and 0 <= args.min_map50 <= 1):
        parser.error("quality thresholds must be in [0, 1]")

    best_pt = args.checkpoint.expanduser().resolve() if args.checkpoint else train(args)
    if not best_pt.is_file():
        parser.error(f"checkpoint not found: {best_pt}")
    if args.checkpoint:
        print(f"Using existing checkpoint: {best_pt}")
    quality = evaluate(best_pt, args)
    if args.install and not quality["passed"]:
        print(
            "Validation gate did not pass. The model will be installed for "
            "labeling suggestions only; automatic erasure remains disabled. "
            "Add and review more labels, especially no-ad and keep examples, "
            "before expecting automatic removal."
        )

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
        if args.install:
            install_coreml(mlpackage)

        # ORT's NMS op isn't always available; the Rust detector does its own NMS.
        from export_onnx import export as export_onnx, install_into_repo as install_onnx  # type: ignore

        onnx = export_onnx(best_pt, opset=12, simplify=True, nms=False, half=False,
                           dynamic=False, imgsz=args.imgsz)
        if args.install:
            install_onnx(onnx)
            install_runtime_coreml(mlpackage, quality)
    else:
        print()
        print("-> Exporting to ONNX (Windows/Linux ONNX Runtime) ...")
        from export_onnx import export as export_onnx, install_into_repo as install_onnx  # type: ignore

        onnx = export_onnx(best_pt, opset=12, simplify=True, nms=False, half=False,
                           dynamic=False, imgsz=args.imgsz)
        if args.install:
            install_onnx(onnx)
            publish_runtime_manifest(quality)

    print()
    print("OK Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
