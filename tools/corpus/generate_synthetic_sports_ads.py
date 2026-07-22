#!/usr/bin/env python3
"""Generate deterministic sponsor-like overlays on licensed sports backgrounds.

Synthetic marks use invented names and geometric symbols, avoiding third-party
logo artwork. Background licensing/provenance is inherited in each manifest
record. All derivatives of one background retain its split_group.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
from datetime import datetime, timezone
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

WORDS = [
    "AERON", "APEX", "BOLT", "CIRRUS", "DRIVE", "ECHO", "FLUX", "FORGE",
    "KINETIC", "NOVA", "ORBIT", "PULSE", "QUANTA", "RIVET", "SUMMIT", "VANTA",
]
COLORS = ["#f44336", "#ffca28", "#29b6f6", "#66bb6a", "#ab47bc", "#ffffff", "#111111"]


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes())
    return digest.hexdigest()


def load_backgrounds(pool: Path) -> list[tuple[dict, Path]]:
    backgrounds = []
    for manifest in sorted(pool.rglob("manifest.jsonl")):
        for line in manifest.read_text().splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            path = manifest.parent / record["local_path"]
            if path.is_file():
                backgrounds.append((record, path))
    return backgrounds


def font_for(size: int, font_path: Path | None):
    if font_path and font_path.exists():
        return ImageFont.truetype(str(font_path), size=size)
    return ImageFont.load_default(size=size)


def placement_for(record: dict) -> str:
    context = record.get("depicted_context", "").casefold()
    if "jersey" in context or "football" in context:
        return "jersey"
    if "stadium" in context or "rink" in context or "board" in context:
        return "venue_board"
    return "car_livery"


def add_mark(image: Image.Image, rng: random.Random, class_name: str,
             font_path: Path | None) -> tuple[Image.Image, dict]:
    width, height = image.size
    if class_name == "Ad banner":
        patch_w = rng.randint(max(100, width // 5), max(120, width // 2))
        patch_h = rng.randint(max(35, height // 18), max(50, height // 8))
    elif class_name == "Sponsored":
        patch_w = rng.randint(max(90, width // 8), max(120, width // 4))
        patch_h = rng.randint(max(28, height // 24), max(40, height // 12))
    else:
        patch_w = rng.randint(max(45, width // 18), max(80, width // 5))
        patch_h = rng.randint(max(28, height // 30), max(45, height // 9))
    patch_w = min(patch_w, int(width * 0.7))
    patch_h = min(patch_h, int(height * 0.35))

    patch = Image.new("RGBA", (patch_w, patch_h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(patch)
    foreground = rng.choice(COLORS)
    background = rng.choice([color for color in COLORS if color != foreground])
    radius = max(2, patch_h // 8)
    draw.rounded_rectangle((0, 0, patch_w - 1, patch_h - 1), radius=radius,
                           fill=background, outline=foreground, width=max(1, patch_h // 18))
    word = rng.choice(WORDS)
    if class_name == "Sponsored":
        word = "SPONSORED " + word
    font = font_for(max(10, int(patch_h * 0.42)), font_path)
    box = draw.textbbox((0, 0), word, font=font)
    text_w, text_h = box[2] - box[0], box[3] - box[1]
    if text_w > patch_w - 12:
        font = font_for(max(8, int(patch_h * 0.30)), font_path)
        box = draw.textbbox((0, 0), word, font=font)
        text_w, text_h = box[2] - box[0], box[3] - box[1]
    draw.text(((patch_w - text_w) / 2, (patch_h - text_h) / 2 - box[1]), word,
              font=font, fill=foreground)
    if class_name == "Logo":
        symbol_size = max(4, patch_h // 6)
        draw.regular_polygon((symbol_size * 1.5, patch_h / 2, symbol_size), n_sides=rng.randint(3, 7),
                             fill=foreground)

    angle = rng.uniform(-18, 18)
    patch = patch.rotate(angle, expand=True, resample=Image.Resampling.BICUBIC)
    max_x = max(0, width - patch.width)
    max_y = max(0, height - patch.height)
    x = rng.randint(0, max_x) if max_x else 0
    y = rng.randint(int(height * 0.12), max_y) if max_y >= int(height * 0.12) else 0
    image.alpha_composite(patch, (x, y))
    return image, {
        "class": class_name,
        "height": patch.height / height,
        "width": patch.width / width,
        "x": x / width,
        "y": y / height,
    }


def generate(pool: Path, output: Path, count: int, seed: int, font_path: Path | None) -> dict:
    backgrounds = load_backgrounds(pool)
    if not backgrounds:
        raise ValueError(f"no backgrounds found below {pool}")
    output.mkdir(parents=True, exist_ok=True)
    manifest_path = output / "manifest.jsonl"
    manifest_path.write_text("")
    class_counts = {"Logo": 0, "Ad banner": 0, "Sponsored": 0}
    negatives = 0

    for index in range(count):
        rng = random.Random(seed + index)
        source_record, source_path = backgrounds[index % len(backgrounds)]
        with Image.open(source_path) as source:
            image = source.convert("RGBA")
            image.thumbnail((1600, 1200), Image.Resampling.LANCZOS)
        boxes = []
        if rng.random() < 0.15:
            negatives += 1
        else:
            for _ in range(rng.randint(1, 6)):
                choice = rng.random()
                class_name = "Logo" if choice < 0.72 else "Ad banner" if choice < 0.94 else "Sponsored"
                image, box = add_mark(image, rng, class_name, font_path)
                box["placement"] = placement_for(source_record)
                boxes.append(box)
                class_counts[class_name] += 1
        destination = output / f"synthetic-{index:05d}.jpg"
        image.convert("RGB").save(destination, quality=91, optimize=True)
        digest = file_sha256(destination)
        labels = destination.with_suffix(destination.suffix + ".labels.json")
        labels.write_text(json.dumps({
            "boxes": boxes,
            "generated": True,
            "reviewed": True,
        }, indent=2, sort_keys=True) + "\n")
        record = {
            "annotation_status": "reviewed_synthetic",
            "artist": source_record.get("artist", ""),
            "attribution": source_record.get("attribution", ""),
            "categories": ["synthetic_sports_ad"],
            "depicted_context": source_record.get("depicted_context", ""),
            "license": source_record["license"],
            "license_url": source_record.get("license_url", ""),
            "local_path": destination.name,
            "retrieved_at": datetime.now(timezone.utc).isoformat(),
            "sha256": digest,
            "source": "synthetic_derivative",
            "source_page": source_record.get("source_page", ""),
            "split_group": source_record.get("split_group") or source_record.get("source_page"),
            "synthetic_seed": seed + index,
        }
        with manifest_path.open("a") as handle:
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    return {"classes": class_counts, "images": count, "negatives": negatives}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pool", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--count", type=int, default=500)
    parser.add_argument("--seed", type=int, default=20260711)
    parser.add_argument("--font", type=Path,
                        default=Path("Sources/Resources/Fonts/Geist-Variable.ttf"))
    args = parser.parse_args()
    print(json.dumps(generate(args.pool, args.output, args.count, args.seed, args.font), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
