#!/usr/bin/env python3
"""Bake a fixed open-vocabulary into a pretrained YOLO-World-v2 model.

This is the "no training" engine: a pretrained open-vocab base
(`yolov8s-worldv2.pt`) is reparameterized with a fixed concept-level
vocabulary via ``set_classes`` (which folds CLIP text embeddings into the
detection head), producing a prompt-free model that runs as a plain YOLO at
runtime — no text encoder, real-time, fully offline.

  Usage:
    tools/.venv/bin/python tools/build_openvocab.py
    tools/.venv/bin/python tools/build_openvocab.py --base yolov8s-worldv2.pt \
        --vocab tools/vocab/liveblock-vocab.json --out liveblock-detector.pt

The heavy `ultralytics` import is deliberately LAZY (inside ``build``) so this
module imports — and ``load_vocab`` is unit-testable — on Python 3.13 without
torch/ultralytics installed.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_VOCAB = REPO_ROOT / "tools" / "vocab" / "liveblock-vocab.json"


def load_vocab(path: str) -> tuple[list[str], dict[int, str]]:
    """Load a Vocabulary JSON file → ``(primary_prompts, id→name)``.

    ONE prompt per class (``prompts[0]``, the strongest), so ``set_classes``
    yields a model whose class **index == the vocab class id**. The display
    label is the class ``name`` (applied after ``set_classes``), because the
    shared config / Swift / Windows / Linux layers all key on the class name —
    keeping the model's labels and the vocabulary in lock-step. Classes are
    sorted by id so index alignment is guaranteed.
    """
    data = json.loads(Path(path).read_text())
    classes = sorted(data.get("classes", []), key=lambda c: c["id"])
    if not classes:
        raise ValueError("vocabulary must contain at least one class")
    ids = [c.get("id") for c in classes]
    if ids != list(range(len(classes))):
        raise ValueError("vocabulary class ids must be unique, contiguous, and zero-based")
    if any(not isinstance(c.get("name"), str) or not c["name"].strip() for c in classes):
        raise ValueError("every vocabulary class needs a non-empty name")
    if len({c["name"] for c in classes}) != len(classes):
        raise ValueError("vocabulary class names must be unique")
    if any(not c.get("prompts") or not isinstance(c["prompts"][0], str)
           or not c["prompts"][0].strip() for c in classes):
        raise ValueError("every vocabulary class needs a non-empty primary prompt")
    prompts = [c["prompts"][0] for c in classes]
    names = {c["id"]: c["name"] for c in classes}
    return prompts, names


def build(base: str = "yolov8s-worldv2.pt",
          vocab_path: str = str(DEFAULT_VOCAB),
          out: str = "liveblock-detector.pt") -> Path:
    """Bake the vocabulary into the open-vocab base and save the result.

    ``ultralytics`` is imported lazily here so importing this module (and
    running the ``load_vocab`` unit test) does not require torch.
    """
    from ultralytics import YOLOWorld  # type: ignore

    prompts, names = load_vocab(vocab_path)
    model = YOLOWorld(base)
    model.set_classes(prompts)      # bakes CLIP text embeddings (from strong prompts)
    # Fail loudly if this Ultralytics version cannot set display labels. Runtime
    # filtering is name-keyed; silently saving prompt/COCO labels is unsafe.
    model.model.names = names       # display label = vocab class name (Logo/Ad banner/Sponsored)
    model.save(out)
    return Path(out)


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--base", default="yolov8s-worldv2.pt")
    ap.add_argument("--vocab", default=str(DEFAULT_VOCAB))
    ap.add_argument("--out", default="liveblock-detector.pt")
    a = ap.parse_args()
    print(build(a.base, a.vocab, a.out))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
