#!/usr/bin/env python3
"""Validate annotated sports-ad images and build a leakage-resistant YOLO corpus.

Input directories contain manifest.jsonl records and optional <image>.labels.json:
{
  "reviewed": true,
  "boxes": [
    {"class": "Logo", "x": 0.1, "y": 0.2, "width": 0.2, "height": 0.1,
     "placement": "jersey"}
  ]
}

Empty boxes are retained as hard negatives. Splits are assigned by split_group,
not individual image, preventing adjacent video frames or one photo source from
leaking across train/validation/test.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

from PIL import Image, ImageStat
import yaml

from promotion_contract import (
    REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
    REQUIRED_PROMOTION_PLACEMENTS,
    REQUIRED_PROMOTION_PRESERVATION_KINDS,
)

CLASSES = {"Logo": 0, "Ad banner": 1, "Sponsored": 2}
PRESERVATION_KINDS = {
    "team_name", "jersey_number", "vehicle_number", "team_crest", "manufacturer_badge",
}
PLACEMENT_KINDS = {
    "car_livery", "jersey", "venue_board", "broadcast_overlay", "ordinary_screen", "helmet",
}
ALLOWED_LICENSES = {
    "cc0", "public domain", "pd", "cc by 2.0", "cc by 3.0", "cc by 4.0", "mit",
}


def normalized_license(value: str) -> str:
    return value.casefold().replace("creative commons ", "cc ").strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def dhash(path: Path) -> str:
    with Image.open(path) as image:
        grayscale = image.convert("L").resize((9, 8))
        flattened = getattr(grayscale, "get_flattened_data", grayscale.getdata)
        pixels = list(flattened())
        mean = tuple(round(channel / 16) for channel in ImageStat.Stat(image.convert("RGB")).mean)
    bits = [pixels[row * 9 + col] > pixels[row * 9 + col + 1]
            for row in range(8) for col in range(8)]
    value = sum(int(bit) << index for index, bit in enumerate(bits))
    # Mean colour prevents all uniform images from collapsing to the same dHash.
    return f"{value:016x}-{'-'.join(map(str, mean))}"


def split_for(group: str, val_fraction: float, test_fraction: float) -> str:
    bucket = int(hashlib.sha256(group.encode()).hexdigest()[:8], 16) / 0xFFFFFFFF
    if bucket < test_fraction:
        return "test"
    if bucket < test_fraction + val_fraction:
        return "val"
    return "train"


def validate_normalized_rect(region: dict, source: Path) -> None:
    values = [float(region[key]) for key in ("x", "y", "width", "height")]
    x, y, width, height = values
    if not (0 <= x < 1 and 0 <= y < 1 and 0 < width <= 1 and 0 < height <= 1
            and x + width <= 1.000001 and y + height <= 1.000001):
        raise ValueError(f"{source}: invalid normalized box {values}")


def validate_box(box: dict, source: Path) -> tuple[int, str]:
    class_name = box.get("class")
    if class_name not in CLASSES:
        raise ValueError(f"{source}: unknown class {class_name!r}; expected {list(CLASSES)}")
    validate_normalized_rect(box, source)
    placement = box.get("placement")
    if placement not in PLACEMENT_KINDS:
        raise ValueError(
            f"{source}: unknown placement {placement!r}; expected {sorted(PLACEMENT_KINDS)}"
        )
    return CLASSES[class_name], placement


def validate_preserve_region(region: dict, source: Path) -> str:
    kind = region.get("kind")
    if kind not in PRESERVATION_KINDS:
        raise ValueError(
            f"{source}: unknown preserve kind {kind!r}; expected {sorted(PRESERVATION_KINDS)}"
        )
    validate_normalized_rect(region, source)
    return kind


def preserved_region_coverage(box: dict, region: dict) -> float:
    """Fraction of a preservation region covered by a sponsor target box."""
    bx, by, bw, bh = (float(box[key]) for key in ("x", "y", "width", "height"))
    rx, ry, rw, rh = (float(region[key]) for key in ("x", "y", "width", "height"))
    intersection_width = min(bx + bw, rx + rw) - max(bx, rx)
    intersection_height = min(by + bh, ry + rh) - max(by, ry)
    if intersection_width <= 0 or intersection_height <= 0:
        return 0.0
    return intersection_width * intersection_height / (rw * rh)


def yolo_line(class_id: int, box: dict) -> str:
    x, y = float(box["x"]), float(box["y"])
    width, height = float(box["width"]), float(box["height"])
    return f"{class_id} {x + width / 2:.6f} {y + height / 2:.6f} {width:.6f} {height:.6f}"


def load_records(root: Path) -> list[tuple[dict, Path, Path]]:
    records = []
    for manifest in sorted(root.rglob("manifest.jsonl")):
        for line_number, line in enumerate(manifest.read_text().splitlines(), 1):
            if not line.strip():
                continue
            record = json.loads(line)
            image = manifest.parent / record["local_path"]
            labels = image.with_suffix(image.suffix + ".labels.json")
            if not image.is_file():
                raise FileNotFoundError(f"{manifest}:{line_number}: missing {image}")
            records.append((record, image, labels))
    return records


def build(root: Path, output: Path, val_fraction: float, test_fraction: float,
          review_methods: set[str] | None = None,
          required_test_placements: set[str] | None = None,
          stratify_placements: set[str] | None = None,
          stratify_preservation_kinds: set[str] | None = None,
          stratify_negative_placements: set[str] | None = None) -> dict:
    records = load_records(root)
    if not records:
        raise ValueError(f"no manifest records found beneath {root}")

    accepted = []
    seen_sha: set[str] = set()
    seen_dhash: set[str] = set()
    skipped = Counter()
    for record, image, labels_path in records:
        if normalized_license(record.get("license", "")) not in ALLOWED_LICENSES:
            skipped["license"] += 1
            continue
        actual_sha = sha256(image)
        if actual_sha != record.get("sha256"):
            raise ValueError(f"checksum mismatch: {image}")
        image_dhash = dhash(image)
        if actual_sha in seen_sha or image_dhash in seen_dhash:
            skipped["duplicate"] += 1
            continue
        if not labels_path.is_file():
            skipped["unannotated"] += 1
            continue
        labels_doc = json.loads(labels_path.read_text())
        if labels_doc.get("reviewed") is not True:
            skipped["unreviewed"] += 1
            continue
        review_method = str(labels_doc.get("review_method", "unspecified"))
        if review_method == "human":
            reviewer = str(labels_doc.get("reviewed_by", "")).strip()
            reviewed_at = str(labels_doc.get("reviewed_at", "")).strip()
            try:
                parsed_reviewed_at = datetime.fromisoformat(reviewed_at.replace("Z", "+00:00"))
            except ValueError:
                parsed_reviewed_at = None
            if not reviewer or parsed_reviewed_at is None or parsed_reviewed_at.tzinfo is None:
                skipped["human_review_provenance"] += 1
                continue
            if labels_doc.get("excluded") is True:
                skipped["reviewer_excluded"] += 1
                continue
        if review_methods is not None and review_method not in review_methods:
            skipped["review_method"] += 1
            continue
        boxes = labels_doc.get("boxes", [])
        placements = []
        for box in boxes:
            _, placement = validate_box(box, labels_path)
            placements.append(placement)
        negative_placements = labels_doc.get("negative_placements", [])
        if not isinstance(negative_placements, list) or any(
            value not in PLACEMENT_KINDS for value in negative_placements
        ):
            raise ValueError(
                f"{labels_path}: negative_placements must use {sorted(PLACEMENT_KINDS)}"
            )
        record = {**record, "negative_placements": sorted(set(negative_placements))}
        preserve_regions = labels_doc.get("preserve_regions", [])
        for region in preserve_regions:
            validate_preserve_region(region, labels_path)
            for box in boxes:
                if preserved_region_coverage(box, region) >= 0.5:
                    raise ValueError(
                        f"{labels_path}: sponsor box contradicts preserve region {region['kind']!r}"
                    )
        seen_sha.add(actual_sha)
        seen_dhash.add(image_dhash)
        accepted.append((record, image, boxes, placements, image_dhash, review_method,
                         preserve_regions))

    if not accepted:
        method_note = f" matching review methods {sorted(review_methods)}" if review_methods else ""
        raise ValueError(
            "no reviewed, licensed, non-duplicate images with valid labels" + method_note
        )

    if output.exists():
        shutil.rmtree(output)
    for split in ("train", "val", "test"):
        (output / "images" / split).mkdir(parents=True)
        (output / "labels" / split).mkdir(parents=True)

    counts = Counter()
    class_counts = Counter()
    placement_counts = Counter()
    review_method_counts = Counter()
    preservation_counts = Counter()
    negative_placement_counts = Counter()
    placement_split_counts: dict[str, Counter] = defaultdict(Counter)
    negative_placement_split_counts: dict[str, Counter] = defaultdict(Counter)
    preservation_split_counts: dict[str, Counter] = defaultdict(Counter)
    provenance = []
    annotations = []

    def record_group(item) -> str:
        record = item[0]
        return record.get("split_group") or record.get("source_page") or record["sha256"]

    groups = sorted({record_group(item) for item in accepted},
                    key=lambda group: hashlib.sha256(group.encode()).hexdigest())
    test_n = max(1, round(len(groups) * test_fraction)) if test_fraction > 0 and len(groups) >= 3 else 0
    val_n = max(1, round(len(groups) * val_fraction)) if val_fraction > 0 and len(groups) >= 3 else 0
    while test_n + val_n >= len(groups):
        if val_n >= test_n and val_n > 0:
            val_n -= 1
        elif test_n > 0:
            test_n -= 1

    group_placements: dict[str, set[str]] = defaultdict(set)
    group_negative_placements: dict[str, set[str]] = defaultdict(set)
    group_preservation_kinds: dict[str, set[str]] = defaultdict(set)
    for item in accepted:
        group = record_group(item)
        group_placements[group].update(item[3])
        group_negative_placements[group].update(item[0].get("negative_placements", []))
        group_preservation_kinds[group].update(region["kind"] for region in item[6])
    test_groups: set[str] = set()
    val_groups: set[str] = set()
    protected_train_groups: set[str] = set()
    facet_groups = {
        **{("placement", value): group_placements for value in (stratify_placements or set())},
        **{("preservation kind", value): group_preservation_kinds
           for value in (stratify_preservation_kinds or set())},
        **{("negative placement", value): group_negative_placements
           for value in (stratify_negative_placements or set())},
    }
    facet_candidates = {
        facet: [group for group in groups if facet[1] in group_values[group]]
        for facet, group_values in facet_groups.items()
    }
    # Reserve scarce slices first so a broad slice (for example car_livery)
    # cannot consume the only viable identity/jersey/venue groups.
    for (facet_name, value), candidates in sorted(
        facet_candidates.items(), key=lambda item: (len(item[1]), item[0])
    ):
        group_values = facet_groups[(facet_name, value)]
        required_groups = 1 + int(test_n > 0) + int(val_n > 0)
        if len(candidates) < required_groups:
            raise ValueError(
                f"{facet_name} {value!r} needs {required_groups} source groups for "
                f"train/val/test stratification; found {len(candidates)}"
            )
        train_candidate = next(
            (group for group in candidates if group in protected_train_groups),
            None,
        ) or next(
            (group for group in reversed(candidates)
             if group not in test_groups and group not in val_groups),
            None,
        )
        if train_candidate is None:
            raise ValueError(f"cannot reserve a training group for {facet_name} {value!r}")
        protected_train_groups.add(train_candidate)
        if test_n and not any(value in group_values[group] for group in test_groups):
            candidate = next(
                (group for group in candidates
                 if group not in val_groups and group not in protected_train_groups),
                None,
            )
            if candidate is None:
                raise ValueError(f"cannot reserve a test group for {facet_name} {value!r}")
            test_groups.add(candidate)
        if val_n and not any(value in group_values[group] for group in val_groups):
            candidate = next(
                (group for group in candidates
                 if group not in test_groups and group not in protected_train_groups),
                None,
            )
            if candidate is None:
                raise ValueError(f"cannot reserve a validation group for {facet_name} {value!r}")
            val_groups.add(candidate)

    test_target = max(test_n, len(test_groups))
    val_target = max(val_n, len(val_groups))
    if test_target + val_target >= len(groups):
        raise ValueError("stratification leaves no training groups")
    for group in groups:
        if (len(test_groups) < test_target and group not in val_groups
                and group not in protected_train_groups):
            test_groups.add(group)
    for group in groups:
        if (len(val_groups) < val_target and group not in test_groups
                and group not in protected_train_groups):
            val_groups.add(group)
    group_splits = {
        group: "test" if group in test_groups else "val" if group in val_groups else "train"
        for group in groups
    }

    for record, image, boxes, placements, image_dhash, review_method, preserve_regions in accepted:
        group = record_group((record, image, boxes, placements, image_dhash, review_method,
                              preserve_regions))
        split = group_splits[group]
        stem = record["sha256"][:20]
        destination = output / "images" / split / f"{stem}{image.suffix.lower()}"
        shutil.copy2(image, destination)
        lines = []
        for box in boxes:
            class_id, _ = validate_box(box, image)
            lines.append(yolo_line(class_id, box))
            class_counts[box["class"]] += 1
        (output / "labels" / split / f"{stem}.txt").write_text("\n".join(lines) + ("\n" if lines else ""))
        placement_counts.update(placements)
        preservation_counts.update(region["kind"] for region in preserve_regions)
        negative_placement_counts.update(record.get("negative_placements", []))
        negative_placement_split_counts[split].update(record.get("negative_placements", []))
        preservation_split_counts[split].update(region["kind"] for region in preserve_regions)
        placement_split_counts[split].update(placements)
        review_method_counts[review_method] += 1
        counts[split] += 1
        corpus_path = str(destination.relative_to(output))
        provenance.append({
            **record,
            "corpus_path": corpus_path,
            "dhash": image_dhash,
            "split": split,
        })
        annotations.append({
            "boxes": [{
                "box": [float(box[key]) for key in ("x", "y", "width", "height")],
                "class_id": CLASSES[box["class"]],
                "class_name": box["class"],
                "placement": box.get("placement", "unknown"),
            } for box in boxes],
            "corpus_path": corpus_path,
            "negative_placements": record.get("negative_placements", []),
            "review_method": review_method,
            "preserve_regions": [{
                "box": [float(region[key]) for key in ("x", "y", "width", "height")],
                "kind": region["kind"],
            } for region in preserve_regions],
            "split": split,
        })

    data = {
        "path": str(output.resolve()),
        "train": "images/train",
        "val": "images/val",
        "test": "images/test",
        "names": {value: key for key, value in CLASSES.items()},
    }
    (output / "data.yaml").write_text(yaml.safe_dump(data, sort_keys=False))
    with (output / "provenance.jsonl").open("w") as handle:
        for record in sorted(provenance, key=lambda item: item["sha256"]):
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    with (output / "annotations.jsonl").open("w") as handle:
        for record in sorted(annotations, key=lambda item: item["corpus_path"]):
            handle.write(json.dumps(record, sort_keys=True) + "\n")
    stats = {
        "classes": dict(sorted(class_counts.items())),
        "images": dict(sorted(counts.items())),
        "negative_placements": dict(sorted(negative_placement_counts.items())),
        "negative_placements_by_split": {
            split: dict(sorted(values.items()))
            for split, values in sorted(negative_placement_split_counts.items())
        },
        "placements": dict(sorted(placement_counts.items())),
        "preservation_regions": dict(sorted(preservation_counts.items())),
        "preservation_regions_by_split": {
            split: dict(sorted(values.items()))
            for split, values in sorted(preservation_split_counts.items())
        },
        "placements_by_split": {
            split: dict(sorted(values.items()))
            for split, values in sorted(placement_split_counts.items())
        },
        "review_methods": dict(sorted(review_method_counts.items())),
        "skipped": dict(sorted(skipped.items())),
        "total_images": sum(counts.values()),
    }
    (output / "stats.json").write_text(json.dumps(stats, indent=2, sort_keys=True) + "\n")
    if required_test_placements:
        present = set(placement_split_counts.get("test", {}))
        missing = sorted(required_test_placements - present)
        if missing:
            raise ValueError(
                f"test split is missing required placement slices: {missing}; "
                f"present={sorted(present)}"
            )
    return stats


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--val-fraction", type=float, default=0.15)
    parser.add_argument("--test-fraction", type=float, default=0.15)
    parser.add_argument("--require-review-method", action="append", default=None,
                        help="only admit labels with this explicit review_method; repeat to allow several")
    parser.add_argument("--require-test-placement", action="append", default=None,
                        help="fail unless the test split contains this placement; repeat per required slice")
    parser.add_argument("--stratify-placement", action="append", default=None,
                        help="deterministically reserve val/test source groups for this placement")
    parser.add_argument("--stratify-preservation-kind", action="append", default=None,
                        help="reserve independent train/val/test groups for a keep-region kind")
    parser.add_argument("--stratify-negative-placement", action="append", default=None,
                        help="reserve independent train/val/test contextual hard-negative groups")
    args = parser.parse_args()
    if args.val_fraction < 0 or args.test_fraction < 0 or args.val_fraction + args.test_fraction >= 1:
        parser.error("split fractions must be non-negative and sum to less than 1")
    methods = set(args.require_review_method) if args.require_review_method else None
    placements = set(args.require_test_placement) if args.require_test_placement else None
    stratify = set(args.stratify_placement) if args.stratify_placement else None
    preservation = (set(args.stratify_preservation_kind)
                    if args.stratify_preservation_kind else None)
    negative_placements = (set(args.stratify_negative_placement)
                           if args.stratify_negative_placement else None)
    print(json.dumps(build(args.input, args.output, args.val_fraction, args.test_fraction,
                           methods, placements, stratify, preservation,
                           negative_placements), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
