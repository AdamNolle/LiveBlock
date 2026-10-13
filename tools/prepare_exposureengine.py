#!/usr/bin/env python3
"""Convert ExposureEngine YOLO-OBB labels into LiveBlock's sponsor class.

All frames from one match stay in the same split. The upstream train/val/test
split puts frames from the same match on both sides and inflates validation.
Only image/label pairs are used; unmatched files are reported and excluded.
Original data: https://huggingface.co/datasets/SimulaMet-HOST/ExposureEngine
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import sys
from collections import defaultdict
from pathlib import Path


NAMES = ["ad", "sponsor_remove", "team_keep", "number_keep"]


def obb_to_yolo(line: str, source: Path) -> str:
    fields = line.split()
    if len(fields) != 9:
        raise ValueError(f"{source}: expected class id plus four OBB corners")
    try:
        int(fields[0])  # many brand ids collapse to sponsor_remove
        corners = [float(value) for value in fields[1:]]
    except ValueError as exc:
        raise ValueError(f"{source}: invalid OBB row") from exc
    if not all(math.isfinite(value) and 0 <= value <= 1 for value in corners):
        raise ValueError(f"{source}: OBB corner outside normalized image")
    xs, ys = corners[::2], corners[1::2]
    x0, x1 = min(xs), max(xs)
    y0, y1 = min(ys), max(ys)
    if x1 <= x0 or y1 <= y0:
        raise ValueError(f"{source}: empty OBB")
    return f"1 {(x0+x1)/2:.6f} {(y0+y1)/2:.6f} {x1-x0:.6f} {y1-y0:.6f}"


def collect(source: Path) -> tuple[dict[str, list[tuple[Path, list[str]]]], dict]:
    matches: dict[str, list[tuple[Path, list[str]]]] = defaultdict(list)
    stats = {"images_without_labels": 0, "labels_without_images": 0,
             "exact_duplicates": 0, "conflicting_duplicate_groups": 0,
             "invalid_label_images": 0, "duplicate_boxes_dropped": 0,
             "source_boxes": 0}
    candidates: dict[str, list[tuple[Path, list[str]]]] = defaultdict(list)
    for split in ("train", "val", "test"):
        image_dir = source / split / "images"
        label_dir = source / split / "labels"
        if not image_dir.is_dir() or not label_dir.is_dir():
            raise ValueError(f"Missing image/label directories under {source / split}")
        images = {p.stem: p for p in image_dir.iterdir()
                  if p.suffix.lower() in {".jpg", ".jpeg", ".png"}}
        labels = {p.stem: p for p in label_dir.glob("*.txt")}
        stats["images_without_labels"] += len(images.keys() - labels.keys())
        stats["labels_without_images"] += len(labels.keys() - images.keys())
        for stem in sorted(images.keys() & labels.keys()):
            image, label = images[stem], labels[stem]
            try:
                lines = [obb_to_yolo(row, label)
                         for row in label.read_text(encoding="utf-8").splitlines()
                         if row.strip()]
            except (UnicodeError, ValueError):
                stats["invalid_label_images"] += 1
                continue
            unique_lines = list(dict.fromkeys(lines))
            stats["duplicate_boxes_dropped"] += len(lines) - len(unique_lines)
            lines = unique_lines
            if not lines:
                continue
            digest = hashlib.sha256(image.read_bytes()).hexdigest()
            candidates[digest].append((image, lines))
    for versions in candidates.values():
        signatures = {tuple(sorted(lines)) for _, lines in versions}
        if len(signatures) > 1:
            stats["conflicting_duplicate_groups"] += 1
            continue
        image, lines = versions[0]
        stats["exact_duplicates"] += len(versions) - 1
        stats["source_boxes"] += len(lines)
        match_id = image.stem.split("_", 1)[0]
        matches[match_id].append((image, lines))
    return dict(matches), stats


def split_matches(matches: dict, seed: int) -> dict[str, list[str]]:
    ids = sorted(matches)
    if len(ids) < 3:
        raise ValueError("Need at least three distinct matches for train, val, test")
    random.Random(seed).shuffle(ids)
    total = sum(len(matches[match]) for match in ids)
    result: dict[str, list[str]] = {"train": [], "val": [], "test": []}
    counts = {split: 0 for split in result}
    for match in ids:
        if counts["test"] < max(1, total * .10):
            split = "test"
        elif counts["val"] < max(1, total * .15):
            split = "val"
        else:
            split = "train"
        result[split].append(match)
        counts[split] += len(matches[match])
    if any(not match_ids for match_ids in result.values()):
        raise ValueError("Split has no matches; add more source matches")
    return result


def prepare(source: Path, output: Path, seed: int) -> dict:
    source, output = source.resolve(), output.resolve()
    if output.exists():
        raise ValueError(f"Output already exists: {output}; choose a new path")
    matches, stats = collect(source)
    assignments = split_matches(matches, seed)
    output.mkdir(parents=True)
    report = {"source": str(source), "seed": seed, "classes": NAMES,
              "match_ids": assignments, "splits": {}, **stats}
    for split, match_ids in assignments.items():
        image_dir = output / "images" / split
        label_dir = output / "labels" / split
        image_dir.mkdir(parents=True)
        label_dir.mkdir(parents=True)
        count = 0
        boxes = 0
        for match in match_ids:
            for image, lines in matches[match]:
                target = image_dir / image.name
                if target.exists():
                    raise ValueError(f"Duplicate filename: {image.name}")
                try:
                    os.link(image, target)
                except OSError:
                    import shutil
                    shutil.copy2(image, target)
                (label_dir / f"{image.stem}.txt").write_text(
                    "\n".join(lines) + "\n", encoding="utf-8")
                count += 1
                boxes += len(lines)
        report["splits"][split] = {"images": count, "sponsor_boxes": boxes,
                                     "matches": len(match_ids)}
    yaml = [f"path: '{output.as_posix()}'", "train: images/train",
            "val: images/val", "test: images/test", "names:"]
    yaml.extend(f"  {index}: {name}" for index, name in enumerate(NAMES))
    (output / "data.yaml").write_text("\n".join(yaml) + "\n", encoding="utf-8")
    (output / "dataset_report.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()
    try:
        report = prepare(args.source, args.output, args.seed)
    except ValueError as exc:
        parser.error(str(exc))
    print(json.dumps(report["splits"], indent=2))
    print(f"YOLO dataset: {args.output.resolve() / 'data.yaml'}")
    print("Only sponsor_remove is labeled; team_keep and number_keep need separate data.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
