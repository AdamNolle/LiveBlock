from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.prepare_exposureengine import obb_to_yolo, prepare


class ExposureEngineTests(unittest.TestCase):
    def test_obb_becomes_sponsor_axis_aligned_box(self) -> None:
        row = "641 .2 .3 .4 .3 .4 .5 .2 .5"
        self.assertEqual(obb_to_yolo(row, Path("label.txt")),
                         "1 0.300000 0.400000 0.200000 0.200000")

    def test_match_frames_never_cross_splits(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "source"
            for split in ("train", "val", "test"):
                (source / split / "images").mkdir(parents=True)
                (source / split / "labels").mkdir(parents=True)
            for index, (split, match) in enumerate([
                ("train", "100"), ("val", "100"), ("train", "200"),
                ("test", "300"), ("train", "400"), ("val", "500"),
            ]):
                stem = f"{match}_{index:06d}"
                (source / split / "images" / f"{stem}.jpg").write_bytes(stem.encode())
                (source / split / "labels" / f"{stem}.txt").write_text(
                    "1 .2 .3 .4 .3 .4 .5 .2 .5\n", encoding="utf-8"
                )
            report = prepare(source, root / "converted", seed=42)
            groups = report["match_ids"]
            self.assertEqual(len({match for ids in groups.values() for match in ids}), 5)
            for split, ids in groups.items():
                stems = {p.stem.split("_")[0]
                         for p in (root / "converted" / "images" / split).iterdir()}
                self.assertEqual(stems, set(ids))


if __name__ == "__main__":
    unittest.main()
