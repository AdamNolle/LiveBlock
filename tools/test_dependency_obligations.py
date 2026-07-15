import json
import tempfile
import unittest
from pathlib import Path

from verify_dependency_obligations import verify

ROOT = Path(__file__).resolve().parents[1]
DECISIONS = ROOT / "licenses/dependency-license-decisions.json"
OFFER = ROOT / "licenses/mpl-source-offer.json"
LOCKS = [
    ROOT / "core/Cargo.lock",
    ROOT / "platform/windows/Cargo.lock",
    ROOT / "platform/linux/Cargo.lock",
]


class DependencyObligationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        decisions = json.loads(DECISIONS.read_text())
        offer = json.loads(OFFER.read_text())
        self.report = self.root / "dependency-licenses.json"
        self.report.write_text(json.dumps({
            "schemaVersion": 1,
            "passed": True,
            "licenseSelections": decisions["decisions"],
            "reviewRequired": [
                {"purl": item["purl"], "license": item["declaredLicense"]}
                for item in offer["sourceOffers"]
            ],
        }))

    def tearDown(self):
        self.temp.cleanup()

    def test_real_locked_decisions_and_source_offer_are_complete(self):
        document = verify(self.report, DECISIONS, OFFER, LOCKS)
        self.assertTrue(document["passed"])
        self.assertEqual(len(document["selectedLicensePurls"]), 2)
        self.assertEqual(len(document["preparedMplSourceOfferPurls"]), 5)
        self.assertNotIn(str(ROOT), json.dumps(document))

    def test_missing_or_extra_review_component_fails(self):
        report = json.loads(self.report.read_text())
        report["reviewRequired"].pop()
        self.report.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "exactly cover"):
            verify(self.report, DECISIONS, OFFER, LOCKS)

    def test_license_text_and_generated_selection_tampering_fail(self):
        copied = self.root / "licenses"
        copied.mkdir()
        copied_offer = copied / "mpl-source-offer.json"
        copied_offer.write_bytes(OFFER.read_bytes())
        (copied / "MPL-2.0.txt").write_text("tampered")
        with self.assertRaisesRegex(ValueError, "text hash mismatch"):
            verify(self.report, DECISIONS, copied_offer, LOCKS)

        report = json.loads(self.report.read_text())
        report["licenseSelections"][0]["selectedLicense"] = "Apache-2.0"
        self.report.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "exactly match"):
            verify(self.report, DECISIONS, OFFER, LOCKS)


if __name__ == "__main__":
    unittest.main()
