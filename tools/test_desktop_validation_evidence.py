import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path

from tools import desktop_validation_evidence as evidence


class DesktopValidationEvidenceTests(unittest.TestCase):
    def test_init_record_and_hashes_are_rerunnable_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "run"
            evidence.init_run(
                Namespace(
                    output=run,
                    platform="windows",
                    evidence_state="build-only",
                    allow_dirty=True,
                    os_build="test-os",
                    hardware="test-machine",
                    gpu="test-gpu",
                    displays="one display at 100%",
                    package_sha256=None,
                )
            )
            environment = json.loads((run / "environment.json").read_text())
            self.assertEqual(environment["schemaVersion"], 1)
            self.assertEqual(environment["platform"], "windows")
            log = run / "commands.log"
            log.write_text("cargo test\n")
            evidence.record_result(
                Namespace(
                    run_dir=run,
                    scenario="native-build",
                    result="pass",
                    operator="CI",
                    started_at="2026-07-14T00:00:00Z",
                    finished_at="2026-07-14T00:01:00Z",
                    notes="build only",
                    evidence=["commands.log"],
                )
            )
            results = json.loads((run / "scenario-results.json").read_text())["results"]
            self.assertEqual(results[0]["evidence"], ["commands.log"])
            evidence.hash_run(Namespace(run_dir=run))
            hashes = (run / "artifacts.sha256").read_text()
            self.assertIn("environment.json", hashes)
            self.assertIn("scenario-results.json", hashes)
            with self.assertRaises(FileExistsError):
                evidence.hash_run(Namespace(run_dir=run))
            with self.assertRaises(ValueError):
                evidence.record_result(
                    Namespace(
                        run_dir=run,
                        scenario="after-seal",
                        result="pass",
                        operator="CI",
                        started_at="2026-07-14T00:02:00Z",
                        finished_at="2026-07-14T00:03:00Z",
                        notes="must fail",
                        evidence=[],
                    )
                )

    def test_record_rejects_outside_and_symlink_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run = root / "run"
            run.mkdir()
            (run / "scenario-results.json").write_text('{"schemaVersion":1,"results":[]}')
            outside = root / "outside.log"
            outside.write_text("private")
            with self.assertRaises(ValueError):
                evidence.confined_evidence(run.resolve(), "../outside.log")
            link = run / "link.log"
            try:
                link.symlink_to(outside)
            except OSError:
                self.skipTest("symlinks unavailable")
            with self.assertRaises(ValueError):
                evidence.confined_evidence(run.resolve(), "link.log")


if __name__ == "__main__":
    unittest.main()
