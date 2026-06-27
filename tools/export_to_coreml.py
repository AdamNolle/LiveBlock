#!/usr/bin/env python3
"""Export a YOLOv8/YOLOv11 .pt to a CoreML .mlpackage AND an ONNX .onnx.

One training run -> every platform: macOS loads the CoreML .mlpackage, while
Windows (ORT + DirectML) and Linux (ORT + CUDA/ROCm/CPU) load the .onnx. By
default this script emits BOTH so a single run covers all targets.

  Usage:
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --install
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --no-int8
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --no-onnx

The CoreML package is written next to the .pt by ultralytics, then optionally
copied (with a `.bak` of the previous one) to `Sources/yolov8n.mlpackage` so the
macOS app picks it up after the next build. The ONNX is produced via
`export_onnx.py` (so the logic stays in one place) and, with --install, copied
into the Windows/Linux Tauri resources + the per-user runtime models dir.
"""
from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGET_PATH = REPO_ROOT / "Sources" / "yolov8n.mlpackage"

# Local sibling import for the shared ONNX exporter.
sys.path.insert(0, str(Path(__file__).resolve().parent))


def export(pt_path: Path, *, int8: bool, nms: bool) -> Path:
    try:
        from ultralytics import YOLO  # type: ignore
    except ImportError as e:
        sys.exit(f"ultralytics not installed. Run tools/setup_env.sh first. ({e})")

    if not pt_path.exists():
        sys.exit(f"Not found: {pt_path}")

    print(f"-> Loading {pt_path} ...")
    model = YOLO(str(pt_path))

    print(f"-> Exporting to CoreML (int8={int8}, nms={nms}) ...")
    output = model.export(format="coreml", int8=int8, nms=nms)

    # ultralytics returns either a path string or a Path to the .mlpackage.
    out_path = Path(output) if not isinstance(output, Path) else output
    if not out_path.exists():
        # Sometimes the file ends up alongside the .pt with the same stem.
        candidate = pt_path.with_suffix(".mlpackage")
        if candidate.exists():
            out_path = candidate
        else:
            sys.exit(f"Export reported success but {out_path} doesn't exist.")
    print(f"OK Exported: {out_path}")
    return out_path


def install_into_repo(mlpackage: Path) -> None:
    if mlpackage.resolve() == TARGET_PATH.resolve():
        print(f"OK Already at {TARGET_PATH}")
        return

    if TARGET_PATH.exists():
        backup = TARGET_PATH.with_suffix(".mlpackage.bak")
        if backup.exists():
            shutil.rmtree(backup)
        print(f"-> Backing up existing model -> {backup}")
        shutil.move(str(TARGET_PATH), str(backup))

    TARGET_PATH.parent.mkdir(parents=True, exist_ok=True)
    print(f"-> Copying {mlpackage} -> {TARGET_PATH}")
    if mlpackage.is_dir():
        shutil.copytree(mlpackage, TARGET_PATH)
    else:
        shutil.copy2(mlpackage, TARGET_PATH)
    print(f"OK Installed. Now: ./run.sh --clean")


def export_onnx_alongside(pt_path: Path, *, install: bool, nms: bool) -> Path | None:
    """Produce a sibling .onnx via export_onnx.py (shared logic, one place)."""
    try:
        import export_onnx  # type: ignore
    except ImportError as e:
        print(f"Warning: could not import export_onnx ({e}); skipping ONNX.", file=sys.stderr)
        return None

    print()
    print("-> Also exporting ONNX (for Windows/Linux ONNX Runtime) ...")
    try:
        onnx_path = export_onnx.export(
            pt_path, opset=12, simplify=True, nms=nms, half=False, dynamic=False
        )
    except SystemExit as e:
        # export_onnx.export() calls sys.exit on hard failures; don't let an ONNX
        # hiccup kill a successful CoreML run.
        print(f"Warning: ONNX export failed ({e}); CoreML export is unaffected.", file=sys.stderr)
        return None

    if install:
        export_onnx.install_into_repo(onnx_path)
    return onnx_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument("pt", type=Path, help="Path to a YOLOv8/YOLOv11 .pt")
    parser.add_argument(
        "--install",
        action="store_true",
        help="Replace Sources/yolov8n.mlpackage (and ONNX targets) with the export",
    )
    parser.add_argument(
        "--no-int8",
        dest="int8",
        action="store_false",
        default=True,
        help="Disable INT8 weight quantization",
    )
    parser.add_argument(
        "--no-nms",
        dest="nms",
        action="store_false",
        default=True,
        help="Disable embedded NMS (then app code must do post-NMS)",
    )
    parser.add_argument(
        "--no-onnx",
        dest="onnx",
        action="store_false",
        default=True,
        help="Skip the sibling ONNX export (CoreML only)",
    )
    args = parser.parse_args()

    out = export(args.pt, int8=args.int8, nms=args.nms)
    if args.install:
        install_into_repo(out)

    if args.onnx:
        # ORT's CPU/DML graph can't always run ultralytics' embedded NMS op, and
        # the Rust detector does its own NMS, so force nms=False for the ONNX.
        export_onnx_alongside(args.pt, install=args.install, nms=False)

    return 0


if __name__ == "__main__":
    sys.exit(main())
