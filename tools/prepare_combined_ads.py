#!/usr/bin/env python3
"""Combine prepared search-ad and soccer-sponsor datasets without changing splits."""
import argparse
import json
from pathlib import Path
import shutil


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent / "datasets/combined/yolo")
    args = parser.parse_args()
    if args.output.exists(): parser.error(f"Output already exists: {args.output}")
    counts = {split: 0 for split in ("train", "val", "test")}
    sources = []
    for domain in ("adserp", "exposure"):
        source = Path(__file__).resolve().parent / "datasets" / domain / "yolo"
        if not (source / "provenance.json").exists(): parser.error(f"Prepare {domain} first")
        for split in counts:
            for folder in ("images", "labels"): (args.output / folder / split).mkdir(parents=True, exist_ok=True)
            for image in sorted((source / "images" / split).iterdir()):
                label = source / "labels" / split / (image.stem+".txt")
                rows = label.read_text().splitlines()
                if any(row.split()[0] != "0" for row in rows): raise ValueError("Expected a single removal class")
                shutil.copy2(image, args.output / "images" / split / (domain+"-"+image.name))
                shutil.copy2(label, args.output / "labels" / split / (domain+"-"+label.name))
                counts[split] += 1
        shutil.copy2(source / "ATTRIBUTION.md", args.output / (domain+"-ATTRIBUTION.md"))
        sources.append({"domain": domain, "provenance": json.loads((source / "provenance.json").read_text())})
    (args.output / "provenance.json").write_text(json.dumps({"sources": sources, "counts": counts,
        "changes": "Search ads and explicit soccer sponsor boxes both map to the removal class ad; original grouped splits preserved."}, indent=2)+"\n")
    (args.output / "data.yaml").write_text(f"path: {json.dumps(str(args.output.resolve()))}\ntrain: images/train\nval: images/val\ntest: images/test\nnames:\n  0: ad\n")
    print(counts); print(args.output / "data.yaml")
    return 0


if __name__ == "__main__": raise SystemExit(main())
