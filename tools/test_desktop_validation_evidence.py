import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

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

    def test_pass_requires_nonempty_evidence_and_attributed_operator(self):
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
                    displays="test-display",
                    package_sha256=None,
                )
            )
            base = dict(
                run_dir=run,
                scenario="protected-content-policy",
                result="pass",
                operator="CI",
                started_at="2026-07-14T00:00:00Z",
                finished_at="2026-07-14T00:01:00Z",
                notes="deterministic policy only",
                evidence=[],
            )
            with self.assertRaises(ValueError):
                evidence.record_result(Namespace(**base))
            log = run / "policy.log"
            log.write_text("")
            base["evidence"] = ["policy.log"]
            with self.assertRaises(ValueError):
                evidence.record_result(Namespace(**base))
            log.write_text("test passed\n")
            base["operator"] = "   "
            with self.assertRaises(ValueError):
                evidence.record_result(Namespace(**base))

    def test_blocked_state_cannot_record_pass_and_requires_explanation(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "run"
            def clean_git(*args):
                return "" if args[:2] == ("status", "--porcelain") else "a" * 40

            with patch.object(evidence, "git", side_effect=clean_git):
                evidence.init_run(
                    Namespace(
                        output=run,
                        platform="windows",
                        evidence_state="blocked-hardware",
                        allow_dirty=False,
                        os_build="test-os",
                        hardware="hardware unavailable",
                        gpu="not-recorded",
                        displays="not-recorded",
                        package_sha256=None,
                    )
                )
            base = dict(
                run_dir=run,
                scenario="permitted-game-environment",
                result="pass",
                operator="release-owner",
                started_at="2026-07-14T00:00:00Z",
                finished_at="2026-07-14T00:01:00Z",
                notes="not available",
                evidence=[],
            )
            with self.assertRaises(ValueError):
                evidence.record_result(Namespace(**base))
            base["result"] = "blocked"
            base["notes"] = ""
            with self.assertRaises(ValueError):
                evidence.record_result(Namespace(**base))
            base["notes"] = "No vendor-permitted anti-cheat test environment is available."
            evidence.record_result(Namespace(**base))
            evidence.hash_run(Namespace(run_dir=run))

    def test_verified_run_rejects_cross_platform_host(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "run"

            def clean_git(*args):
                return "" if args[:2] == ("status", "--porcelain") else "a" * 40

            with (
                patch.object(evidence, "git", side_effect=clean_git),
                patch.object(evidence.platform, "system", return_value="Darwin"),
                self.assertRaises(ValueError),
            ):
                evidence.init_run(
                    Namespace(
                        output=run,
                        platform="windows",
                        evidence_state="verified-real-device",
                        allow_dirty=False,
                        os_build="test-os",
                        hardware="test-machine",
                        gpu="test-gpu",
                        displays="test-display",
                        package_sha256=None,
                    )
                )
            self.assertFalse(run.exists())

    def test_hashes_reject_unsealed_symlink_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run = root / "run"
            evidence.init_run(
                Namespace(
                    output=run,
                    platform="windows",
                    evidence_state="build-only",
                    allow_dirty=True,
                    os_build="test-os",
                    hardware="test-machine",
                    gpu="test-gpu",
                    displays="test-display",
                    package_sha256=None,
                )
            )
            evidence.record_result(
                Namespace(
                    run_dir=run,
                    scenario="expected-failure",
                    result="fail",
                    operator="CI",
                    started_at="2026-07-14T00:00:00Z",
                    finished_at="2026-07-14T00:01:00Z",
                    notes="failure evidence may be absent",
                    evidence=[],
                )
            )
            outside = root / "outside.log"
            outside.write_text("outside\n")
            try:
                (run / "unsealed-link").symlink_to(outside)
            except OSError:
                self.skipTest("symlinks unavailable")
            with self.assertRaises(ValueError):
                evidence.hash_run(Namespace(run_dir=run))


if __name__ == "__main__":
    unittest.main()
