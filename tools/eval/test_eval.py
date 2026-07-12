"""Pure-metric unit tests for the eval harness.

These run on a bare system Python (no torch/ultralytics venv) because
`run_eval` defers all model imports into the functions that need them.
"""

import json

import run_eval
from run_score_sweep import f1
from run_eval import (
    greedy_match,
    iou,
    parse_negative_placement_false_positive_ceilings,
    parse_placement_floors,
    parse_preservation_false_positive_ceilings,
    preserved_region_coverage,
)


def test_score_sweep_f1_handles_zero_and_harmonic_mean():
    assert f1(0, 0) == 0
    assert f1(0.5, 0.25) == 1 / 3


def test_iou_half_overlap():
    a = (0, 0, 10, 10)
    b = (5, 0, 10, 10)  # x, y, w, h
    # intersection 5x10=50, union 100+100-50=150
    assert abs(iou(a, b) - (50 / 150)) < 1e-6


def test_iou_identical_is_one():
    a = (3, 4, 20, 10)
    assert abs(iou(a, a) - 1.0) < 1e-9


def test_iou_disjoint_is_zero():
    assert iou((0, 0, 10, 10), (100, 100, 5, 5)) == 0.0


def test_iou_zero_area_is_zero():
    assert iou((0, 0, 0, 10), (0, 0, 10, 10)) == 0.0


def test_greedy_match_counts():
    truths = [
        {"class_id": 0, "box": [0, 0, 10, 10]},
        {"class_id": 1, "box": [50, 50, 10, 10]},
    ]
    preds = [
        {"class_id": 0, "box": [1, 1, 10, 10], "score": 0.9},  # matches truth 0
        {"class_id": 0, "box": [200, 200, 10, 10], "score": 0.8},  # false positive
    ]
    tp, fp, fn = greedy_match(preds, truths, iou_threshold=0.5)
    assert (tp, fp, fn) == (1, 1, 1)


def test_greedy_match_wrong_class_no_match():
    truths = [{"class_id": 0, "box": [0, 0, 10, 10]}]
    preds = [{"class_id": 1, "box": [0, 0, 10, 10], "score": 1.0}]
    tp, fp, fn = greedy_match(preds, truths)
    assert (tp, fp, fn) == (0, 1, 1)


def test_parse_placement_recall_floors():
    assert parse_placement_floors(["jersey=0.5", "venue_board=0.25"]) == {
        "jersey": 0.5,
        "venue_board": 0.25,
    }


def test_parse_negative_placement_false_positive_ceilings():
    assert parse_negative_placement_false_positive_ceilings([
        "ordinary_screen=0", "broadcast_overlay=2",
    ]) == {"ordinary_screen": 0, "broadcast_overlay": 2}


def test_parse_preservation_false_positive_ceilings():
    assert parse_preservation_false_positive_ceilings([
        "team_name=0", "jersey_number=1",
    ]) == {"team_name": 0, "jersey_number": 1}


def test_preserved_region_coverage_measures_keep_region_not_detection_size():
    assert preserved_region_coverage([0, 0, 100, 100], [25, 25, 10, 10]) == 1.0
    assert preserved_region_coverage([0, 0, 5, 5], [25, 25, 10, 10]) == 0.0


def test_evaluate_slices_false_positives_on_contextual_hard_negatives(tmp_path, monkeypatch):
    image = tmp_path / "screen.jpg"
    image.write_bytes(b"fixture")
    image.with_suffix(".json").write_text(json.dumps({
        "boxes": [],
        "negative": True,
        "negative_placements": ["ordinary_screen", "broadcast_overlay"],
    }))
    monkeypatch.setattr(run_eval, "_load_model", lambda _path: object())
    monkeypatch.setattr(run_eval, "_predict", lambda *_args: [
        {"class_id": 0, "box": [0, 0, 10, 10], "score": 0.9},
        {"class_id": 1, "box": [20, 20, 10, 10], "score": 0.8},
    ])

    result = run_eval.evaluate("model.pt", str(tmp_path))
    assert result["fp"] == 2
    assert result["negative_placements"] == {
        "broadcast_overlay": {"false_positives": 2, "images": 1},
        "ordinary_screen": {"false_positives": 2, "images": 1},
    }


def test_evaluate_reports_ground_truth_placement_recall(tmp_path, monkeypatch):
    image = tmp_path / "fixture.jpg"
    image.write_bytes(b"fixture")
    image.with_suffix(".json").write_text(json.dumps({
        "boxes": [
            {"class_id": 0, "box": [0, 0, 10, 10], "placement": "jersey"},
            {"class_id": 1, "box": [50, 50, 10, 10], "placement": "venue_board"},
        ],
        "preserve_regions": [
            {"kind": "jersey_number", "box": [80, 80, 10, 10]},
        ],
    }))
    monkeypatch.setattr(run_eval, "_load_model", lambda _path: object())
    monkeypatch.setattr(run_eval, "_predict", lambda *_args: [
        {"class_id": 0, "box": [0, 0, 10, 10], "score": 0.9},
        {"class_id": 0, "box": [80, 80, 10, 10], "score": 0.8},
    ])

    result = run_eval.evaluate("model.pt", str(tmp_path))
    assert result["per_placement"] == {
        "jersey": {"recall": 1.0, "tp": 1, "fn": 0, "truths": 1},
        "venue_board": {"recall": 0.0, "tp": 0, "fn": 1, "truths": 1},
    }
    assert result["preservation"] == {
        "jersey_number": {"false_positives": 1, "regions": 1},
    }
