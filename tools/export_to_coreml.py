#!/usr/bin/env python3
"""Export a YOLO .pt to a CoreML .mlpackage or an ONNX graph.

  Usage:
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --no-int8
    tools/.venv/bin/python tools/export_to_coreml.py path/to/best.pt --format onnx

The exported artifact is written next to the .pt by ultralytics. Export never
installs: replacement requires a complete passing schema-5 report consumed by
`tools/install_verified_model.py`.

The heavy `ultralytics` import is deliberately LAZY (inside ``export``) so this
module imports on Python 3.13 without the venv / torch installed.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

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
    """Retained as a fail-closed compatibility shim for older callers."""
    raise RuntimeError(
        f"direct installation of {mlpackage} is disabled; "
        "run verify_promotion.py and install_verified_model.py"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("pt", type=Path, help="Path to a YOLO .pt")
    parser.add_argument("--format", choices=["coreml", "onnx"], default="coreml",
                        help="Export format (default: coreml)")
    parser.add_argument("--install", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--no-int8", dest="int8", action="store_false", default=True,
                        help="Disable INT8 weight quantization (CoreML only)")
    parser.add_argument("--no-nms", dest="nms", action="store_false", default=True,
                        help="Disable embedded NMS (then app code must do post-NMS)")
    args = parser.parse_args()

    if args.install:
        parser.error("direct installation is disabled; run verify_promotion.py and install_verified_model.py")
    export(args.pt, fmt=args.format, int8=args.int8, nms=args.nms)
    return 0


if __name__ == "__main__":
    sys.exit(main())
