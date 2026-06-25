"""Pure-metric unit tests for the eval harness.

These run on a bare system Python (no torch/ultralytics venv) because
`run_eval` defers all model imports into the functions that need them.
"""

from run_eval import greedy_match, iou


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
