# LiveBlock — Local Training Tools

These scripts let you train a logo/ad detector on your own Mac and drop it into
the LiveBlock app. Everything runs on Apple Silicon via PyTorch's `mps` backend
(no Nvidia required).

## What you'll get vs. what's hard

**Achievable today (these tools):**
- A YOLOv8-based **brand-logo detector** trained on a public logo dataset. After
  ~30 min – 3 hrs of training on an M-series Mac, the bundled model becomes
  one that actually fires on logos in your training set.
- A YOLOv8 **IAB ad-slot detector** if you assemble a dataset of standard ad
  layouts (300×250, 728×90, 160×600, 970×250 banners cropped from real pages).

**Not achievable with this tooling:**
- "Block any ad on any site, even ones the model has never seen." That requires
  a vision-language model (GroundingDINO + SigLIP zero-shot, or a SAM 2-style
  ad/no-ad segmentation model) — multi-GB weights, multi-week training. See
  [`ASSESSMENT.md`](../ASSESSMENT.md) §3 Path B / Path C.

Pick the path that matches the time you have.

---

## Hands-off training (set it and forget it)

`tools/auto.sh` runs Path B unattended: backgrounded, sleep-prevented,
auto-installs the trained model, rebuilds the app, and sends a macOS
notification when it's done.

```bash
# 1. Get a YOLO-format dataset (see "Get a dataset" below — ~3 min on Roboflow).

# 2. Start training in the background (returns immediately).
tools/auto.sh /path/to/data.yaml

# Optional: shorter run for a first try
tools/auto.sh /path/to/data.yaml --epochs 30 --batch 8

# Check progress, anytime, from any terminal
tools/auto.sh status     # one-line status
tools/auto.sh log        # tail -f the live log
tools/auto.sh stop       # cancel the job
```

After running `tools/auto.sh DATA`, **close the terminal**. The job continues
under `caffeinate -i` (Mac stays awake even if idle). When training finishes
you'll get a system notification and the bundled model will already be
rebuilt — just `./run.sh` to use it.

A few practical notes:
- Keep your **lid open**. `caffeinate -i` prevents idle-sleep, not lid-sleep.
- Use **AC power**. M-series GPUs draw real wattage during training.
- If the notification says "build failed", run `tools/auto.sh log` to see why.
  The trained `.pt` is preserved at `tools/runs/logo-finetune/weights/best.pt`.

### Get a dataset (3 minutes)

Easiest source: **Roboflow Universe** (free signup, thousands of public
datasets). Walkthrough:

1. Sign up at <https://universe.roboflow.com>.
2. Search "logo detection" or "brand detection". Open a project that has
   ≥1000 images and the brands you care about.
3. Click **Download Dataset** → choose **YOLOv8** format → "show download code"
   → copy the bash snippet they show you.
4. Paste into your terminal — it `curl`s a zip and unzips it. You'll get a
   folder containing `data.yaml`, `train/`, `valid/`.
5. Pass that `data.yaml` path to `tools/auto.sh`.

If you'd rather use HuggingFace, LogoDet-3K, or your own labelled images,
see "Path B" below — same training script, you just bring the dataset.

---

## One-time setup

```bash
cd tools
./setup_env.sh                # creates tools/.venv with ultralytics + coremltools
source .venv/bin/activate
python -c 'import ultralytics, coremltools; print("ok")'
```

Requires Python 3.10–3.12. `brew install python@3.11` if missing.

---

## Path A — Drop in a community pre-trained model (~5 min)

The fastest way to a working logo detector. You skip training entirely.

1. Find a community-trained YOLOv8 logo `.pt`. Good starting points:
   - **HuggingFace** — search "yolov8 logo detection" on
     [huggingface.co/models](https://huggingface.co/models).
   - **Roboflow Universe** — public projects at
     [universe.roboflow.com](https://universe.roboflow.com), filter to
     "Logos" / "Brand Detection" — most projects let you download YOLOv8
     weights directly.
2. Save the file somewhere local (e.g. `~/Downloads/logos-yolov8n.pt`).
3. Export a candidate:
   ```bash
   tools/.venv/bin/python tools/export_to_coreml.py \
       ~/Downloads/logos-yolov8n.pt
   ```
4. Evaluate it with `tools/verify_promotion.py`. Only a complete passing
   schema-5 report may be installed:
   ```bash
   tools/.venv/bin/python tools/install_verified_model.py \
       --report tools/runs/promotion-gate-current.json \
       --destination Sources/liveblock-detector.mlpackage
   ./run.sh --clean
   ```

Community weights are untrusted candidates until their license, taxonomy,
quality, preservation behavior, parity, latency, and artifact fingerprints all
pass the same promotion policy as locally trained weights.

---

## Path B — Train on your own dataset (30 min – 3 hrs)

Best for brand-specific detection where you know which logos you want blocked.

### 1. Get a dataset in YOLO format

You need `data.yaml` plus `train/images`, `train/labels`, `val/images`,
`val/labels`. Each label `.txt` has rows `class_id cx cy w h` (normalized).

Four sources, easiest first:

**Path D — your own labeled screenshots** (now built in!). Use the in-app
labeling pipeline: capture screenshots with ⌘⇧S while browsing, label them
in the **Label** window (Control Panel → Label), then run:
```bash
tools/.venv/bin/python tools/export_labels.py
```
That writes a YOLO-format dataset to
`~/Library/Application Support/LiveBlock/training/exports/<timestamp>/`
ready to feed to `train_logos.py` or `tools/auto.sh`.

**Roboflow Universe** (recommended for cold-starting — easy, free tier).
[universe.roboflow.com](https://universe.roboflow.com) → search "logo
detection" → pick a project → Download Dataset → choose the **YOLOv8**
format. You'll get a folder with `data.yaml` ready to use.

**LogoDet-3K** (3,000 brand classes, 158k images, ~6 GB).
[github.com/Wangjing1551/LogoDet-3K-Dataset](https://github.com/Wangjing1551/LogoDet-3K-Dataset)
→ follow their conversion script to YOLO format.

**OpenLogo** (352 brand classes, 27k images).
Academic dataset, search "OpenLogo dataset Hu Yu". Convert using the
included Pascal-VOC → YOLO scripts on GitHub.

Smaller datasets train faster but generalize less. **Start small** (1,000–5,000
images, 5–30 brands) to confirm the pipeline before committing to a big run.

### 2. Train

```bash
tools/.venv/bin/python tools/train_logos.py \
    --data path/to/data.yaml \
    --epochs 50 \
    --imgsz 640 \
    --device auto
```

`--device auto` picks `mps` on Apple Silicon, `cuda` on Nvidia, else `cpu`.
Training exports a candidate only. Verify and install it using the schema-5
commands above; direct `--install` paths fail closed.

What you'll see while training:
- Epoch progress with mAP@50, mAP@50-95, train loss, val loss.
- `tools/runs/logo-finetune/` will contain checkpoints (`weights/best.pt`,
  `weights/last.pt`) and PR/F1 plots.
- An empty disk and a fan that sounds like a hovercraft. ~3–6 GB peak disk for
  intermediate batches; ~16 GB RAM is comfortable.

Realistic training times (M-series, batch=16, imgsz=640):
| Dataset size | Epochs | M2 Pro | M1 Air |
|---|---|---|---|
| 1k images | 30 | ~10 min | ~25 min |
| 5k images | 50 | ~30 min | ~75 min |
| 30k images | 100 | ~3 hrs | ~8 hrs |

If your Mac runs out of memory: reduce `--batch` (try 8 or 4), or `--imgsz`
(try 416). If training is going nowhere after 10 epochs: confirm `data.yaml`
points at the right folders and labels are non-empty.

### 3. Rebuild the app

```bash
./run.sh --clean    # forces DerivedData to recompile the new .mlpackage
```

---

## Path C — Build your own dataset

You'll need this if you want detection that's specific to YOUR ads (the ones
that show up in your specific browsing). Real talk: this is a multi-week
project. The training tools above don't help with the data side.

A reasonable workflow:
1. Capture screenshots over a week of normal browsing (50–500 images).
2. Annotate with [Label Studio](https://labelstud.io) (free, OSS) or
   [Roboflow Annotate](https://roboflow.com) (free tier).
3. Export YOLOv8 format → run Path B above.

Honestly, by the time you've labelled 500 images, you'll have a strong opinion
about what "ad" means to you, and your model will reflect that. That's the
point.

---

## Troubleshooting

**`No module named ultralytics`** — you forgot to activate the venv.
`source tools/.venv/bin/activate`.

**Training crashes with `MPS out of memory`** — drop `--batch` to 8 or 4.

**Training runs but mAP@50 stays near 0** — your labels are wrong. Open
`tools/runs/logo-finetune/labels.jpg` to see how ultralytics is interpreting
your annotations. Check that label .txt rows are normalized 0–1 and that
class IDs match `data.yaml`'s `names` list.

**App still detects COCO classes after install** — DerivedData cached the old
compiled model. Run `./run.sh --clean`.

**`coremltools` import fails on Python 3.13+** — coremltools doesn't support
3.13 yet. Use Python 3.11: `PYTHON=$(brew --prefix python@3.11)/bin/python3
./setup_env.sh`.

---

## What goes where

```
tools/
  setup_env.sh             # create venv + install deps
  requirements.txt         # ultralytics + coremltools + friends
  train_logos.py           # ultralytics fine-tune wrapper
  export_to_coreml.py      # .pt → .mlpackage with int8 + NMS
  export_labels.py         # in-app labels → YOLO dataset
  auto.sh                  # hands-off train+rebuild+notify
  _unattended_runner.sh    # internal worker
  README.md                # this file
  .venv/                   # gitignored
  runs/                    # training outputs, gitignored
```

User-collected training data lives outside the repo, in your
Application Support folder:

```
~/Library/Application Support/LiveBlock/training/
  screenshots/   captured PNGs (⌘⇧S in app, or auto-capture)
  labels/        one JSON per labeled PNG (in-app format)
  exports/       YOLO-format datasets from export_labels.py
  trash/         screenshots you discarded in the labeling UI
```

## End-to-end "train on your own data" flow

1. Run LiveBlock. **Start** capturing.
2. While browsing, press **⌘⇧S** every time you see an ad.
   (Or: enable Auto-capture in Settings — saves a frame every N seconds.)
3. Open the **Label** window (Control Panel → Label, or menu bar).
   For each screenshot: drag a rectangle around each ad, then press → to
   advance. Press **N** if there are no ads in the screenshot
   (negative example — just as valuable).
4. Once you have ~200+ labeled screenshots:
   ```bash
   tools/.venv/bin/python tools/export_labels.py
   tools/auto.sh ~/Library/Application\ Support/LiveBlock/training/exports/<latest>/data.yaml
   ```
5. Walk away. macOS notification arrives 30 min – 3 hrs later.
6. `./run.sh` — your fine-tuned model is now active. Toggle Detection in the
   menu bar to see it fire.

Iterate by capturing more, labeling more, and re-training. Each cycle the
detector gets better at the ads you actually see in your day-to-day.
