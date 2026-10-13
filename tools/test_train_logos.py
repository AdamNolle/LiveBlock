"""Quality policy checks using synthetic validation metrics, no ML dependencies."""
from __future__ import annotations

import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import train_logos


class TrainingQualityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = self.root / "data.yaml"
        self.data.write_text("test: images/val\n", encoding="utf-8")
        val = self.root / "labels" / "val"
        val.mkdir(parents=True)
        (val / "examples.txt").write_text(
            "".join(f"{class_id} .5 .5 .1 .1\n" for class_id in range(4) for _ in range(5)),
            encoding="utf-8",
        )
        weights = self.root / "run" / "weights"
        weights.mkdir(parents=True)
        self.best = weights / "best.pt"
        self.args = types.SimpleNamespace(
            data=self.data, imgsz=640, batch=2, device="cpu", run_name="test",
            min_precision=.8, min_map50=.5,
        )

    def evaluate(self, keep_recall: float) -> dict:
        box = types.SimpleNamespace(
            mp=.93, mr=.92, map50=.91, map=.8,
            ap_class_index=[0, 1, 2, 3],
            p=[.90, .95, .93, .94],
            r=[.9, .9, keep_recall, .94],
            ap50=[.91, .92, .90, .90],
        )
        fake_metrics = types.SimpleNamespace(box=box)

        class FakeYOLO:
            names = {0: "ad", 1: "sponsor_remove", 2: "team_keep", 3: "number_keep"}

            def __init__(self, checkpoint: str) -> None:
                self.checkpoint = checkpoint

            def val(self, **kwargs):
                return fake_metrics

        with patch.dict("sys.modules", {"ultralytics": types.SimpleNamespace(YOLO=FakeYOLO)}):
            return train_logos.evaluate(self.best, self.args)

    def test_sponsor_requires_strong_keep_recall(self) -> None:
        self.assertEqual(self.evaluate(.7)["auto_block_labels"], ["ad"])
        self.assertEqual(self.evaluate(.95)["auto_block_labels"], ["ad", "sponsor_remove"])


if __name__ == "__main__":
    unittest.main()
