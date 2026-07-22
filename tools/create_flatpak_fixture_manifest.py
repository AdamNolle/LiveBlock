#!/usr/bin/env python3
"""Create a local-only Flatpak package-transition fixture manifest."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

FIXTURE_VERSION = "0.0.9"
MARKER_SOURCE = "platform/linux/flatpak/build-only-version-fixture.txt"
MARKER_DESTINATION = "/app/share/liveblock-build-only-version-fixture"


def create_fixture(source: Path, output: Path) -> None:
    source = source.resolve(strict=True)
    output = output.resolve(strict=False)
    if output.exists():
        raise ValueError(f"fixture output already exists: {output}")
    if output.parent != source.parent:
        raise ValueError("fixture manifest must stay beside the source manifest")

    manifest = json.loads(source.read_text(encoding="utf-8"))
    modules = manifest.get("modules")
    if not isinstance(modules, list):
        raise ValueError("Flatpak modules must be a list")
    matches = [
        module
        for module in modules
        if isinstance(module, dict) and module.get("name") == "liveblock-linux"
    ]
    if len(matches) != 1:
        raise ValueError("expected exactly one liveblock-linux module")
    commands = matches[0].get("build-commands")
    if not isinstance(commands, list) or not all(isinstance(item, str) for item in commands):
        raise ValueError("liveblock-linux build commands must be strings")
    commands.append(
        f"install -Dm644 {MARKER_SOURCE} {MARKER_DESTINATION}"
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(manifest, handle, sort_keys=False, indent=2)
        handle.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    create_fixture(args.source, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
