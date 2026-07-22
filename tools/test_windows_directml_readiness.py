import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from verify_windows_directml_readiness import GATES, ReadinessError, verify

ROOT = Path(__file__).resolve().parents[1]
READINESS = ROOT / "platform/windows/directml-transport-readiness.json"
SCHEMA = ROOT / "contracts/windows-directml-transport-readiness.schema.json"
DOC = (ROOT / "docs/WINDOWS_DIRECTML.md").read_text()
CAPTURE = (ROOT / "platform/windows/src-tauri/src/capture.rs").read_text()
DETECTION = (ROOT / "platform/windows/src-tauri/src/detection.rs").read_text()
CARGO = (ROOT / "platform/windows/src-tauri/Cargo.toml").read_text()
MAIN = (ROOT / "platform/windows/src-tauri/src/main.rs").read_text()


class WindowsDirectMlReadinessTests(unittest.TestCase):
    def mutated_readiness(self, mutation):
        temporary = tempfile.TemporaryDirectory(dir=ROOT / "tools")
        path = Path(temporary.name) / "readiness.json"
        value = json.loads(READINESS.read_text())
        mutation(value)
        path.write_text(json.dumps(value))
        return temporary, path

    def test_current_readiness_is_explicitly_blocked_zero_of_eight(self):
        summary = verify(ROOT, READINESS)
        self.assertEqual(summary["status"], "blocked")
        self.assertEqual(summary["decision"], "defer")
        self.assertEqual(summary["passedGateCount"], 0)
        self.assertEqual(summary["requiredGateCount"], 8)
        self.assertEqual(set(json.loads(READINESS.read_text())["requiredGates"]), set(GATES))
        self.assertTrue(SCHEMA.is_file())

    def test_ready_or_passed_claims_require_complete_hash_bound_evidence(self):
        mutations = (
            lambda value: value.update(status="ready", decision="enable"),
            lambda value: value["requiredGates"]["captureAdapterIdentity"].update(passed=True),
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                temporary, path = self.mutated_readiness(mutation)
                try:
                    with self.assertRaises(ReadinessError):
                        verify(ROOT, path)
                finally:
                    temporary.cleanup()

    def test_failed_gate_cannot_carry_implied_accepted_evidence(self):
        evidence_path = ROOT / "docs/WINDOWS_DIRECTML.md"
        digest = hashlib.sha256(evidence_path.read_bytes()).hexdigest()

        def mutate(value):
            value["requiredGates"]["captureAdapterIdentity"]["evidence"] = [
                {
                    "kind": "source-test",
                    "reference": "docs/WINDOWS_DIRECTML.md",
                    "sha256": digest,
                    "vendor": "not-applicable",
                }
            ]

        temporary, path = self.mutated_readiness(mutate)
        try:
            with self.assertRaisesRegex(ReadinessError, "failed gate"):
                verify(ROOT, path)
        finally:
            temporary.cleanup()

    def test_future_schema_unknown_fields_and_runtime_drift_fail_closed(self):
        mutations = (
            lambda value: value.update(schemaVersion=2),
            lambda value: value.update(unexpected=True),
            lambda value: value["runtime"].update(ort="2.0.0-rc.12"),
            lambda value: value.update(schemaVersion=True),
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                temporary, path = self.mutated_readiness(mutation)
                try:
                    with self.assertRaises(ReadinessError):
                        verify(ROOT, path)
                finally:
                    temporary.cleanup()

    def test_duplicate_json_fields_fail_closed(self):
        temporary = tempfile.TemporaryDirectory(dir=ROOT / "tools")
        path = Path(temporary.name) / "readiness.json"
        raw = READINESS.read_text().replace(
            '"schemaVersion": 1,',
            '"schemaVersion": 1, "schemaVersion": 1,',
            1,
        )
        path.write_text(raw)
        try:
            with self.assertRaisesRegex(ReadinessError, "duplicate JSON field"):
                verify(ROOT, path)
        finally:
            temporary.cleanup()

    def test_current_source_cannot_be_misrepresented_as_texture_transport(self):
        self.assertIn("D3D11CreateDevice(", CAPTURE)
        self.assertIn("D3D_DRIVER_TYPE_HARDWARE", CAPTURE)
        self.assertIn("MiscFlags: 0", CAPTURE)
        self.assertNotIn("D3D11On12CreateDevice", CAPTURE)
        self.assertNotIn("Win32_Graphics_Direct3D12", CARGO)
        self.assertNotIn("Win32_Graphics_Direct3D11on12", CARGO)
        self.assertIn("preprocess_bgra_letterbox", DETECTION)
        self.assertIn("Array4::from_shape_vec", DETECTION)
        self.assertNotIn("CreateGPUAllocationFromD3DResource", DETECTION)
        self.assertNotIn("create_binding()", DETECTION)
        self.assertIn("directml_registered_cpu_uploaded_tensor", DETECTION)
        self.assertIn("RunOptions::new()", MAIN)
        self.assertIn("run_options.terminate()", MAIN)
        self.assertIn("blocked (0/8 readiness gates)", MAIN)

    def test_documented_decision_covers_ownership_sync_and_revisit_boundary(self):
        for phrase in (
            "plain D3D11 device",
            "MiscFlags: 0",
            "D3D11On12 cannot retroactively unwrap",
            "CreateGPUAllocationFromD3DResource",
            "per-frame I/O binding",
            "0/8 gates passed",
            "not a safe incremental patch",
            "leave the texture-transport checklist item open",
            "promoted ONNX artifact",
        ):
            self.assertIn(phrase, DOC)


if __name__ == "__main__":
    unittest.main()
