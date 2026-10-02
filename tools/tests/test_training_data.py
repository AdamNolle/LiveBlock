import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import export_labels
import export_onnx
import export_to_coreml


class DatasetTests(unittest.TestCase):
    def test_failed_onnx_metadata_copy_preserves_installed_model(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "new.onnx"; target = root / "installed/model.onnx"
            target.parent.mkdir()
            source.write_bytes(b"new"); target.write_bytes(b"working")
            source.with_suffix(".classes.json").write_text('{"names":["ad"]}')
            target.with_suffix(".classes.json").write_text('{"names":["previous"]}')
            copy = export_onnx.shutil.copy2
            def fail_metadata(src, dst):
                if Path(src).suffix == ".json":
                    raise OSError("fixture: interrupted metadata copy")
                return copy(src, dst)
            with patch.object(export_onnx.shutil, "copy2", side_effect=fail_metadata), self.assertRaises(OSError):
                export_onnx._install_one(source, target)
            self.assertEqual(target.read_bytes(), b"working")
            self.assertEqual(json.loads(target.with_suffix(".classes.json").read_text())["names"], ["previous"])

    def test_failed_coreml_copy_preserves_installed_package(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); source = root / "new.mlpackage"; target = root / "Sources/model.mlpackage"
            source.mkdir(); target.mkdir(parents=True)
            (target / "weights").write_bytes(b"working")
            with patch.object(export_to_coreml, "TARGET_PATH", target), patch.object(export_to_coreml.shutil, "copytree", side_effect=OSError("fixture: interrupted copy")), self.assertRaises(OSError):
                export_to_coreml.install_into_repo(source)
            self.assertEqual((target / "weights").read_bytes(), b"working")

    def test_failed_coreml_commit_restores_installed_package(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); source = root / "new.mlpackage"; target = root / "Sources/model.mlpackage"
            source.mkdir(); target.mkdir(parents=True)
            (source / "weights").write_bytes(b"new"); (target / "weights").write_bytes(b"working")
            replace = export_to_coreml.os.replace
            def fail_commit(src, dst):
                if Path(src).parent.name.startswith(".liveblock-model-"):
                    raise OSError("fixture: interrupted commit")
                return replace(src, dst)
            with patch.object(export_to_coreml, "TARGET_PATH", target), patch.object(export_to_coreml.os, "replace", side_effect=fail_commit), self.assertRaises(OSError):
                export_to_coreml.install_into_repo(source)
            self.assertEqual((target / "weights").read_bytes(), b"working")

    def box(self, **changes):
        return {"x": 0.1, "y": 0.2, "width": 0.3, "height": 0.4, **changes}

    def test_unknown_classes_and_nonfinite_rectangles_are_rejected(self):
        for box in [self.box(**{"class": "future_keep"}), self.box(x=float("nan")), self.box(width=-1), self.box(x=0.9)]:
            with self.assertRaises(ValueError):
                export_labels.to_yolo_lines({"boxes": [box]}, export_labels.CLASS_TO_ID, binary=False)

    def test_keep_classes_remain_distinct_from_remove_classes(self):
        boxes = [self.box(**{"class": c}) for c in export_labels.CLASS_ORDER]
        lines = export_labels.to_yolo_lines({"boxes": boxes}, export_labels.CLASS_TO_ID, binary=False)
        self.assertEqual([line.split()[0] for line in lines], ["0", "1", "2", "3"])
        self.assertEqual(len(export_labels.to_yolo_lines({"boxes": boxes}, {"ad": 0}, binary=True)), 2)

    def test_export_has_disjoint_nonempty_splits_and_keeps_negatives(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "screenshots").mkdir(); (root / "labels").mkdir()
            for i in range(4):
                (root / "screenshots" / f"{i}.png").write_bytes(b"fixture-image")
                (root / "labels" / f"{i}.json").write_text(json.dumps({"boxes": [self.box()] if i < 2 else []}))
            argv = ["export_labels.py", "--root", str(root), "--name", "checked", "--include-empty"]
            with patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(export_labels.main(), 0)
            out = root / "exports/checked"
            train = {p.name for p in (out / "images/train").iterdir()}
            val = {p.name for p in (out / "images/val").iterdir()}
            self.assertTrue(train and val); self.assertFalse(train & val)
            self.assertEqual(len(train | val), 4)
            with patch.object(sys, "argv", argv), self.assertRaises(SystemExit):
                export_labels.main()

    def test_single_image_cannot_masquerade_as_validated_training(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); (root / "screenshots").mkdir(); (root / "labels").mkdir()
            (root / "screenshots/one.png").write_bytes(b"fixture")
            (root / "labels/one.json").write_text(json.dumps({"boxes": [self.box()]}))
            with patch.object(sys, "argv", ["export_labels.py", "--root", temp]), self.assertRaises(SystemExit):
                export_labels.main()

    def test_model_install_carries_metadata_and_clears_stale_metadata(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "source.onnx"; target = Path(temp) / "installed/model.onnx"
            source.write_bytes(b"onnx-fixture")
            source.with_suffix(".classes.json").write_text('{"names":["ad"]}')
            with contextlib.redirect_stdout(io.StringIO()):
                export_onnx._install_one(source, target)
                self.assertEqual(target.with_suffix(".classes.json").read_text(), source.with_suffix(".classes.json").read_text())
                source.with_suffix(".classes.json").unlink()
                export_onnx._install_one(source, target)
            self.assertFalse(target.with_suffix(".classes.json").exists())


class PublicDatasetTests(unittest.TestCase):
    def test_query_split_is_stable_and_has_three_partitions(self):
        from prepare_adserp import split_for_query
        self.assertEqual(split_for_query("same-query"), split_for_query("same-query"))
        self.assertEqual({split_for_query(str(i)) for i in range(1000)}, {"train", "val", "test"})

    def test_crop_clips_both_corners_and_omits_nonvisible_ads(self):
        from prepare_adserp import crop_labels
        annotations = {"native_ad": [
            {"location": {"x": -10, "y": 90}, "size": {"width": 40, "height": 30}},
            {"location": {"x": 20, "y": 300}, "size": {"width": 30, "height": 30}}]}
        rows = crop_labels(annotations, 100, 400, 100, 100)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0], "0 0.150000 0.100000 0.300000 0.200000")


class DeploymentEvaluationTests(unittest.TestCase):
    def test_duplicates_wrong_classes_and_negative_images_count_as_false_positives(self):
        from evaluate_predictions import evaluate
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "ad.txt").write_text("0 0.5 0.5 0.4 0.4\n")
            (root / "negative.txt").write_text("")
            box = {"class": "ad", "x": .3, "y": .3, "width": .4, "height": .4, "confidence": .9}
            results = evaluate({"ad": [box, box, {**box, "class": "team_keep"}], "negative": [box]}, root, ["ad"])
            self.assertEqual((results["tp"], results["fp"], results["fn"]), (1, 3, 0))
            self.assertEqual(results["precision"], .25)
            results = evaluate({}, root, ["ad"])
            self.assertEqual((results["tp"], results["fp"], results["fn"]), (0, 0, 1))


class SportsDatasetTests(unittest.TestCase):
    def test_frames_from_same_clip_cannot_cross_splits(self):
        from prepare_exposure import split_for_clip
        self.assertEqual(split_for_clip("12048_000028"), split_for_clip("12048_000043"))

    def test_sponsor_polygon_is_enclosed_and_invalid_geometry_is_rejected(self):
        from prepare_exposure import convert_labels
        self.assertEqual(convert_labels("408 0.2 0.3 0.4 0.2 0.5 0.4 0.3 0.5"),
                         ["0 0.350000 0.350000 0.300000 0.300000"])
        for invalid in ("0 0.2 0.3", "0 nan 0 0 0 1 1 1 0", "0 -1 0 0 0 1 1 1 0"):
            with self.assertRaises(ValueError): convert_labels(invalid)


if __name__ == "__main__": unittest.main()
