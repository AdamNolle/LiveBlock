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


def parse_placement_floors(values: Sequence[str]) -> Dict[str, float]:
    floors: Dict[str, float] = {}
    for value in values:
        placement, separator, floor_text = value.partition("=")
        if not separator or not placement:
            raise ValueError(f"invalid placement recall floor {value!r}; expected placement=0..1")
        floor = float(floor_text)
        if not 0 <= floor <= 1:
            raise ValueError(f"placement recall floor must be between 0 and 1: {value!r}")
        floors[placement] = floor
    return floors


def parse_preservation_false_positive_ceilings(values: Sequence[str]) -> Dict[str, int]:
    ceilings: Dict[str, int] = {}
    for value in values:
        kind, separator, ceiling_text = value.partition("=")
        if not separator or not kind:
            raise ValueError(
                f"invalid preservation false-positive ceiling {value!r}; expected kind=0.."
            )
        try:
            ceiling = int(ceiling_text)
        except ValueError as error:
            raise ValueError(
                f"preservation false-positive ceiling must be an integer: {value!r}"
            ) from error
        if ceiling < 0:
            raise ValueError(
                f"preservation false-positive ceiling must not be negative: {value!r}"
            )
        ceilings[kind] = ceiling
    return ceilings


def parse_negative_placement_false_positive_ceilings(values: Sequence[str]) -> Dict[str, int]:
    ceilings: Dict[str, int] = {}
    for value in values:
        placement, separator, ceiling_text = value.partition("=")
        if not separator or not placement:
            raise ValueError(f"invalid negative-placement ceiling {value!r}; expected placement=0..")
        try:
            ceiling = int(ceiling_text)
        except ValueError as error:
            raise ValueError(f"negative-placement ceiling must be an integer: {value!r}") from error
        if ceiling < 0:
            raise ValueError(f"negative-placement ceiling must not be negative: {value!r}")
        ceilings[placement] = ceiling
    return ceilings


def preserved_region_coverage(prediction: Box, region: Box) -> float:
    """Return the fraction of a keep-region covered by a predicted block."""
    px, py, pw, ph = prediction
    rx, ry, rw, rh = region
    if pw <= 0 or ph <= 0 or rw <= 0 or rh <= 0:
        return 0.0
    inter_w = min(px + pw, rx + rw) - max(px, rx)
    inter_h = min(py + ph, ry + rh) - max(py, ry)
    if inter_w <= 0 or inter_h <= 0:
        return 0.0
    return (inter_w * inter_h) / (rw * rh)


def _load_fixture_data(image_path: Path) -> Tuple[List[dict], bool, List[dict], List[str]]:
    """Load sponsor truths, keep regions, and hard-negative placement context."""
    sidecar = image_path.with_suffix(".json")
    if not sidecar.exists():
        return [], True, [], []
    data = json.loads(sidecar.read_text())
    negative_placements = list(data.get("negative_placements", []))
    if data.get("negative"):
        return [], True, list(data.get("preserve_regions", [])), negative_placements
    boxes = [
        {
            "class_id": int(b["class_id"]),
            "box": list(b["box"]),
            "placement": b.get("placement", "unknown"),
        }
        for b in data.get("boxes", [])
    ]
    return boxes, len(boxes) == 0, list(data.get("preserve_regions", [])), negative_placements


def load_fixture_labels(image_path: Path) -> Tuple[List[dict], bool]:
    """Load ground-truth boxes for an image. Returns (truths, is_negative)."""
    truths, is_negative, _, _ = _load_fixture_data(image_path)
    return truths, is_negative


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
    preservation_coverage_threshold: float = 0.5,
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
    per_placement: Dict[str, Dict[str, int]] = {}
    negative_placements: Dict[str, Dict[str, int]] = {}
    preservation: Dict[str, Dict[str, int]] = {}

    def bump(cls_id: int, key: str, n: int = 1) -> None:
        pc = per_class.setdefault(cls_id, {"tp": 0, "fp": 0, "fn": 0})
        pc[key] += n

    for img in fixtures:
        truths, is_negative, preserve_regions, negative_contexts = _load_fixture_data(img)
        preds = _predict(model, img, score_threshold)

        for region in preserve_regions:
            kind = str(region.get("kind", "unknown"))
            counters = preservation.setdefault(kind, {"false_positives": 0, "regions": 0})
            counters["regions"] += 1
            if any(
                preserved_region_coverage(pred["box"], region["box"])
                >= preservation_coverage_threshold
                for pred in preds
            ):
                counters["false_positives"] += 1

        if is_negative:
            # Every detection on a hard negative is a false positive. Context
            # metadata allows honest FP slices without assigning placement to a detection.
            for placement in negative_contexts:
                counters = negative_placements.setdefault(placement, {"false_positives": 0, "images": 0})
                counters["false_positives"] += len(preds)
                counters["images"] += 1
            agg["fp"] += len(preds)
            for p in preds:
                bump(p["class_id"], "fp")
            continue

        tp, fp, fn = greedy_match(preds, truths, iou_threshold)
        agg["tp"] += tp
        agg["fp"] += fp
        agg["fn"] += fn

        # Contextual negatives can coexist with valid sponsor truths elsewhere
        # in the image. Attribute only unmatched detections to those contexts;
        # matched sponsor predictions remain true positives.
        for placement in negative_contexts:
            counters = negative_placements.setdefault(
                placement, {"false_positives": 0, "images": 0}
            )
            counters["false_positives"] += fp
            counters["images"] += 1

        # Placement is ground-truth metadata, not a predicted class. Report
        # slice recall (TP/FN) without inventing a placement for unmatched FPs.
        for placement in {truth.get("placement", "unknown") for truth in truths}:
            placement_truths = [
                truth for truth in truths
                if truth.get("placement", "unknown") == placement
            ]
            placement_tp, _, placement_fn = greedy_match(preds, placement_truths, iou_threshold)
            counters = per_placement.setdefault(placement, {"tp": 0, "fn": 0})
            counters["tp"] += placement_tp
            counters["fn"] += placement_fn

        # per-class breakdown
        for cls_id in {b["class_id"] for b in truths} | {p["class_id"] for p in preds}:
            c_preds = [p for p in preds if p["class_id"] == cls_id]
            c_truths = [t for t in truths if t["class_id"] == cls_id]
            ctp, cfp, cfn = greedy_match(c_preds, c_truths, iou_threshold)
            bump(cls_id, "tp", ctp)
            bump(cls_id, "fp", cfp)
            bump(cls_id, "fn", cfn)

    result = {
        "tp": agg["tp"],
        "fp": agg["fp"],
        "fn": agg["fn"],
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
        "negative_placements": {
            placement: dict(sorted(values.items()))
            for placement, values in sorted(negative_placements.items())
        },
        "per_placement": {
            placement: {
                "recall": _safe_div(values["tp"], values["tp"] + values["fn"]),
                "tp": values["tp"],
                "fn": values["fn"],
                "truths": values["tp"] + values["fn"],
            }
            for placement, values in sorted(per_placement.items())
        },
        "preservation": {
            kind: dict(sorted(values.items()))
            for kind, values in sorted(preservation.items())
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
    ap.add_argument(
        "--assert-precision",
        type=float,
        default=None,
        help="exit non-zero if overall precision falls below this floor",
    )
    ap.add_argument(
        "--max-false-positives",
        type=int,
        default=None,
        help="exit non-zero if aggregate false positives exceed this ceiling",
    )
    ap.add_argument(
        "--assert-placement-recall",
        action="append",
        default=[],
        metavar="PLACEMENT=FLOOR",
        help="repeatable held-out placement recall floor",
    )
    ap.add_argument(
        "--max-negative-placement-false-positives",
        action="append",
        default=[],
        metavar="PLACEMENT=CEILING",
        help="repeatable FP ceiling for contextual hard-negative placements",
    )
    ap.add_argument(
        "--max-preservation-false-positives",
        action="append",
        default=[],
        metavar="KIND=CEILING",
        help="repeatable false-positive ceiling for explicitly preserved regions",
    )
    ap.add_argument(
        "--preservation-coverage",
        type=float,
        default=0.5,
        help="fraction of a preserved region a detection may cover before it is a violation",
    )
    ap.add_argument("--output", help="optional path for machine-readable result JSON")
    args = ap.parse_args(argv)
    try:
        placement_floors = parse_placement_floors(args.assert_placement_recall)
        negative_placement_ceilings = parse_negative_placement_false_positive_ceilings(
            args.max_negative_placement_false_positives
        )
        preservation_ceilings = parse_preservation_false_positive_ceilings(
            args.max_preservation_false_positives
        )
    except ValueError as error:
        ap.error(str(error))
    if not 0 < args.preservation_coverage <= 1:
        ap.error("--preservation-coverage must be in (0, 1]")

    result = evaluate(args.model, args.fixtures, args.iou, args.score,
                      args.preservation_coverage)
    rendered = json.dumps(result, indent=2, sort_keys=True)
    print(rendered)
    if args.output:
        output = Path(args.output)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered + "\n")

    if args.assert_recall is not None and result["recall"] < args.assert_recall:
        print(
            f"FAIL: recall {result['recall']:.3f} < floor {args.assert_recall:.3f}",
            file=sys.stderr,
        )
        return 1
    if args.assert_precision is not None and result["precision"] < args.assert_precision:
        print(
            f"FAIL: precision {result['precision']:.3f} < floor {args.assert_precision:.3f}",
            file=sys.stderr,
        )
        return 1
    if args.max_false_positives is not None and result["fp"] > args.max_false_positives:
        print(
            f"FAIL: false positives {result['fp']} > ceiling {args.max_false_positives}",
            file=sys.stderr,
        )
        return 1
    for placement, floor in placement_floors.items():
        metrics = result["per_placement"].get(placement)
        if metrics is None:
            print(f"FAIL: placement {placement!r} is absent from held-out truths", file=sys.stderr)
            return 1
        if metrics["recall"] < floor:
            print(
                f"FAIL: {placement} recall {metrics['recall']:.3f} < floor {floor:.3f}",
                file=sys.stderr,
            )
            return 1
    for placement, ceiling in negative_placement_ceilings.items():
        metrics = result["negative_placements"].get(placement)
        if metrics is None:
            print(f"FAIL: negative placement {placement!r} is absent from held-out fixtures", file=sys.stderr)
            return 1
        if metrics["false_positives"] > ceiling:
            print(
                f"FAIL: {placement} hard-negative false positives "
                f"{metrics['false_positives']} > ceiling {ceiling}", file=sys.stderr,
            )
            return 1
    for kind, ceiling in preservation_ceilings.items():
        metrics = result["preservation"].get(kind)
        if metrics is None:
            print(f"FAIL: preservation kind {kind!r} is absent from held-out fixtures", file=sys.stderr)
            return 1
        if metrics["false_positives"] > ceiling:
            print(
                f"FAIL: {kind} false positives {metrics['false_positives']} > ceiling {ceiling}",
                file=sys.stderr,
            )
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
