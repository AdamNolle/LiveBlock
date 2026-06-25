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


def load_vocab(path: str) -> list[str]:
    """Load a Vocabulary JSON file and flatten every class's prompts in order.

    The returned flat prompt list is what ``YOLOWorld.set_classes`` consumes.
    Order is preserved: classes in file order, prompts in per-class order.
    """
    data = json.loads(Path(path).read_text())
    prompts: list[str] = []
    for c in data["classes"]:
        prompts.extend(c["prompts"])
    return prompts


def build(base: str = "yolov8s-worldv2.pt",
          vocab_path: str = str(DEFAULT_VOCAB),
          out: str = "liveblock-detector.pt") -> Path:
    """Bake the vocabulary into the open-vocab base and save the result.

    ``ultralytics`` is imported lazily here so importing this module (and
    running the ``load_vocab`` unit test) does not require torch.
    """
    from ultralytics import YOLOWorld  # type: ignore

    prompts = load_vocab(vocab_path)
    model = YOLOWorld(base)
    model.set_classes(prompts)      # bakes CLIP text embeddings into the head
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
