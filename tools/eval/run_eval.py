#!/usr/bin/env python3
"""LiveBlock open-vocab detector eval harness.

Computes precision/recall (overall + per class) for a baked open-vocab model
against a directory of labeled fixtures, plus an IoU helper used by the unit
tests.

IMPORTANT: torch / ultralytics / PIL imports are done *lazily* inside the
functions that need a model so that the pure-metric code (`iou`, matching)
and `test_eval.py` run on a bare system Python with no ML venv installed.

Fixture format (per image `foo.png` an optional sidecar `foo.json`):

    {
      "boxes": [
        {"class_id": 0, "box": [x, y, w, h]},
        ...
      ],
      "negative": false
    }

`box` is [x, y, w, h] in pixels. A fixture with no boxes (or "negative": true)
is a hard negative: any detection on it counts as a false positive.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, List, Sequence, Tuple

Box = Sequence[float]  # (x, y, w, h)

IMG_EXTS = (".png", ".jpg", ".jpeg", ".bmp", ".webp")


def iou(box_a: Box, box_b: Box) -> float:
    """Intersection-over-union of two axis-aligned boxes in (x, y, w, h) form.

    Returns 0.0 when the boxes do not overlap or either has non-positive area.
    """
    ax, ay, aw, ah = box_a
    bx, by, bw, bh = box_b
    if aw <= 0 or ah <= 0 or bw <= 0 or bh <= 0:
        return 0.0

    ax2, ay2 = ax + aw, ay + ah
    bx2, by2 = bx + bw, by + bh

    inter_x1 = max(ax, bx)
    inter_y1 = max(ay, by)
    inter_x2 = min(ax2, bx2)
    inter_y2 = min(ay2, by2)

    inter_w = inter_x2 - inter_x1
    inter_h = inter_y2 - inter_y1
    if inter_w <= 0 or inter_h <= 0:
        return 0.0

    inter = inter_w * inter_h
    union = aw * ah + bw * bh - inter
    if union <= 0:
        return 0.0
    return inter / union


def greedy_match(
    preds: List[dict],
    truths: List[dict],
    iou_threshold: float = 0.5,
) -> Tuple[int, int, int]:
    """Greedy one-to-one match of predictions to ground truth.

    Each element is a dict with keys "class_id" and "box" (x, y, w, h).
    Predictions may also carry a "score" used to order matching (highest first).

    Returns (true_positives, false_positives, false_negatives).
    A prediction matches a truth only if class ids agree and IoU >= threshold.
    """
    ordered = sorted(preds, key=lambda p: p.get("score", 1.0), reverse=True)
    used = [False] * len(truths)
    tp = 0
    fp = 0
    for p in ordered:
        best_iou = 0.0
        best_j = -1
        for j, t in enumerate(truths):
            if used[j] or t["class_id"] != p["class_id"]:
                continue
            cur = iou(p["box"], t["box"])
            if cur >= iou_threshold and cur > best_iou:
                best_iou = cur
                best_j = j
        if best_j >= 0:
            used[best_j] = True
            tp += 1
        else:
            fp += 1
    fn = used.count(False)
    return tp, fp, fn


def _safe_div(num: float, den: float) -> float:
    return num / den if den else 0.0


def load_fixture_labels(image_path: Path) -> Tuple[List[dict], bool]:
    """Load ground-truth boxes for an image. Returns (truths, is_negative)."""
    sidecar = image_path.with_suffix(".json")
    if not sidecar.exists():
        return [], True
    data = json.loads(sidecar.read_text())
    if data.get("negative"):
        return [], True
    boxes = [
        {"class_id": int(b["class_id"]), "box": list(b["box"])}
        for b in data.get("boxes", [])
    ]
    return boxes, len(boxes) == 0


def _load_model(model_path: str):
    """Lazily import ultralytics and load the baked detector model."""
    from ultralytics import YOLO  # noqa: WPS433 (lazy by design)

    return YOLO(model_path)


def _predict(model, image_path: Path, score_threshold: float) -> List[dict]:
    """Run the model on one image, returning xywh-pixel detections."""
    results = model(str(image_path), verbose=False, conf=score_threshold)
    preds: List[dict] = []
    for r in results:
        boxes = getattr(r, "boxes", None)
        if boxes is None:
            continue
        for b in boxes:
            x1, y1, x2, y2 = (float(v) for v in b.xyxy[0].tolist())
            preds.append(
                {
                    "class_id": int(b.cls[0].item()),
                    "score": float(b.conf[0].item()),
                    "box": [x1, y1, x2 - x1, y2 - y1],
                }
            )
    return preds


def evaluate(
    model_path: str,
    fixtures_dir: str,
    iou_threshold: float = 0.5,
    score_threshold: float = 0.25,
) -> Dict:
    """Evaluate a model over a fixtures dir.

    Returns {"precision", "recall", "per_class": {id: {precision, recall, ...}}}.
    Requires a real model artifact + ML venv (imports are lazy).
    """
    model = _load_model(model_path)
    fixtures = sorted(
        p
        for p in Path(fixtures_dir).iterdir()
        if p.suffix.lower() in IMG_EXTS
    )
    if not fixtures:
        raise FileNotFoundError(f"no fixture images found in {fixtures_dir!r}")

    agg = {"tp": 0, "fp": 0, "fn": 0}
    per_class: Dict[int, Dict[str, int]] = {}

    def bump(cls_id: int, key: str, n: int = 1) -> None:
        pc = per_class.setdefault(cls_id, {"tp": 0, "fp": 0, "fn": 0})
        pc[key] += n

    for img in fixtures:
        truths, is_negative = load_fixture_labels(img)
        preds = _predict(model, img, score_threshold)

        if is_negative:
            # every detection on a hard negative is a false positive
            agg["fp"] += len(preds)
            for p in preds:
                bump(p["class_id"], "fp")
            continue

        tp, fp, fn = greedy_match(preds, truths, iou_threshold)
        agg["tp"] += tp
        agg["fp"] += fp
        agg["fn"] += fn

        # per-class breakdown
        for cls_id in {b["class_id"] for b in truths} | {p["class_id"] for p in preds}:
            c_preds = [p for p in preds if p["class_id"] == cls_id]
            c_truths = [t for t in truths if t["class_id"] == cls_id]
            ctp, cfp, cfn = greedy_match(c_preds, c_truths, iou_threshold)
            bump(cls_id, "tp", ctp)
            bump(cls_id, "fp", cfp)
            bump(cls_id, "fn", cfn)

    result = {
        "precision": _safe_div(agg["tp"], agg["tp"] + agg["fp"]),
        "recall": _safe_div(agg["tp"], agg["tp"] + agg["fn"]),
        "per_class": {
            cls_id: {
                "precision": _safe_div(v["tp"], v["tp"] + v["fp"]),
                "recall": _safe_div(v["tp"], v["tp"] + v["fn"]),
                "tp": v["tp"],
                "fp": v["fp"],
                "fn": v["fn"],
            }
            for cls_id, v in sorted(per_class.items())
        },
    }
    return result


def main(argv: List[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="LiveBlock detector eval harness")
    ap.add_argument(
        "--model",
        default="../../Sources/liveblock-detector.mlpackage",
        help="path to the baked detector model (.mlpackage/.onnx/.pt)",
    )
    ap.add_argument(
        "--fixtures",
        default="fixtures",
        help="directory of labeled fixture images",
    )
    ap.add_argument("--iou", type=float, default=0.5, help="IoU match threshold")
    ap.add_argument(
        "--score",
        type=float,
        default=0.25,
        help="minimum detection score",
    )
    ap.add_argument(
        "--assert-recall",
        type=float,
        default=None,
        help="exit non-zero if overall recall falls below this floor",
    )
    args = ap.parse_args(argv)

    result = evaluate(args.model, args.fixtures, args.iou, args.score)
    print(json.dumps(result, indent=2, sort_keys=True))

    if args.assert_recall is not None and result["recall"] < args.assert_recall:
        print(
            f"FAIL: recall {result['recall']:.3f} < floor {args.assert_recall:.3f}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
