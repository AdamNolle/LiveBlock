#!/usr/bin/env python3
"""Cross-platform LiveBlock data-directory resolution.

Single source of truth for *where* the LiveBlock training data lives, so every
tool (export_labels.py, build_gallery.py, the export scripts, auto.ps1/.sh)
agrees with the native apps:

   macOS   ->  ~/Library/Application Support/LiveBlock
   Windows ->  %APPDATA%\\LiveBlock   (typically C:\\Users\\<you>\\AppData\\Roaming\\LiveBlock)
   Linux   ->  $XDG_DATA_HOME/LiveBlock  or  ~/.local/share/LiveBlock

These mirror, exactly:
   - macOS:   Sources/TrainingController.swift (TrainingPaths)
   - Windows: platform/windows/src-tauri/src/paths.rs (appdata_root)
   - Linux:   platform/linux/src-tauri/src/paths.rs

Keep them in lockstep — if a native path helper changes, change this too.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

__all__ = [
    "app_support_root",
    "training_root",
    "screenshots_dir",
    "labels_dir",
    "exports_dir",
    "trash_dir",
    "gallery_dir",
    "models_dir",
]


def app_support_root() -> Path:
    """Return the per-user LiveBlock application-support directory for this OS.

    Honors the same environment overrides the native apps respect
    (``%APPDATA%`` on Windows, ``$XDG_DATA_HOME`` on Linux) and falls back to a
    sensible default when they are unset.
    """
    home = Path.home()

    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "LiveBlock"

    if os.name == "nt" or sys.platform.startswith("win"):
        # %APPDATA% is the Roaming profile; this is what paths.rs uses.
        appdata = os.environ.get("APPDATA")
        if appdata:
            return Path(appdata) / "LiveBlock"
        # Fallback if APPDATA is somehow unset (e.g. stripped service env).
        return home / "AppData" / "Roaming" / "LiveBlock"

    # Linux / other Unix: XDG Base Directory spec.
    xdg = os.environ.get("XDG_DATA_HOME")
    base = Path(xdg) if xdg else home / ".local" / "share"
    return base / "LiveBlock"


def training_root() -> Path:
    """`<app-support>/training` — root for screenshots, labels, exports."""
    return app_support_root() / "training"


def screenshots_dir() -> Path:
    return training_root() / "screenshots"


def labels_dir() -> Path:
    return training_root() / "labels"


def exports_dir() -> Path:
    return training_root() / "exports"


def trash_dir() -> Path:
    return training_root() / "trash"


def gallery_dir() -> Path:
    """`<app-support>/gallery` — reference logos + computed embeddings.

    Layout (see tools/build_gallery.py):
        gallery/
          references/<mark_id>/*.png|*.jpg   # input reference logos
          embeddings/gallery.json            # per-mark mean vectors (default)
          embeddings/<mark_id>.npy           # optional per-mark raw matrices
    """
    return app_support_root() / "gallery"


def models_dir() -> Path:
    """`<app-support>/models` — runtime ONNX / CoreML the apps load at runtime.

    The native apps prefer a model here over their bundled fallback, so
    installing an export into this directory updates a *running* app without a
    rebuild.
    """
    return app_support_root() / "models"


if __name__ == "__main__":
    # Quick `python tools/lb_paths.py` introspection for debugging on any OS.
    print(f"platform           : {sys.platform} (os.name={os.name})")
    print(f"app_support_root   : {app_support_root()}")
    print(f"training_root      : {training_root()}")
    print(f"  screenshots_dir  : {screenshots_dir()}")
    print(f"  labels_dir       : {labels_dir()}")
    print(f"  exports_dir      : {exports_dir()}")
    print(f"  trash_dir        : {trash_dir()}")
    print(f"gallery_dir        : {gallery_dir()}")
    print(f"models_dir         : {models_dir()}")
