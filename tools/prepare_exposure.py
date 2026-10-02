#!/usr/bin/env python3
"""Prepare public soccer sponsor annotations with clip-grouped YOLO splits.

Source: https://huggingface.co/datasets/SimulaMet-HOST/ExposureEngine (MIT).
Oriented boxes become enclosing horizontal boxes, as required by LiveBlock.
"""
from __future__ import annotations
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import shutil
import urllib.request
import time

REPOSITORY = "SimulaMet-HOST/ExposureEngine"
REVISION = "a652cd0df28c889535adb0dfdd4aad30652cca6b"


def split_for_clip(stem: str) -> str:
    clip = stem.rsplit("_", 1)[0]
    bucket = int(hashlib.sha256(("liveblock-exposure-v1:"+clip).encode()).hexdigest()[:8], 16) % 100
    return "train" if bucket < 70 else "val" if bucket < 85 else "test"


def convert_labels(text: str) -> list[str]:
    result = []
    for row in text.splitlines():
        if not row.strip(): continue
        fields = [float(value) for value in row.split()]
        if len(fields) != 9 or not all(math.isfinite(v) for v in fields):
            raise ValueError("Expected a class ID and four finite normalized polygon corners")
        xs, ys = fields[1::2], fields[2::2]
        if min(xs+ys) < 0 or max(xs+ys) > 1:
            raise ValueError("Polygon outside normalized image bounds")
        x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
        if x1 <= x0 or y1 <= y0: raise ValueError("Degenerate sponsor polygon")
        result.append(f"0 {(x0+x1)/2:.6f} {(y0+y1)/2:.6f} {x1-x0:.6f} {y1-y0:.6f}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parent / "datasets/exposure/source")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent / "datasets/exposure/yolo")
    args = parser.parse_args()
    if args.output.exists(): parser.error(f"Output already exists: {args.output}")
    from huggingface_hub import HfApi
    files = [f for f in HfApi(token=False).list_repo_tree(REPOSITORY, repo_type="dataset", revision=REVISION, recursive=True)
             if hasattr(f, "size") and (f.path == "README.md" or f.path.endswith((".jpg", ".png", ".txt")))]
    def download(file):
        target = args.source / file.path
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            url = f"https://huggingface.co/datasets/{REPOSITORY}/resolve/{REVISION}/{file.path}"
            for attempt in range(4):
                try:
                    with urllib.request.urlopen(url, timeout=60) as response:
                        data = response.read()
                    break
                except (OSError, TimeoutError):
                    if attempt == 3: raise
                    time.sleep(2 ** attempt)
        else: data = target.read_bytes()
        if file.lfs:
            valid = hashlib.sha256(data).hexdigest() == file.lfs.sha256
        else:
            valid = hashlib.sha1(f"blob {len(data)}\0".encode()+data).hexdigest() == file.blob_id
        if len(data) != file.size or not valid: raise RuntimeError(f"Source checksum mismatch: {file.path}")
        if not target.exists(): target.write_bytes(data)
        return {"path": file.path, "sha256": hashlib.sha256(data).hexdigest()}
    with ThreadPoolExecutor(max_workers=8) as pool:
        verified = list(pool.map(download, files))
    print(f"Verified {len(verified)} public source files", flush=True)
    counts = {s: {"images": 0, "boxes": 0, "negatives": 0} for s in ("train", "val", "test")}
    for split in counts:
        for folder in ("images", "labels"): (args.output / folder / split).mkdir(parents=True)
    pages = []
    skipped = []
    invalid = []
    labels_by_stem = {}
    for label in sorted(args.source.glob("*/labels/*.txt")):
        if label.stem in labels_by_stem and label.read_bytes() != labels_by_stem[label.stem].read_bytes():
            raise ValueError(f"Conflicting source annotations: {label.stem}")
        labels_by_stem[label.stem] = label
    stems = set()
    for image in sorted(args.source.glob("*/images/*")):
        if image.suffix.lower() not in (".jpg", ".png"): continue
        if image.stem in stems: raise ValueError(f"Repeated frame across source splits: {image.stem}")
        stems.add(image.stem)
        label = labels_by_stem.get(image.stem)
        if label is None:
            skipped.append(str(image.relative_to(args.source)))
            continue
        try:
            rows = convert_labels(label.read_text())
        except ValueError as error:
            invalid.append({"image": str(image.relative_to(args.source)), "annotation": str(label.relative_to(args.source)), "reason": str(error)})
            continue
        split = split_for_clip(image.stem)
        shutil.copy2(image, args.output / "images" / split / image.name)
        (args.output / "labels" / split / label.name).write_text("\n".join(rows)+("\n" if rows else ""))
        counts[split]["images"] += 1; counts[split]["boxes"] += len(rows); counts[split]["negatives"] += not rows
        pages.append({"frame": image.stem, "clip": image.stem.rsplit("_",1)[0], "split": split})
    if any(c["boxes"] == 0 for c in counts.values()): raise RuntimeError("A grouped split has no sponsor boxes")
    report = {"source": f"https://huggingface.co/datasets/{REPOSITORY}", "revision": REVISION,
              "source_license": "MIT (dataset card)", "changes": "OBB polygons enclosed as horizontal sponsor_remove boxes; SHA-256 clip-grouped splits; negatives retained",
              "counts": counts, "frames": pages, "unannotated_frames_excluded": skipped, "invalid_frames_excluded": invalid, "verified_files": verified}
    (args.output / "provenance.json").write_text(json.dumps(report, indent=2)+"\n")
    (args.output / "ATTRIBUTION.md").write_text("# ExposureEngine attribution\n\nMehdi Houshmand Sarkhoosh et al. ExposureEngine: Oriented Logo Detection and Sponsor Visibility Analytics in Sports Broadcasts (2025). https://arxiv.org/abs/2510.04739\n\nSource: "+report["source"]+"\n\nLicense: MIT, as declared by the source dataset card (preserved in source/README.md).\n\nChanges: "+report["changes"]+".\n\nHorizontal rectangles include more surrounding pixels than the original polygons. This dataset does not annotate team emblems or jersey numbers as protection classes.\n")
    (args.output / "data.yaml").write_text(f"path: {json.dumps(str(args.output.resolve()))}\ntrain: images/train\nval: images/val\ntest: images/test\nnames:\n  0: sponsor_remove\n")
    print(json.dumps(counts, indent=2)); print(args.output / "data.yaml")
    return 0


if __name__ == "__main__": raise SystemExit(main())
