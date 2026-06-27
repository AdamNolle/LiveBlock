#!/usr/bin/env python3
"""Build the protected-mark REFERENCE GALLERY for on-device team/number keep.

The sports-broadcast pipeline (liveblock-core decide_verdict / ClassifySignals)
force-KEEPS any detected mark whose cosine similarity to a gallery of protected
team/league marks is >= tau_keep (PolicyThresholds::team_keep_sim, default 0.6).
That `team_gallery_sim` is computed ON-DEVICE by liveblock-detection's
GalleryMatcher against the embeddings this script produces.

What this does:
  1. Walk a folder of reference logo images, grouped by mark:
         references/
           <mark_id>/  *.png|*.jpg|*.jpeg|*.webp   # >=1 image per mark
           ...
     (a flat folder of images is also accepted; each file becomes its own mark)
  2. Embed every image with a frozen vision encoder (DINOv2 by default, CLIP
     optional), L2-normalize each vector.
  3. Write, per mark, BOTH:
       - the per-image vectors (so the matcher can take a max over views), and
       - the mean ("prototype") vector.
  4. Emit a single `gallery.json` manifest the Rust GalleryMatcher loads, plus
     optional per-mark `.npy` matrices for fast bulk load.

PREREQUISITE (documented, not auto-run here): the encoder weights must be
present. This script is structured to run once they are:
    pip install torch torchvision timm pillow numpy           # DINOv2 via timm
    # or, for CLIP:
    pip install open_clip_torch torch pillow numpy
The first run will, if `timm`/`open_clip` can fetch weights, download them; on a
fully offline box, pre-place the weights in the HuggingFace/timm cache.

  Usage:
    python tools/build_gallery.py                       # default dirs, DINOv2
    python tools/build_gallery.py --backbone clip
    python tools/build_gallery.py --references /path/to/logos --out /path/to/gallery
    python tools/build_gallery.py --no-npy              # JSON manifest only

Output JSON schema (stable contract for liveblock-detection::GalleryMatcher):
{
  "schema": "liveblock.gallery.v1",
  "backbone": "dinov2_vits14",
  "dim": 384,
  "metric": "cosine",
  "normalized": true,

  // FLAT view — maps 1:1 onto GalleryMatcher.embeddings (Vec<Vec<f32>>). A
  // consumer can do the trivial thing: read `embeddings` straight into
  // GalleryMatcher::with_embeddings(...). `embedding_marks[i]` names the mark
  // that row i came from (parallel array) so the mark/kind link survives.
  "embeddings": [[...], [...], ...],          // every view vector, L2-normalized
  "embedding_marks": ["team_acme_fc", ...],   // parallel to `embeddings`

  // STRUCTURED view — richer, for matchers that want per-mark prototypes or a
  // max-over-views policy.
  "marks": [
    {
      "id": "team_acme_fc",
      "kind": "team_keep",                 // or "number_keep"
      "num_views": 3,
      "prototype": [0.01, -0.02, ...],     // L2-normalized mean, length == dim
      "views": [[...], [...], [...]]       // each L2-normalized, length == dim
    },
    ...
  ]
}
The current GalleryMatcher (core/crates/liveblock-detection/src/gallery.rs) holds
a flat `Vec<Vec<f32>>` and returns max cosine similarity, so feed it `embeddings`
directly. Richer matchers can instead use each mark's `prototype` or `views`.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, List, Tuple

# Local helper for cross-platform default dirs.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import lb_paths  # noqa: E402

IMAGE_EXTS = {".png", ".jpg", ".jpeg", ".webp", ".bmp"}

# Backbone registry: name -> (loader_fn_name, embedding_dim, default_weights).
# The dim is recorded in the manifest; the matcher validates crop-embedding dim
# against it. timm's DINOv2 ViT-S/14 is small + fast and a good on-device default.
BACKBONES = {
    "dinov2": {"timm_name": "vit_small_patch14_dinov2.lvd142m", "dim": 384},
    "dinov2_vitb": {"timm_name": "vit_base_patch14_dinov2.lvd142m", "dim": 768},
    "clip": {"open_clip_name": "ViT-B-32", "open_clip_pretrained": "laion2b_s34b_b79k", "dim": 512},
}


# --------------------------------------------------------------------------- #
# Embedder backends. Each returns a callable: PIL.Image -> 1-D float32 ndarray
# (NOT yet normalized; we normalize centrally). Imports are lazy so the rest of
# the script (arg parsing, discovery, --help) works without the heavy deps.
# --------------------------------------------------------------------------- #
def _build_dinov2_embedder(timm_name: str):
    try:
        import numpy as np  # noqa: F401
        import timm  # type: ignore
        import torch  # type: ignore
        from timm.data import resolve_data_config  # type: ignore
        from timm.data.transforms_factory import create_transform  # type: ignore
    except ImportError as e:
        sys.exit(
            "DINOv2 backbone needs torch + timm. Install prerequisites:\n"
            "    pip install torch torchvision timm pillow numpy\n"
            f"(import error: {e})"
        )

    print(f"-> Loading DINOv2 backbone {timm_name} (downloads weights on first run) ...")
    model = timm.create_model(timm_name, pretrained=True, num_classes=0)
    model.eval()
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)

    cfg = resolve_data_config({}, model=model)
    transform = create_transform(**cfg)

    @torch.inference_mode()
    def embed(img):
        import numpy as np

        x = transform(img.convert("RGB")).unsqueeze(0).to(device)
        feat = model(x)  # (1, dim) — pooled CLS/global feature
        return feat.squeeze(0).float().cpu().numpy().astype(np.float32)

    return embed


def _build_clip_embedder(model_name: str, pretrained: str):
    try:
        import numpy as np  # noqa: F401
        import open_clip  # type: ignore
        import torch  # type: ignore
    except ImportError as e:
        sys.exit(
            "CLIP backbone needs open_clip_torch + torch. Install prerequisites:\n"
            "    pip install open_clip_torch torch pillow numpy\n"
            f"(import error: {e})"
        )

    print(f"-> Loading CLIP backbone {model_name}/{pretrained} (downloads weights on first run) ...")
    model, _, preprocess = open_clip.create_model_and_transforms(model_name, pretrained=pretrained)
    model.eval()
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)

    @torch.inference_mode()
    def embed(img):
        import numpy as np

        x = preprocess(img.convert("RGB")).unsqueeze(0).to(device)
        feat = model.encode_image(x)  # (1, dim)
        return feat.squeeze(0).float().cpu().numpy().astype(np.float32)

    return embed


def build_embedder(backbone: str) -> Tuple[callable, int]:
    spec = BACKBONES.get(backbone)
    if spec is None:
        sys.exit(f"Unknown backbone {backbone!r}. Choose one of: {', '.join(BACKBONES)}")
    if backbone.startswith("dinov2"):
        return _build_dinov2_embedder(spec["timm_name"]), spec["dim"]
    if backbone == "clip":
        return _build_clip_embedder(spec["open_clip_name"], spec["open_clip_pretrained"]), spec["dim"]
    sys.exit(f"No embedder wired for backbone {backbone!r}.")  # pragma: no cover


# --------------------------------------------------------------------------- #
# Discovery + I/O
# --------------------------------------------------------------------------- #
def discover_marks(references: Path) -> Dict[str, List[Path]]:
    """Group reference images by mark.

    - Subdirectory layout: each subdir is a mark; its name is the mark id.
    - Flat layout: each loose image is its own mark (id == file stem).
    """
    if not references.is_dir():
        sys.exit(
            f"References folder not found: {references}\n"
            f"Create it and drop team/league logo images inside (one subfolder per mark)."
        )

    marks: Dict[str, List[Path]] = {}
    subdirs = [d for d in sorted(references.iterdir()) if d.is_dir()]
    if subdirs:
        for d in subdirs:
            imgs = [p for p in sorted(d.iterdir()) if p.suffix.lower() in IMAGE_EXTS]
            if imgs:
                marks[d.name] = imgs
    # Also pick up loose images at the top level (each its own mark).
    loose = [p for p in sorted(references.iterdir()) if p.suffix.lower() in IMAGE_EXTS]
    for p in loose:
        marks.setdefault(p.stem, []).append(p)

    if not marks:
        sys.exit(
            f"No images found under {references}. Supported: "
            f"{', '.join(sorted(IMAGE_EXTS))}."
        )
    return marks


def infer_kind(mark_id: str) -> str:
    """Heuristic keep-kind from the mark id so the manifest is self-describing.

    Marks named like 'number_23' / '#7' / 'num-44' are number_keep; everything
    else is treated as team_keep. The on-device matcher may ignore this, but it
    makes the gallery auditable and lets the pipeline weight numbers separately.
    """
    low = mark_id.lower()
    if low.startswith(("number", "num", "no_", "no-", "#")) or low.replace("_", "").replace("-", "").isdigit():
        return "number_keep"
    return "team_keep"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    ap.add_argument(
        "--references",
        type=Path,
        default=lb_paths.gallery_dir() / "references",
        help="Folder of reference logos (subfolder per mark). Default: <gallery>/references",
    )
    ap.add_argument(
        "--out",
        type=Path,
        default=lb_paths.gallery_dir() / "embeddings",
        help="Output dir for gallery.json (+ optional .npy). Default: <gallery>/embeddings",
    )
    ap.add_argument(
        "--backbone",
        choices=list(BACKBONES.keys()),
        default="dinov2",
        help="Vision encoder (default: dinov2 / ViT-S/14)",
    )
    ap.add_argument(
        "--no-npy",
        dest="npy",
        action="store_false",
        default=True,
        help="Skip writing per-mark .npy matrices (JSON manifest only)",
    )
    args = ap.parse_args()

    references = args.references.expanduser().resolve()
    out_dir = args.out.expanduser().resolve()
    marks = discover_marks(references)
    print(f"-> {len(marks)} mark(s) discovered under {references}")

    # Defer heavy deps until after discovery so --help and empty-folder errors are fast.
    try:
        import numpy as np
        from PIL import Image  # type: ignore
    except ImportError as e:
        sys.exit(
            "Embedding needs numpy + pillow. Install prerequisites:\n"
            "    pip install numpy pillow\n"
            f"(import error: {e})"
        )

    embed, dim = build_embedder(args.backbone)
    out_dir.mkdir(parents=True, exist_ok=True)

    def l2norm(v):
        n = float(np.linalg.norm(v))
        if n < 1e-12:
            return v.astype(np.float32)
        return (v / n).astype(np.float32)

    manifest_marks = []
    flat_embeddings: List[List[float]] = []  # maps 1:1 onto GalleryMatcher.embeddings
    flat_embedding_marks: List[str] = []     # parallel: which mark each row is from
    total_views = 0
    for mark_id, paths in marks.items():
        vecs = []
        for p in paths:
            try:
                img = Image.open(p)
            except Exception as e:  # noqa: BLE001 — skip unreadable images, keep going
                print(f"  Warning: skipping {p} ({e})", file=sys.stderr)
                continue
            v = embed(img)
            if v.shape[0] != dim:
                sys.exit(
                    f"Embedding dim mismatch for {p}: got {v.shape[0]}, expected {dim}. "
                    f"Backbone/registry are inconsistent."
                )
            vecs.append(l2norm(v))
        if not vecs:
            print(f"  Warning: no usable images for mark {mark_id!r}; skipped.", file=sys.stderr)
            continue

        mat = np.stack(vecs, axis=0)  # (num_views, dim), each L2-normalized
        proto = l2norm(mat.mean(axis=0))  # normalize the mean too
        kind = infer_kind(mark_id)
        total_views += mat.shape[0]
        print(f"  {mark_id}: {mat.shape[0]} view(s)  kind={kind}")

        if args.npy:
            np.save(out_dir / f"{mark_id}.npy", mat)

        views = [[round(float(x), 6) for x in row] for row in mat.tolist()]
        manifest_marks.append(
            {
                "id": mark_id,
                "kind": kind,
                "num_views": int(mat.shape[0]),
                "prototype": [round(float(x), 6) for x in proto.tolist()],
                "views": views,
            }
        )
        for row in views:
            flat_embeddings.append(row)
            flat_embedding_marks.append(mark_id)

    if not manifest_marks:
        sys.exit("No marks were embeddable. Check your reference images.")

    manifest = {
        "schema": "liveblock.gallery.v1",
        "backbone": BACKBONES[args.backbone].get("timm_name")
        or BACKBONES[args.backbone].get("open_clip_name", args.backbone),
        "dim": dim,
        "metric": "cosine",
        "normalized": True,
        # Flat view: feed straight into GalleryMatcher::with_embeddings(...).
        "embeddings": flat_embeddings,
        "embedding_marks": flat_embedding_marks,
        # Structured view: per-mark prototypes + views.
        "marks": manifest_marks,
    }
    manifest_path = out_dir / "gallery.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    print()
    print(f"OK Gallery written: {manifest_path}")
    print(f"   marks: {len(manifest_marks)}   total views: {total_views}   dim: {dim}")
    if args.npy:
        print(f"   per-mark .npy matrices alongside in {out_dir}")
    print()
    print("On-device: liveblock-detection::GalleryMatcher loads this JSON and computes")
    print("team_gallery_sim = max cosine(crop_embedding, mark.views) for ClassifySignals.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
