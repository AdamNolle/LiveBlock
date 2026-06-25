#!/usr/bin/env python3
"""Export a YOLO .pt to a CoreML .mlpackage or an ONNX graph.

  Usage:
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --install
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --no-int8
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --format onnx

The exported artifact is written next to the .pt by ultralytics. For CoreML it
is optionally copied (with a `.bak` of the previous one) to
`Sources/liveblock-detector.mlpackage` so the app picks it up after the next
build.

The heavy `ultralytics` import is deliberately LAZY (inside ``export``) so this
module imports on Python 3.13 without the venv / torch installed.
"""
from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGET_PATH = REPO_ROOT / "Sources" / "liveblock-detector.mlpackage"


def export(pt_path: Path, *, fmt: str = "coreml", int8: bool = True,
           nms: bool = True) -> Path:
    try:
        from ultralytics import YOLO  # type: ignore
    except ImportError as e:
        sys.exit(f"ultralytics not installed. Run tools/setup_env.sh first. ({e})")

    if not pt_path.exists():
        sys.exit(f"Not found: {pt_path}")

    print(f"→ Loading {pt_path}…")
    model = YOLO(str(pt_path))

    if fmt == "onnx":
        print(f"→ Exporting to ONNX (nms={nms}, imgsz=640, opset=13)…")
        # opset 13 is the floor that current onnxruntime accepts; raise only if
        # onnxruntime rejects it.
        output = model.export(format="onnx", nms=nms, imgsz=640, opset=13)
        out_suffix = ".onnx"
    else:
        print(f"→ Exporting to CoreML (int8={int8}, nms={nms})…")
        output = model.export(format="coreml", int8=int8, nms=nms, imgsz=640)
        out_suffix = ".mlpackage"

    # ultralytics returns either a path string or a Path to the artifact.
    out_path = Path(output) if not isinstance(output, Path) else output
    if not out_path.exists():
        # Sometimes the file ends up alongside the .pt with the same stem.
        candidate = pt_path.with_suffix(out_suffix)
        if candidate.exists():
            out_path = candidate
        else:
            sys.exit(f"Export reported success but {out_path} doesn't exist.")
    print(f"✓ Exported: {out_path}")
    return out_path


def install_into_repo(mlpackage: Path) -> None:
    if mlpackage.resolve() == TARGET_PATH.resolve():
        print(f"✓ Already at {TARGET_PATH}")
        return

    if TARGET_PATH.exists():
        backup = TARGET_PATH.with_suffix(".mlpackage.bak")
        if backup.exists():
            shutil.rmtree(backup)
        print(f"→ Backing up existing model → {backup}")
        shutil.move(str(TARGET_PATH), str(backup))

    TARGET_PATH.parent.mkdir(parents=True, exist_ok=True)
    print(f"→ Copying {mlpackage} → {TARGET_PATH}")
    if mlpackage.is_dir():
        shutil.copytree(mlpackage, TARGET_PATH)
    else:
        shutil.copy2(mlpackage, TARGET_PATH)
    print(f"✓ Installed. Now: ./run.sh --clean")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("pt", type=Path, help="Path to a YOLO .pt")
    parser.add_argument("--format", choices=["coreml", "onnx"], default="coreml",
                        help="Export format (default: coreml)")
    parser.add_argument("--install", action="store_true",
                        help="Replace Sources/liveblock-detector.mlpackage with "
                             "the export (CoreML only)")
    parser.add_argument("--no-int8", dest="int8", action="store_false", default=True,
                        help="Disable INT8 weight quantization (CoreML only)")
    parser.add_argument("--no-nms", dest="nms", action="store_false", default=True,
                        help="Disable embedded NMS (then app code must do post-NMS)")
    args = parser.parse_args()

    out = export(args.pt, fmt=args.format, int8=args.int8, nms=args.nms)
    if args.install:
        if args.format != "coreml":
            sys.exit("--install only applies to the CoreML .mlpackage export")
        install_into_repo(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
