"""Small, dependency-free checks for the dataset boundary."""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("export_labels.py")


class ExportLabelsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "screenshots").mkdir()
        (self.root / "labels").mkdir()

    def screenshot(self, stem: str, boxes: list[dict], pixels: bytes | None = None) -> None:
        (self.root / "screenshots" / f"{stem}.png").write_bytes(pixels or stem.encode())
        (self.root / "labels" / f"{stem}.json").write_text(json.dumps({
            "image": f"{stem}.png", "imageWidth": 100, "imageHeight": 100,
            "boxes": boxes,
        }), encoding="utf-8")

    def run_export(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.root), "--name", "test"],
            capture_output=True, text=True,
        )

    def test_classes_negatives_and_duplicate_split(self) -> None:
        self.screenshot("a", [{"x": .1, "y": .2, "width": .2, "height": .3,
                                "class": "sponsor_remove"}])
        self.screenshot("a_copy", [{"x": .1, "y": .2, "width": .2, "height": .3,
                                     "class": "sponsor_remove"}], pixels=b"a")
        self.screenshot("b", [{"x": .4, "y": .4, "width": .1, "height": .1,
                                "class": "team_keep"}])
        self.screenshot("c", [])
        result = self.run_export()
        self.assertEqual(result.returncode, 0, result.stderr)
        out = self.root / "exports" / "test"
        report = json.loads((out / "dataset_report.json").read_text())
        self.assertEqual(report["duplicate_images_dropped"], 1)
        self.assertEqual(report["negative_images"], 1)
        self.assertEqual(report["box_counts"]["sponsor_remove"], 1)
        self.assertEqual(report["box_counts"]["team_keep"], 1)
        labels = list((out / "labels").rglob("*.txt"))
        self.assertTrue(any(p.read_text().startswith("1 0.200000 0.350000") for p in labels))
        self.assertTrue(any(not p.read_text() for p in labels))

    def test_unknown_class_fails_closed(self) -> None:
        self.screenshot("a", [{"x": 0, "y": 0, "width": .2, "height": .2,
                                "class": "league_keep"}])
        self.screenshot("b", [])
        result = self.run_export()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown box class", result.stderr)

    def test_conflicting_labels_on_identical_image_fail(self) -> None:
        self.screenshot("a", [{"x": 0, "y": 0, "width": .2, "height": .2}], pixels=b"same")
        self.screenshot("b", [], pixels=b"same")
        result = self.run_export()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Conflicting labels", result.stderr)

    def test_invalid_geometry_fails(self) -> None:
        self.screenshot("a", [{"x": .9, "y": 0, "width": .2, "height": .2}])
        self.screenshot("b", [])
        result = self.run_export()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the image", result.stderr)


if __name__ == "__main__":
    unittest.main()
