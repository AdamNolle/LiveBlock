#!/usr/bin/env python3
"""Create review-required label proposals with a CoreML or Ultralytics detector.

Proposals are never marked reviewed and therefore cannot enter the corpus until
a person verifies, edits, and sets `reviewed: true`.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import coremltools as ct
from PIL import Image

RUNTIME_CLASSES = ["Logo", "Ad banner", "Sponsored"]


def predict_coreml(model, image_path: Path, threshold: float) -> list[dict]:
    with Image.open(image_path) as image:
        result = model.predict({"image": image.convert("RGB")})
    confidence = result["confidence"]
    coordinates = result["coordinates"]
    boxes = []
    for scores, (cx, cy, width, height) in zip(confidence, coordinates):
        class_id = int(scores.argmax())
        score = float(scores[class_id])
        if class_id >= len(RUNTIME_CLASSES) or score < threshold:
            continue
        x = max(0.0, float(cx - width / 2))
        y = max(0.0, float(cy - height / 2))
        width = min(float(width), 1.0 - x)
        height = min(float(height), 1.0 - y)
        if width <= 0 or height <= 0:
            continue
        boxes.append({
            "class": RUNTIME_CLASSES[class_id],
            "confidence": round(score, 6),
            "height": round(height, 6),
            "placement": "unknown",
            "width": round(width, 6),
            "x": round(x, 6),
            "y": round(y, 6),
        })
    return boxes


def predict_yolo(model, image_path: Path, threshold: float) -> list[dict]:
    result = model(str(image_path), verbose=False, conf=threshold)[0]
    image_height, image_width = result.orig_shape
    boxes = []
    for box in result.boxes:
        class_id = int(box.cls[0].item())
        if class_id >= len(RUNTIME_CLASSES):
            continue
        x1, y1, x2, y2 = (float(value) for value in box.xyxy[0].tolist())
        boxes.append({
            "class": RUNTIME_CLASSES[class_id],
            "confidence": round(float(box.conf[0].item()), 6),
            "height": round((y2 - y1) / image_height, 6),
            "placement": "unknown",
            "width": round((x2 - x1) / image_width, 6),
            "x": round(x1 / image_width, 6),
            "y": round(y1 / image_height, 6),
        })
    return boxes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--pool", type=Path, required=True)
    parser.add_argument("--threshold", type=float, default=0.15)
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    if args.model.suffix == ".pt":
        from ultralytics import YOLO
        model = YOLO(str(args.model))
        predictor = predict_yolo
    else:
        model = ct.models.MLModel(str(args.model))
        predictor = predict_coreml
    count = 0
    for manifest in sorted(args.pool.rglob("manifest.jsonl")):
        for line in manifest.read_text().splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            image = manifest.parent / record["local_path"]
            labels = image.with_suffix(image.suffix + ".labels.json")
            if labels.exists() and not args.overwrite:
                continue
            proposals = predictor(model, image, args.threshold)
            labels.write_text(json.dumps({
                "boxes": proposals,
                "model": str(args.model),
                "reviewed": False,
            }, indent=2, sort_keys=True) + "\n")
            print(f"{image}: {len(proposals)} proposals")
            count += 1
    print(f"wrote {count} review-required proposal files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
