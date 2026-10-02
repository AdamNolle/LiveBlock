#!/usr/bin/env python3
"""Foreground, cancellable export/train/install entry point for desktop apps."""
from __future__ import annotations
import sys
import argparse
import uuid
from pathlib import Path
import export_labels
import train_logos


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--root", type=Path, default=export_labels.DEFAULT_ROOT)
    parsed, training_args = parser.parse_known_args()
    name = "app-" + uuid.uuid4().hex
    sys.argv = ["export_labels.py", "--root", str(parsed.root), "--name", name, "--include-empty"]
    export_labels.main()
    dataset = parsed.root / "exports" / name / "data.yaml"
    sys.argv = ["train_logos.py", "--data", str(dataset), "--install", *training_args]
    return train_logos.main()


if __name__ == "__main__":
    sys.exit(main())
