#!/usr/bin/env python3
"""Export a YOLOv8/YOLOv11 .pt to ONNX for ONNX Runtime (Windows/Linux/macOS).

One training run should feed every platform:
  - macOS uses the CoreML .mlpackage (see export_to_coreml.py)
  - Windows (ORT + DirectML) and Linux (ORT + CUDA/ROCm/CPU) use this .onnx

  Usage:
    tools/.venv/bin/python tools/export_onnx.py path/to/best.pt
    python tools/export_onnx.py path/to/best.pt --install
    python tools/export_onnx.py path/to/best.pt --opset 12 --no-simplify --no-nms

The exported .onnx is written next to the .pt by ultralytics. With --install it
is copied (with a `.bak` of the previous one) to:
  - the per-OS LiveBlock runtime models dir (so a running Tauri app picks it up), AND
  - the in-repo Tauri resources dir for the current OS (so a fresh build bundles it).

NMS note: ORT's basic CPU/DML graph cannot always run ultralytics' embedded NMS
op. The Windows/Linux Rust detector (platform/*/src-tauri/src/detection.rs) does
its own NMS via liveblock_detection::non_max_suppression, so by default we export
WITHOUT embedded NMS (--no-nms is implied unless you pass --nms). Pass --nms only
if your ORT build includes the NMS contrib op and you've disabled the Rust NMS.
"""
from __future__ import annotations

import argparse
import os
import shutil
import hashlib
import json
import sys
import tempfile
from pathlib import Path

# Local helper for cross-platform install targets.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import lb_paths  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent


def repo_resources_onnx_target() -> Path:
    """In-repo Tauri resources path for the current OS (so a build bundles it).

    Windows/Linux Tauri apps load `resources/yolov8n.onnx`. macOS does not use
    ONNX at runtime (it uses CoreML), but we still drop the .onnx into the repo
    so a cross-build or a future ORT-on-mac path can find it.
    """
    if sys.platform.startswith("linux"):
        base = REPO_ROOT / "platform" / "linux" / "src-tauri" / "resources"
    elif sys.platform == "darwin":
        # macOS uses CoreML at runtime; keep a copy under windows resources is
        # wrong — instead stage it under the repo models/ dir for portability.
        base = REPO_ROOT / "models"
    else:
        base = REPO_ROOT / "platform" / "windows" / "src-tauri" / "resources"
    return base / "yolov8n.onnx"


def runtime_onnx_target() -> Path:
    """Per-user runtime models dir target (updates a *running* app, no rebuild)."""
    return lb_paths.models_dir() / "yolov8n.onnx"


def export(pt_path: Path, *, opset: int, simplify: bool, nms: bool, half: bool, dynamic: bool) -> Path:
    try:
        from ultralytics import YOLO  # type: ignore
    except ImportError as e:
        sys.exit(f"ultralytics not installed. Run tools/setup_env.sh first. ({e})")

    if not pt_path.exists():
        sys.exit(f"Not found: {pt_path}")

    print(f"-> Loading {pt_path} ...")
    model = YOLO(str(pt_path))

    print(
        f"-> Exporting to ONNX (opset={opset}, simplify={simplify}, nms={nms}, "
        f"half={half}, dynamic={dynamic}) ..."
    )
    output = model.export(
        format="onnx",
        opset=opset,
        simplify=simplify,
        nms=nms,
        half=half,
        dynamic=dynamic,
    )

    out_path = Path(output) if not isinstance(output, Path) else output
    if not out_path.exists():
        candidate = pt_path.with_suffix(".onnx")
        if candidate.exists():
            out_path = candidate
        else:
            sys.exit(f"Export reported success but {out_path} doesn't exist.")
    print(f"OK Exported: {out_path}")
    names = model.names
    ordered_names = [names[i] for i in range(len(names))] if isinstance(names, dict) else list(names)
    out_path.with_suffix(".classes.json").write_text(json.dumps({
        "schema_version": 1,
        "names": ordered_names,
        "model_sha256": hashlib.sha256(out_path.read_bytes()).hexdigest(),
    }, indent=2) + "\n", encoding="utf-8")
    return out_path


def _install_one(src: Path, target: Path) -> None:
    if src.resolve() == target.resolve():
        print(f"OK Already at {target}")
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    metadata = src.with_suffix(".classes.json")
    target_metadata = target.with_suffix(".classes.json")
    with tempfile.TemporaryDirectory(prefix=".liveblock-model-", dir=target.parent) as temp:
        staged = Path(temp) / target.name
        staged_metadata = Path(temp) / target_metadata.name
        shutil.copy2(src, staged)
        if metadata.exists():
            shutil.copy2(metadata, staged_metadata)
        if target.exists():
            staged_backup = Path(temp) / "previous.onnx"
            shutil.copy2(target, staged_backup)
            os.replace(staged_backup, target.with_suffix(".onnx.bak"))
        if target_metadata.exists():
            staged_backup = Path(temp) / "previous.classes.json"
            shutil.copy2(target_metadata, staged_backup)
            os.replace(staged_backup, target_metadata.with_suffix(".json.bak"))
        # During the two replacements a checksum mismatch fails closed; no
        # detector can load partial weights or treat stale labels as removals.
        os.replace(staged, target)
        if metadata.exists():
            os.replace(staged_metadata, target_metadata)
        else:
            target_metadata.unlink(missing_ok=True)


def install_into_repo(onnx: Path) -> None:
    """Install to BOTH the in-repo resources dir and the per-user runtime dir."""
    _install_one(onnx, repo_resources_onnx_target())
    # One trained export supplies both desktop ports, including when training
    # on macOS. Previously Mac exports were staged only in models/.
    for platform in ("windows", "linux"):
        _install_one(onnx, REPO_ROOT / "platform" / platform / "src-tauri" / "resources" / "yolov8n.onnx")
    _install_one(onnx, runtime_onnx_target())
    print("OK Installed. Windows/Linux: rebuild the Tauri app or relaunch to pick it up.")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument("pt", type=Path, help="Path to a YOLOv8/YOLOv11 .pt")
    parser.add_argument(
        "--install",
        action="store_true",
        help="Copy the export into repo resources + per-user runtime models dir",
    )
    parser.add_argument("--opset", type=int, default=12, help="ONNX opset (default 12)")
    parser.add_argument(
        "--no-simplify",
        dest="simplify",
        action="store_false",
        default=True,
        help="Disable onnx-simplifier pass",
    )
    parser.add_argument(
        "--nms",
        dest="nms",
        action="store_true",
        default=False,
        help="Embed NMS in the graph (default off; Rust does NMS). Needs ORT NMS op.",
    )
    parser.add_argument(
        "--half",
        action="store_true",
        default=False,
        help="Export FP16 weights (smaller; needs a GPU EP that supports FP16)",
    )
    parser.add_argument(
        "--dynamic",
        action="store_true",
        default=False,
        help="Dynamic input shapes (default off; fixed 640 is fastest for ORT)",
    )
    args = parser.parse_args()

    out = export(
        args.pt,
        opset=args.opset,
        simplify=args.simplify,
        nms=args.nms,
        half=args.half,
        dynamic=args.dynamic,
    )
    if args.install:
        install_into_repo(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
