#!/usr/bin/env python3
"""Export the in-app labeling JSON files to a YOLO-format dataset.

Reads from:
   ~/Library/Application Support/LiveBlock/training/screenshots/*.png
   ~/Library/Application Support/LiveBlock/training/labels/*.json

Writes a YOLO-formatted dataset suitable for `tools/train_logos.py`:
   ~/Library/Application Support/LiveBlock/training/exports/<run-name>/
       images/train/  images/val/
       labels/train/  labels/val/
       data.yaml

  Usage:
    tools/.venv/bin/python tools/export_labels.py
    tools/.venv/bin/python tools/export_labels.py --val-split 0.15 --name my-export
    tools/.venv/bin/python tools/export_labels.py --include-empty
"""
from __future__ import annotations

import argparse
import json
import random
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Tuple

HOME = Path.home()
DEFAULT_ROOT = HOME / "Library" / "Application Support" / "LiveBlock" / "training"


def discover(root: Path) -> List[Tuple[Path, Path]]:
    """Return list of (image_path, label_path) for fully labeled screenshots."""
    images = root / "screenshots"
    labels = root / "labels"
    if not images.is_dir():
        sys.exit(f"No screenshots directory: {images}")
    pairs = []
    for png in sorted(images.glob("*.png")):
        json_path = labels / (png.stem + ".json")
        if json_path.exists():
            pairs.append((png, json_path))
    return pairs


def to_yolo_lines(label_doc: dict, *, class_id: int = 0) -> List[str]:
    """Convert in-app label JSON to YOLO format lines."""
    lines = []
    for box in label_doc.get("boxes", []):
        # YOLO uses center coords, normalized [0..1].
        x = float(box["x"])
        y = float(box["y"])
        w = float(box["width"])
        h = float(box["height"])
        cx = x + w / 2.0
        cy = y + h / 2.0
        lines.append(f"{class_id} {cx:.6f} {cy:.6f} {w:.6f} {h:.6f}")
    return lines


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                    help="Training root (default: ~/Library/Application Support/LiveBlock/training)")
    ap.add_argument("--name", type=str, default=None,
                    help="Export folder name (default: auto-generated timestamp)")
    ap.add_argument("--val-split", type=float, default=0.15,
                    help="Fraction held out for validation (default 0.15)")
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--include-empty", action="store_true",
                    help="Include screenshots labeled with zero boxes (negative samples).")
    ap.add_argument("--class-name", type=str, default="ad",
                    help="Class name for data.yaml (single-class dataset)")
    args = ap.parse_args()

    root = args.root.expanduser().resolve()
    pairs = discover(root)
    if not pairs:
        sys.exit("No labeled screenshots found. Open the Labeling window in LiveBlock first.")

    # Filter empties unless asked.
    kept = []
    skipped_empty = 0
    for png, json_path in pairs:
        try:
            doc = json.loads(json_path.read_text())
        except json.JSONDecodeError as e:
            print(f"Warning: couldn't parse {json_path}: {e}", file=sys.stderr)
            continue
        if not doc.get("boxes") and not args.include_empty:
            skipped_empty += 1
            continue
        kept.append((png, doc))

    if not kept:
        sys.exit("All labels are empty. Add some boxes, or pass --include-empty.")

    # Split
    random.seed(args.seed)
    random.shuffle(kept)
    val_n = max(1, int(len(kept) * args.val_split)) if len(kept) > 1 else 0
    val = kept[:val_n]
    train = kept[val_n:]

    # Output structure
    name = args.name or "export-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    out = root / "exports" / name
    images_train = out / "images" / "train"
    images_val   = out / "images" / "val"
    labels_train = out / "labels" / "train"
    labels_val   = out / "labels" / "val"
    for d in (images_train, images_val, labels_train, labels_val):
        d.mkdir(parents=True, exist_ok=True)

    def write_split(items, img_dir: Path, lbl_dir: Path):
        for png, doc in items:
            shutil.copy2(png, img_dir / png.name)
            yolo = to_yolo_lines(doc)
            (lbl_dir / (png.stem + ".txt")).write_text("\n".join(yolo) + ("\n" if yolo else ""))

    write_split(train, images_train, labels_train)
    write_split(val, images_val, labels_val)

    # data.yaml
    data_yaml = out / "data.yaml"
    data_yaml.write_text(
        f"path: {out}\n"
        f"train: images/train\n"
        f"val: images/val\n"
        f"names:\n"
        f"  0: {args.class_name}\n"
    )

    print(f"✓ Exported to: {out}")
    print(f"  Train: {len(train)} images")
    print(f"  Val:   {len(val)} images")
    print(f"  Skipped empty: {skipped_empty}")
    print(f"  data.yaml: {data_yaml}")
    print()
    print("Train hands-off with:")
    print(f"    tools/auto.sh '{data_yaml}'")
    return 0


if __name__ == "__main__":
    sys.exit(main())
