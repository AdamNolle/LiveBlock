from __future__ import annotations

import base64
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from promotion_contract import (
    REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
    REQUIRED_PROMOTION_PLACEMENTS,
    REQUIRED_PROMOTION_PRESERVATION_KINDS,
)
from release_blockers import (
    BOUNDARY,
    EXTERNAL_PREREQUISITES,
    KEYRINGS,
    _write_exclusive,
    assess,
    assess_human_review,
    assess_keyrings,
    assess_promotion,
)

COMMIT = "a" * 40
ROOT = Path(__file__).resolve().parents[1]


class ReleaseBlockerAssessmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def _write_keyrings(self, entries: list[dict[str, str]]) -> None:
        for _platform, relative in KEYRINGS:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({"schemaVersion": 1, "keys": entries}) + "\n")

    def _failed_promotion(self) -> dict:
        return {
            "passed": False,
            "failures": ["blocked"],
            "config": {
                "pool": "pool",
                "corpus": "corpus",
                "fixtures": "fixtures",
                "candidate_model": "candidate.pt",
                "baseline_model": "baseline.pt",
                "candidate_coreml": "candidate.mlpackage",
                "baseline_coreml": "baseline.mlpackage",
                "placement": list(REQUIRED_PROMOTION_PLACEMENTS),
                "negative_placement": list(REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
                "preservation_kind": list(REQUIRED_PROMOTION_PRESERVATION_KINDS),
                "min_precision": 0.5,
                "min_recall": 0.5,
                "min_placement_recall": 0.5,
                "max_false_positives": 10,
                "max_p95_ms": 10.0,
                "score": 0.25,
                "benchmark_runs": 30,
                "output": "report.json",
            },
            "gate": {"schema": 5, "code_artifacts": {"gate.py": "a" * 64}},
        }

    def test_current_repository_assessment_remains_conservatively_blocked(self):
        document = assess(
            ROOT,
            commit=COMMIT,
            promotion_report=Path("tools/runs/promotion-gate-current.json"),
            fresh_promotion_report=Path("tools/runs/release-blocker-test/fresh.json"),
        )
        self.assertEqual(document["schemaVersion"], 1)
        self.assertEqual(document["status"], "blocked")
        self.assertFalse(document["automatablePrerequisitesSatisfied"])
        self.assertEqual(document["blockerIds"], [
            "human-review",
            "schema-5-promotion",
            "production-model-trust",
            "directml-texture-transport",
        ])
        self.assertEqual(document["boundary"], BOUNDARY)
        self.assertEqual(document["externalPrerequisitesNotAssessed"], EXTERNAL_PREREQUISITES)
        self.assertEqual(document["gates"]["productionModelTrust"]["reasonCode"], "production-keyring-empty")
        self.assertEqual(document["gates"]["directmlTextureTransport"]["passedGateCount"], 0)
        self.assertEqual(document["gates"]["directmlTextureTransport"]["requiredGateCount"], 8)

    def test_missing_human_inputs_are_blocked_without_inventing_counts(self):
        gate = assess_human_review(self.root)
        self.assertEqual(gate["reasonCode"], "human-review-input-missing")
        self.assertIsNone(gate["approved"])
        self.assertIsNone(gate["planSha256"])

    def test_keyrings_require_three_valid_nonempty_production_rings(self):
        raw_key = bytes(range(32))
        entries = [{
            "keyId": "release-1",
            "publicKeyBase64": base64.b64encode(raw_key).decode("ascii"),
        }]
        self._write_keyrings(entries)
        gate = assess_keyrings(self.root)
        self.assertEqual(gate["status"], "satisfied")
        self.assertEqual(gate["reasonCode"], "production-keyrings-nonempty")
        self.assertEqual([item["keyCount"] for item in gate["keyrings"]], [1, 1, 1])

        first = self.root / KEYRINGS[0][1]
        first.write_text('{"schemaVersion":1,"schemaVersion":1,"keys":[]}')
        gate = assess_keyrings(self.root)
        self.assertEqual(gate["status"], "blocked")
        self.assertEqual(gate["reasonCode"], "production-keyring-invalid")

    def test_current_failed_promotion_is_accepted_only_as_a_blocker(self):
        report = self.root / "report.json"
        report.write_text(json.dumps(self._failed_promotion()))
        with mock.patch("release_blockers.gate_code_artifacts", return_value={"gate.py": "a" * 64}):
            gate = assess_promotion(self.root, report, self.root / "fresh.json")
        self.assertEqual(gate["status"], "blocked")
        self.assertEqual(gate["reasonCode"], "promotion-report-failed")
        self.assertFalse(gate["reportPassed"])
        self.assertTrue(gate["gateCodeCurrent"])
        self.assertEqual(gate["failureCount"], 1)
        self.assertFalse((self.root / "fresh.json").exists())

    def test_passing_recipe_is_not_satisfied_when_fresh_rerun_fails(self):
        report = self.root / "report.json"
        value = self._failed_promotion()
        value["passed"] = True
        value["failures"] = []
        report.write_text(json.dumps(value))
        fresh_path = self.root / "fresh.json"

        def fail_rerun(_recipe: Path, output: Path) -> None:
            failed = self._failed_promotion()
            output.write_text(json.dumps(failed))
            raise ValueError("fresh gate remained blocked")

        with (
            mock.patch("release_blockers.gate_code_artifacts", return_value={"gate.py": "a" * 64}),
            mock.patch("release_blockers.rerun_promotion_gate", side_effect=fail_rerun),
        ):
            gate = assess_promotion(self.root, report, fresh_path)
        self.assertEqual(gate["status"], "blocked")
        self.assertEqual(gate["reasonCode"], "promotion-fresh-rerun-failed")
        self.assertFalse(gate["reportPassed"])
        self.assertTrue(gate["gateCodeCurrent"])
        self.assertEqual(gate["failureCount"], 1)
        self.assertTrue(fresh_path.exists())

    def test_duplicate_or_unknown_promotion_fields_fail_closed(self):
        report = self.root / "report.json"
        report.write_text('{"passed":false,"passed":true,"failures":[],"config":{},"gate":{}}')
        gate = assess_promotion(self.root, report, self.root / "fresh.json")
        self.assertEqual(gate["reasonCode"], "promotion-report-invalid")

        value = self._failed_promotion()
        value["trustedByCaller"] = True
        report.write_text(json.dumps(value))
        gate = assess_promotion(self.root, report, self.root / "fresh.json")
        self.assertEqual(gate["reasonCode"], "promotion-report-invalid")

    def test_assessment_output_is_create_new(self):
        path = self.root / "assessment.json"
        document = {"schemaVersion": 1}
        _write_exclusive(path, document)
        self.assertEqual(json.loads(path.read_text()), document)
        with self.assertRaises(FileExistsError):
            _write_exclusive(path, document)

    def test_published_schema_covers_runtime_gate_and_reason_codes(self):
        schema = json.loads((ROOT / "contracts/release-blocker-assessment.schema.json").read_text())
        self.assertFalse(schema["additionalProperties"])
        self.assertEqual(schema["properties"]["boundary"]["const"], BOUNDARY)
        blocker_values = set(schema["properties"]["blockerIds"]["items"]["enum"])
        self.assertEqual(blocker_values, {
            "human-review",
            "schema-5-promotion",
            "production-model-trust",
            "directml-texture-transport",
        })
        self.assertIn(
            "promotion-report-stale",
            schema["$defs"]["promotion"]["properties"]["reasonCode"]["enum"],
        )


if __name__ == "__main__":
    unittest.main()
