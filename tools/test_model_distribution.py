from __future__ import annotations

import base64
import hashlib
import json
import os
import tempfile
import unittest
from types import SimpleNamespace
from pathlib import Path
from unittest.mock import patch

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

import install_verified_model as installer
from install_verified_model import validate_promotion_report
from sign_model_manifest import (
    DEFAULT_PRIVATE_KEY_ENV,
    MODEL_MANIFEST_SCHEMA,
    _create_manifest_from_attested_report,
    _signing_bytes,
)
from promotion_contract import (
    GATE_SCHEMA,
    REQUIRED_MAX_FALSE_POSITIVES,
    REQUIRED_MAX_P95_MS,
    REQUIRED_MIN_PLACEMENT_RECALL,
    REQUIRED_MIN_PRECISION,
    REQUIRED_MIN_RECALL,
    REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS,
    REQUIRED_PROMOTION_PLACEMENTS,
    REQUIRED_PROMOTION_PRESERVATION_KINDS,
    artifact_sha256,
    gate_code_artifacts,
    promotion_command,
    rerun_promotion_gate,
)


class ModelDistributionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.paths = {
            "baseline_coreml": self.root / "baseline.mlpackage",
            "baseline_model": self.root / "baseline.pt",
            "candidate_coreml": self.root / "candidate.mlpackage",
            "candidate_model": self.root / "candidate.pt",
            "corpus": self.root / "corpus",
            "fixtures": self.root / "fixtures",
            "pool": self.root / "pool",
        }
        for key, path in self.paths.items():
            if key in {"baseline_model", "candidate_model"}:
                path.write_bytes(key.encode())
            else:
                path.mkdir()
                (path / "content.bin").write_bytes(key.encode())
        self.report_path = self.root / "promotion.json"
        self._write_report()
        self.seed = bytes(range(32))

    def tearDown(self):
        self.temp.cleanup()

    def _write_report(self, *, passed: bool = True) -> None:
        config = {
            key: str(path) for key, path in self.paths.items()
        }
        config.update({
            "placement": sorted(REQUIRED_PROMOTION_PLACEMENTS),
            "negative_placement": sorted(REQUIRED_PROMOTION_NEGATIVE_PLACEMENTS),
            "preservation_kind": sorted(REQUIRED_PROMOTION_PRESERVATION_KINDS),
            "min_precision": REQUIRED_MIN_PRECISION,
            "min_recall": REQUIRED_MIN_RECALL,
            "min_placement_recall": REQUIRED_MIN_PLACEMENT_RECALL,
            "max_false_positives": REQUIRED_MAX_FALSE_POSITIVES,
            "max_p95_ms": REQUIRED_MAX_P95_MS,
            "score": 0.25,
            "benchmark_runs": 30,
        })
        report = {
            "passed": passed,
            "failures": [] if passed else ["blocked"],
            "gate": {"schema": GATE_SCHEMA, "code_artifacts": gate_code_artifacts()},
            "config": config,
            "corpus": {},
            "fixtures": {},
            "quality": {},
            "coreml": {
                "candidate": {
                    "core_classes": ["Logo", "Ad banner", "Sponsored"],
                    "latency": {"image_size": [640, 640], "p95_ms": 5.0},
                },
                "baseline": {},
            },
            "artifacts": {
                key: {"path": str(path), "sha256": artifact_sha256(path)}
                for key, path in self.paths.items()
            },
        }
        self.report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

    def _manifest(self) -> dict:
        encoded = base64.b64encode(self.seed).decode("ascii")
        with patch.dict(os.environ, {DEFAULT_PRIVATE_KEY_ENV: encoded}, clear=False):
            return _create_manifest_from_attested_report(
                report_path=self.report_path,
                artifact_format="coreml",
                model_version="1.2.3",
                release_sequence=7,
                key_id="test-release",
            )

    def test_signer_binds_exact_passing_report_and_artifact(self):
        manifest = self._manifest()
        self.assertEqual(manifest["schemaVersion"], MODEL_MANIFEST_SCHEMA)
        self.assertEqual(manifest["promotionGateSchema"], GATE_SCHEMA)
        self.assertEqual(manifest["releaseSequence"], 7)
        self.assertEqual((manifest["inputWidth"], manifest["inputHeight"]), (640, 640))
        self.assertIs(manifest["nmsEmbedded"], True)
        self.assertEqual(
            manifest["promotionReportSha256"],
            hashlib.sha256(self.report_path.read_bytes()).hexdigest(),
        )
        self.assertEqual(
            manifest["artifactSha256"], artifact_sha256(self.paths["candidate_coreml"])
        )
        key = Ed25519PrivateKey.from_private_bytes(self.seed).public_key()
        key.verify(base64.b64decode(manifest["signature"]), _signing_bytes(manifest))
        raw_public = key.public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw,
        )
        self.assertEqual(len(raw_public), 32)

    def test_signer_rejects_failed_stale_or_unexported_artifacts(self):
        self._write_report(passed=False)
        with self.assertRaisesRegex(ValueError, "did not pass"):
            self._manifest()

        self._write_report()
        self.paths["candidate_coreml"].joinpath("content.bin").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "fingerprint"):
            self._manifest()

        self.paths["candidate_coreml"].joinpath("content.bin").write_bytes(
            b"candidate_coreml"
        )
        self._write_report()
        encoded = base64.b64encode(self.seed).decode("ascii")
        with patch.dict(os.environ, {DEFAULT_PRIVATE_KEY_ENV: encoded}, clear=False):
            with self.assertRaisesRegex(ValueError, "candidate_onnx"):
                _create_manifest_from_attested_report(
                    report_path=self.report_path,
                    artifact_format="onnx",
                    model_version="1.2.3",
                    release_sequence=8,
                    key_id="test-release",
                )

    def test_signer_rejects_missing_runtime_evidence(self):
        report = json.loads(self.report_path.read_text())
        del report["coreml"]["candidate"]["latency"]["image_size"]
        self.report_path.write_text(json.dumps(report, sort_keys=True))
        with self.assertRaisesRegex(ValueError, "input image size"):
            self._manifest()

    def test_private_key_is_required_and_never_a_cli_value(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, DEFAULT_PRIVATE_KEY_ENV):
                _create_manifest_from_attested_report(
                    report_path=self.report_path,
                    artifact_format="coreml",
                    model_version="1.2.3",
                    release_sequence=1,
                    key_id="test-release",
                )

    def test_python_tree_hash_matches_rust_fixture(self):
        fixture = self.root / "hash-fixture"
        fixture.mkdir()
        (fixture / "nested").mkdir()
        (fixture / "a.txt").write_bytes(b"X")
        (fixture / "nested" / "b.bin").write_bytes(b"Y")
        self.assertEqual(
            artifact_sha256(fixture),
            "8961fa489500947939aca204c59fff6f27f19e0fabb49cab47e2d0158efcf134",
        )

    def test_python_tree_hash_rejects_symlinks(self):
        target = self.root / "target"
        target.write_text("x")
        link = self.paths["candidate_coreml"] / "link"
        try:
            link.symlink_to(target)
        except OSError:
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(ValueError, "symlink"):
            artifact_sha256(self.paths["candidate_coreml"])

    def test_signing_cli_reconstructs_a_fresh_exclusive_gate_run(self):
        recipe = json.loads(self.report_path.read_text())
        attested = self.root / "attested.json"
        command = promotion_command(recipe, attested)
        self.assertTrue(command[1].endswith("tools/verify_promotion.py"))
        self.assertEqual(command[-3:], ["--output", str(attested), "--exclusive-output"])
        self.assertNotIn(str(self.report_path), command)
        for option in (
            "--candidate-coreml", "--candidate-model", "--placement",
            "--negative-placement", "--preservation-kind", "--max-p95-ms",
        ):
            self.assertIn(option, command)

    @patch("promotion_contract.subprocess.run")
    def test_gate_rerun_does_not_inherit_signing_secret(self, run):
        def fake_gate(command, **_kwargs):
            Path(command[-2]).write_text("{}")
            return SimpleNamespace(returncode=0, stdout="", stderr="")
        run.side_effect = fake_gate
        secret = "SIGNING_SECRET_FOR_TEST"
        with patch.dict(os.environ, {secret: "private"}, clear=False):
            rerun_promotion_gate(
                self.report_path,
                self.root / "attested-rerun.json",
                secret_environment_names=(secret,),
            )
        self.assertNotIn(secret, run.call_args.kwargs["env"])
        self.assertFalse(run.call_args.kwargs["shell"] if "shell" in run.call_args.kwargs else False)

    @patch("install_verified_model.install")
    @patch("install_verified_model.rerun_promotion_gate")
    def test_install_cli_strips_canonical_signing_secret(self, rerun, install):
        destination = self.root / "active.mlpackage"
        install.return_value = destination
        argv = [
            "install_verified_model.py", "--report", str(self.report_path),
            "--destination", str(destination),
        ]
        with patch("sys.argv", argv):
            self.assertEqual(installer.main(), 0)
        self.assertEqual(
            rerun.call_args.kwargs["secret_environment_names"],
            (DEFAULT_PRIVATE_KEY_ENV,),
        )

    def test_report_validator_rejects_unknown_artifact_selector(self):
        with self.assertRaisesRegex(ValueError, "unsupported promotion artifact"):
            validate_promotion_report(self.report_path, "candidate_model")


if __name__ == "__main__":
    unittest.main()
