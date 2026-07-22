#!/usr/bin/env python3
"""Oversample reviewed real training images relative to synthetic derivatives."""
from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path


def oversample(corpus: Path, repeat: int) -> dict:
    records = [json.loads(line) for line in (corpus / "provenance.jsonl").read_text().splitlines() if line]
    added = 0
    sources = 0
    for record in records:
        if record.get("split") != "train" or record.get("source") == "synthetic_derivative":
            continue
        image = corpus / record["corpus_path"]
        label = corpus / "labels" / "train" / f"{image.stem}.txt"
        if not label.exists() or not label.read_text().strip():
            continue
        sources += 1
        for index in range(repeat):
            stem = f"{image.stem}-realrep-{index:02d}"
            shutil.copy2(image, image.with_name(stem + image.suffix))
            shutil.copy2(label, label.with_name(stem + ".txt"))
            added += 1
    result = {"added_images": added, "repeat": repeat, "source_images": sources}
    (corpus / "oversampling.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--repeat", type=int, default=12)
    args = parser.parse_args()
    print(json.dumps(oversample(args.corpus, args.repeat), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
