#!/usr/bin/env python3
"""Export the in-app labeling JSON files to a MULTI-CLASS YOLO dataset.

Reads from the per-user LiveBlock training dir (resolved per-OS by
`tools/lb_paths.py`):
   <app-support>/training/screenshots/*.png
   <app-support>/training/labels/*.json

…and writes a YOLO-formatted dataset suitable for `tools/train_logos.py`:
   <app-support>/training/exports/<run-name>/
       images/train/  images/val/
       labels/train/  labels/val/
       data.yaml          # multi-class names list

MULTI-CLASS mapping (from each label box's `class` field — see
liveblock_labels::LabelClass which serializes to these strings):

    JSON "class"        YOLO class id   name           intent
    --------------      -------------   ------------    ------
    "ad"   (or absent)  0               ad             REMOVE  (legacy default)
    "sponsor_remove"    1               sponsor_remove  REMOVE
    "team_keep"         2               team_keep      KEEP
    "number_keep"       3               number_keep    KEEP

Backward-compat: boxes written by the legacy single-class app have no `class`
key. They deserialize as "ad" -> class id 0, byte-for-byte the same as the old
hardcoded single-class export. So old datasets re-export identically.

The KEEP classes (team_keep / number_keep) are deliberately *trained* — the
detector learns to localize them so the on-device pipeline can CARVE THEM OUT
of the remove-mask (emblems/numbers must never be erased). See
`build_remove_mask_from_tracks` in liveblock-core.

  Usage:
    tools/.venv/bin/python tools/export_labels.py
    tools/.venv/bin/python tools/export_labels.py --val-split 0.15 --name my-export
    tools/.venv/bin/python tools/export_labels.py --include-empty
    tools/.venv/bin/python tools/export_labels.py --binary    # collapse to remove-only
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Tuple

# Local helper (same directory). Importable because the native apps invoke this
# with cwd at the repo root *and* via an absolute path; sys.path[0] is the
# script dir either way, but be explicit to be safe.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import lb_paths  # noqa: E402

DEFAULT_ROOT = lb_paths.training_root()

# Canonical multi-class taxonomy. Order defines the YOLO class id.
# These string keys MUST match liveblock_labels::LabelClass serde renames.
CLASS_ORDER: List[str] = ["ad", "sponsor_remove", "team_keep", "number_keep"]
CLASS_TO_ID: Dict[str, int] = {name: i for i, name in enumerate(CLASS_ORDER)}

# Which classes mean "remove the pixels" (vs. "keep / protect").
REMOVE_CLASSES = {"ad", "sponsor_remove"}
KEEP_CLASSES = {"team_keep", "number_keep"}

# In --binary mode we collapse everything to a single legacy "ad" remove class
# and DROP keep boxes entirely (so the model never localizes protected content).
BINARY_CLASS_ORDER: List[str] = ["ad"]


def discover(root: Path) -> List[Tuple[Path, Path]]:
    """Return list of (image_path, label_path) for fully labeled screenshots."""
    images = root / "screenshots"
    labels = root / "labels"
    if not images.is_dir():
        sys.exit(
            f"No screenshots directory: {images}\n"
            f"(resolved from {root}; capture screenshots in LiveBlock first.)"
        )
    pairs = []
    for png in sorted(images.glob("*.png")):
        json_path = labels / (png.stem + ".json")
        if json_path.exists():
            pairs.append((png, json_path))
    return pairs


def box_class_name(box: dict) -> str:
    """Resolve a box's class string, defaulting to legacy 'ad' when absent.

    Mirrors liveblock_labels: a missing/empty `class` key == LabelClass::Ad.
    Unknown classes are rejected: guessing "remove" could train the model to
    erase an identity mark that was meant to be kept.
    """
    raw = box.get("class")
    if raw is None or raw == "":
        return "ad"
    if isinstance(raw, str) and raw in CLASS_TO_ID:
        return raw
    raise ValueError(f"unknown box class {raw!r}; expected one of {CLASS_ORDER}")


def validate_document(png: Path, doc: dict) -> None:
    """Reject malformed labels before they can contaminate a training run."""
    if not isinstance(doc, dict) or doc.get("image") != png.name:
        raise ValueError(f"{png.name}: label image filename does not match screenshot")
    if not isinstance(doc.get("imageWidth"), int) or doc["imageWidth"] <= 0:
        raise ValueError(f"{png.name}: invalid imageWidth")
    if not isinstance(doc.get("imageHeight"), int) or doc["imageHeight"] <= 0:
        raise ValueError(f"{png.name}: invalid imageHeight")
    if not isinstance(doc.get("boxes"), list):
        raise ValueError(f"{png.name}: boxes must be an array")
    for index, box in enumerate(doc["boxes"]):
        if not isinstance(box, dict):
            raise ValueError(f"{png.name}: box {index} must be an object")
        box_class_name(box)
        try:
            x, y, w, h = (float(box[key]) for key in ("x", "y", "width", "height"))
        except (KeyError, TypeError, ValueError) as exc:
            raise ValueError(f"{png.name}: box {index} has invalid coordinates") from exc
        if not all(math.isfinite(v) for v in (x, y, w, h)):
            raise ValueError(f"{png.name}: box {index} has non-finite coordinates")
        if x < 0 or y < 0 or w <= 0 or h <= 0 or x + w > 1.000001 or y + h > 1.000001:
            raise ValueError(f"{png.name}: box {index} is empty or outside the image")


def to_yolo_lines(label_doc: dict, class_to_id: Dict[str, int], *, binary: bool) -> List[str]:
    """Convert one in-app label JSON document to YOLO-format lines.

    YOLO row: `class_id cx cy w h`, all coords normalized [0..1], center-origin.
    The in-app JSON stores top-left x/y + w/h (also normalized), so we convert
    the origin here. In --binary mode we drop keep boxes and map removes -> 0.
    """
    lines: List[str] = []
    for box in label_doc.get("boxes", []):
        name = box_class_name(box)
        if binary:
            if name in KEEP_CLASSES:
                continue  # never localize protected content in binary mode
            class_id = 0  # single legacy "ad" remove class
        else:
            class_id = class_to_id[name]

        # in-app JSON: top-left origin, normalized. YOLO wants center origin.
        x = float(box["x"])
        y = float(box["y"])
        w = float(box["width"])
        h = float(box["height"])
        cx = x + w / 2.0
        cy = y + h / 2.0
        lines.append(f"{class_id} {cx:.6f} {cy:.6f} {w:.6f} {h:.6f}")
    return lines


def render_data_yaml(out: Path, names: List[str]) -> str:
    """Produce a YOLO data.yaml body with an indexed `names:` mapping."""
    lines = [
        f"path: {out}",
        "train: images/train",
        "val: images/val",
        "names:",
    ]
    for i, name in enumerate(names):
        lines.append(f"  {i}: {name}")
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    ap.add_argument(
        "--root",
        type=Path,
        default=DEFAULT_ROOT,
        help="Training root (default: per-OS LiveBlock training dir)",
    )
    ap.add_argument(
        "--name",
        type=str,
        default=None,
        help="Export folder name (default: auto-generated timestamp)",
    )
    ap.add_argument(
        "--val-split",
        type=float,
        default=0.15,
        help="Fraction held out for validation (default 0.15)",
    )
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--exclude-empty", action="store_true",
                    help="Drop no-ad screenshots (normally included as negative examples).")
    ap.add_argument("--include-empty", action="store_true",
                    help="Compatibility flag; no-ad screenshots are included by default.")
    ap.add_argument(
        "--binary",
        action="store_true",
        help="Collapse to the legacy single 'ad' remove class (drops keep boxes). "
        "Use for backward-compat with old single-class consumers.",
    )
    args = ap.parse_args()

    root = args.root.expanduser().resolve()
    pairs = discover(root)
    if not pairs:
        sys.exit("No labeled screenshots found. Open the Labeling window in LiveBlock first.")

    # Build the class list / id map for this export.
    if args.binary:
        names = list(BINARY_CLASS_ORDER)
        class_to_id = {"ad": 0}
    else:
        names = list(CLASS_ORDER)
        class_to_id = dict(CLASS_TO_ID)

    if not 0 < args.val_split < 1:
        sys.exit("--val-split must be between 0 and 1.")

    # Exact duplicate screenshots must stay in one split. Retain a single copy
    # only when its annotations match, otherwise fail for human review.
    kept: List[Tuple[Path, dict]] = []
    seen: Dict[str, Tuple[Path, str]] = {}
    skipped_empty = 0
    duplicates = 0
    class_counts: Dict[str, int] = {n: 0 for n in CLASS_ORDER}
    for png, json_path in pairs:
        try:
            doc = json.loads(json_path.read_text(encoding="utf-8"))
            validate_document(png, doc)
        except (json.JSONDecodeError, ValueError) as e:
            sys.exit(f"Invalid labels in {json_path}: {e}")
        boxes = doc.get("boxes", [])
        if not boxes and args.exclude_empty:
            skipped_empty += 1
            continue
        digest = hashlib.sha256(png.read_bytes()).hexdigest()
        signature = json.dumps(sorted(to_yolo_lines(doc, class_to_id, binary=args.binary)))
        if digest in seen:
            prior_path, prior_signature = seen[digest]
            if signature != prior_signature:
                sys.exit(f"Conflicting labels on identical screenshots: {prior_path} and {png}")
            duplicates += 1
            continue
        seen[digest] = (png, signature)
        for box in boxes:
            class_counts[box_class_name(box)] += 1
        kept.append((png, doc))

    if len(kept) < 2:
        sys.exit("Need at least two distinct labeled screenshots for train and validation.")
    if not any(to_yolo_lines(doc, class_to_id, binary=args.binary) for _, doc in kept):
        sys.exit("All screenshots are negative examples. Label at least one removable ad or sponsor.")

    # Split
    random.Random(args.seed).shuffle(kept)
    val_n = max(1, int(len(kept) * args.val_split)) if len(kept) > 1 else 0
    val = kept[:val_n]
    train = kept[val_n:]

    # Every class must occur in training. A rare class assigned only to val
    # would otherwise be impossible for the detector to learn.
    all_classes = {box_class_name(b) for _, doc in kept for b in doc["boxes"]}
    train_classes = {box_class_name(b) for _, doc in train for b in doc["boxes"]}
    for missing in sorted(all_classes - train_classes):
        match = next((item for item in val if any(box_class_name(b) == missing
                                                    for b in item[1]["boxes"])), None)
        if match is None:
            sys.exit(f"Class {missing} is absent from training; add more examples.")
        for replacement in train:
            candidate_train = [item for item in train if item is not replacement] + [match]
            candidate_classes = {box_class_name(b) for _, doc in candidate_train
                                 for b in doc["boxes"]}
            if candidate_classes == all_classes:
                train.remove(replacement)
                val.remove(match)
                train.append(match)
                val.append(replacement)
                break
        else:
            sys.exit(f"Cannot make a train/validation split covering {missing}; add more examples.")

    # Output structure
    name = args.name or "export-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    out = root / "exports" / name
    images_train = out / "images" / "train"
    images_val = out / "images" / "val"
    labels_train = out / "labels" / "train"
    labels_val = out / "labels" / "val"
    for d in (images_train, images_val, labels_train, labels_val):
        d.mkdir(parents=True, exist_ok=True)

    def write_split(items, img_dir: Path, lbl_dir: Path):
        for png, doc in items:
            shutil.copy2(png, img_dir / png.name)
            yolo = to_yolo_lines(doc, class_to_id, binary=args.binary)
            (lbl_dir / (png.stem + ".txt")).write_text(
                "\n".join(yolo) + ("\n" if yolo else ""), encoding="utf-8"
            )

    write_split(train, images_train, labels_train)
    write_split(val, images_val, labels_val)

    # data.yaml
    data_yaml = out / "data.yaml"
    data_yaml.write_text(render_data_yaml(out, names), encoding="utf-8")

    report = {
        "schema_version": 1,
        "seed": args.seed,
        "train_images": len(train),
        "val_images": len(val),
        "negative_images": sum(not doc["boxes"] for _, doc in kept),
        "duplicate_images_dropped": duplicates,
        "classes": names,
        "box_counts": class_counts,
    }
    (out / "dataset_report.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    print(f"OK Exported to: {out}")
    print(f"  Train: {len(train)} images")
    print(f"  Val:   {len(val)} images")
    print(f"  Skipped empty: {skipped_empty}")
    print(f"  Negative images: {report['negative_images']}")
    print(f"  Exact duplicates dropped: {duplicates}")
    print(f"  Classes: {', '.join(f'{i}:{n}' for i, n in enumerate(names))}")
    if not args.binary:
        remove_n = sum(class_counts[c] for c in REMOVE_CLASSES)
        keep_n = sum(class_counts[c] for c in KEEP_CLASSES)
        print(
            "  Box counts: "
            + ", ".join(f"{n}={class_counts[n]}" for n in CLASS_ORDER)
            + f"  (remove={remove_n}, keep={keep_n})"
        )
        if keep_n == 0:
            print(
                "  Note: no team_keep/number_keep boxes found. The model can still "
                "be trained, but it won't learn to PROTECT identity marks until you "
                "label some. (Multi-class mapping still applied.)"
            )
    print(f"  data.yaml: {data_yaml}")
    print()
    print("Train hands-off with:")
    if sys.platform == "darwin":
        print(f"    tools/auto.sh '{data_yaml}'")
    elif sys.platform.startswith("linux"):
        print(f"    tools/auto.sh '{data_yaml}'")
    else:
        print(f"    pwsh -File tools/auto.ps1 '{data_yaml}'")
    return 0


if __name__ == "__main__":
    sys.exit(main())
