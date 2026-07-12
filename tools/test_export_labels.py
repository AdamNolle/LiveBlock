import unittest

from export_labels import to_yolo_lines, validate_label_document


class ExportLabelContractTests(unittest.TestCase):
    def test_legacy_and_schema_one_are_accepted(self):
        legacy = {"boxes": []}
        current = {"schemaVersion": 1, "boxes": []}
        validate_label_document(legacy)
        validate_label_document(current)

    def test_future_and_noninteger_schemas_fail_closed(self):
        for version in (99, True, "1", 1.0):
            with self.subTest(version=version):
                with self.assertRaisesRegex(ValueError, "unsupported label schema"):
                    validate_label_document({"schemaVersion": version, "boxes": []})

    def test_yolo_conversion_preserves_normalized_geometry(self):
        lines = to_yolo_lines({
            "schemaVersion": 1,
            "boxes": [{"x": 0.1, "y": 0.2, "width": 0.4, "height": 0.2}],
        })
        self.assertEqual(lines, ["0 0.300000 0.300000 0.400000 0.200000"])


if __name__ == "__main__":
    unittest.main()
