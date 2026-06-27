# Reference-gallery layout (for `tools/build_gallery.py`)

The protected-mark gallery teaches the on-device pipeline which marks to KEEP
(team/league identity, player/car numbers) so a recognized mark is never erased
even when a sponsor classifier is unsure. See `liveblock-core::decide_verdict`
(the `team_gallery_sim >= team_keep_sim` hard-keep override).

`build_gallery.py` reads reference logo images from a folder and writes
per-mark embedding vectors that `liveblock-detection::GalleryMatcher` loads.

## Where the files live (per-OS, resolved by `tools/lb_paths.py`)

```
<app-support>/gallery/
  references/                 # YOU put reference images here
    <mark_id>/                # one subfolder per protected mark
      0.png  1.jpg  ...       # a few clean crops of the logo/number
    barcelona/  ...
    number_10/  ...
  embeddings/                 # build_gallery.py WRITES here
    gallery.json              # the manifest GalleryMatcher loads
    <mark_id>.npy             # optional raw per-view matrices
```

`<app-support>` is:
- macOS:   `~/Library/Application Support/LiveBlock`
- Windows: `%APPDATA%\LiveBlock`
- Linux:   `~/.local/share/LiveBlock` (or `$XDG_DATA_HOME/LiveBlock`)

## Mark id conventions

- A mark id is just the subfolder name. Use stable, lowercase, underscore ids:
  `team_barcelona`, `league_nba`, `number_23`.
- Ids starting with `number`/`num`/`no_`/`#`, or that are all digits, are tagged
  `number_keep` in the manifest; everything else is `team_keep`. (Auditing aid;
  the matcher may treat them uniformly.)

## How many images per mark?

- 1 works, but 3–8 clean views (different sizes, slight rotations, on-jersey vs
  isolated) make the cosine match far more robust — the matcher takes a max over
  views.
- Crop tightly to the mark. Avoid busy backgrounds.

## Running it (once the embedder weights are available)

```bash
# Prereqs (DINOv2 default): pip install torch torchvision timm pillow numpy
python tools/build_gallery.py                 # default dirs above, DINOv2 ViT-S/14
python tools/build_gallery.py --backbone clip # CLIP ViT-B/32 instead
```

The model download is a one-time prerequisite; on first run timm/open_clip fetch
the weights. On an offline box, pre-place the weights in the timm/HuggingFace
cache first.
