#!/usr/bin/env python3
"""Export a YOLO corpus split into LiveBlock's pixel-box eval format."""
from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path

from PIL import Image
import yaml

IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}


def export(corpus: Path, output: Path, split: str = "test") -> dict:
    data = yaml.safe_load((corpus / "data.yaml").read_text())
    names = {int(key): value for key, value in data["names"].items()}
    image_dir = corpus / "images" / split
    label_dir = corpus / "labels" / split
    annotation_path = corpus / "annotations.jsonl"
    annotations = {}
    if annotation_path.exists():
        annotations = {
            record["corpus_path"]: record
            for record in (json.loads(line) for line in annotation_path.read_text().splitlines() if line.strip())
        }
    if not image_dir.is_dir():
        raise FileNotFoundError(image_dir)
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)

    images = 0
    boxes_total = 0
    negatives = 0
    preserve_regions_total = 0
    for source in sorted(image_dir.iterdir()):
        if source.suffix.lower() not in IMAGE_EXTENSIONS:
            continue
        with Image.open(source) as image:
            width, height = image.size
        destination = output / source.name
        shutil.copy2(source, destination)
        yolo_path = label_dir / f"{source.stem}.txt"
        boxes = []
        negative_placements = []
        preserve_regions = []
        corpus_path = str(source.relative_to(corpus))
        annotation = annotations.get(corpus_path)
        if annotation is not None:
            negative_placements = annotation.get("negative_placements", [])
            for item in annotation.get("boxes", []):
                x, y, box_width, box_height = item["box"]
                class_id = int(item["class_id"])
                boxes.append({
                    "box": [x * width, y * height, box_width * width, box_height * height],
                    "class_id": class_id,
                    "class_name": names[class_id],
                    "placement": item.get("placement", "unknown"),
                })
            for region in annotation.get("preserve_regions", []):
                x, y, box_width, box_height = region["box"]
                preserve_regions.append({
                    "box": [x * width, y * height, box_width * width, box_height * height],
                    "kind": region["kind"],
                })
        elif yolo_path.exists():
            for line in yolo_path.read_text().splitlines():
                if not line.strip():
                    continue
                class_id_text, cx_text, cy_text, width_text, height_text = line.split()
                class_id = int(class_id_text)
                cx, cy = float(cx_text), float(cy_text)
                box_width, box_height = float(width_text), float(height_text)
                boxes.append({
                    "box": [
                        (cx - box_width / 2) * width,
                        (cy - box_height / 2) * height,
                        box_width * width,
                        box_height * height,
                    ],
                    "class_id": class_id,
                    "class_name": names[class_id],
                    "placement": "unknown",
                })
        sidecar = destination.with_suffix(".json")
        sidecar.write_text(json.dumps({
            "boxes": boxes,
            "negative": not boxes,
            "negative_placements": negative_placements,
            "preserve_regions": preserve_regions,
            "source_split": split,
        }, indent=2, sort_keys=True) + "\n")
        images += 1
        boxes_total += len(boxes)
        negatives += int(not boxes)
        preserve_regions_total += len(preserve_regions)
    stats = {
        "boxes": boxes_total,
        "images": images,
        "negatives": negatives,
        "preserve_regions": preserve_regions_total,
        "split": split,
    }
    (output / "fixture-stats.json").write_text(json.dumps(stats, indent=2, sort_keys=True) + "\n")
    return stats


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--split", default="test")
    args = parser.parse_args()
    print(json.dumps(export(args.corpus, args.output, args.split), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
