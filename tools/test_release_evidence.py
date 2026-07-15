from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from release_evidence import (
    _validate_relative_path,
    create_inventory,
    create_sbom,
    verify_inventory,
)

COMMIT = "a" * 40


class ReleaseArtifactInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "artifact"
        self.root.mkdir()
        (self.root / "bin").mkdir()
        (self.root / "bin" / "LiveBlock").write_bytes(b"executable")
        (self.root / "resources").mkdir()
        (self.root / "resources" / "manifest.json").write_text("{}\n")

    def tearDown(self):
        self.temp.cleanup()

    def inventory(self):
        return create_inventory(
            self.root,
            platform="test",
            artifact_type="unsigned-test-bundle",
            commit=COMMIT,
            required_paths=["bin/LiveBlock", "resources/manifest.json"],
        )

    def test_inventory_round_trip_is_deterministic(self):
        first = self.inventory()
        second = self.inventory()
        self.assertEqual(first, second)
        self.assertEqual([entry["path"] for entry in first["entries"]], [
            "bin", "bin/LiveBlock", "resources", "resources/manifest.json",
        ])
        verify_inventory(self.root, first)

    def test_verify_rejects_mutation_addition_removal_and_mode_change(self):
        for mutation in ("change", "add", "empty-directory", "remove", "mode", "special-mode"):
            with self.subTest(mutation=mutation):
                document = self.inventory()
                if mutation == "change":
                    (self.root / "bin" / "LiveBlock").write_bytes(b"changed")
                elif mutation == "add":
                    (self.root / "unexpected").write_text("x")
                elif mutation == "empty-directory":
                    (self.root / "empty").mkdir()
                elif mutation == "remove":
                    (self.root / "resources" / "manifest.json").unlink()
                elif mutation == "mode":
                    (self.root / "bin" / "LiveBlock").chmod(0o755)
                else:
                    (self.root / "bin" / "LiveBlock").chmod(0o4755)
                with self.assertRaisesRegex(ValueError, "does not match"):
                    verify_inventory(self.root, document)
                # Restore the fixture for the next subtest.
                for path in (self.root / "unexpected",):
                    path.unlink(missing_ok=True)
                if (self.root / "empty").exists():
                    (self.root / "empty").rmdir()
                (self.root / "bin" / "LiveBlock").write_bytes(b"executable")
                (self.root / "bin" / "LiveBlock").chmod(0o644)
                (self.root / "resources" / "manifest.json").write_text("{}\n")

    def test_inventory_rejects_missing_required_path_and_symlink(self):
        with self.assertRaisesRegex(ValueError, "required artifact paths"):
            create_inventory(
                self.root, platform="test", artifact_type="test", commit=COMMIT,
                required_paths=["missing"],
            )
        link = self.root / "linked"
        try:
            link.symlink_to(self.root / "bin" / "LiveBlock")
        except OSError:
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(ValueError, "non-regular"):
            self.inventory()

    def test_published_path_schema_matches_runtime_canonicalization(self):
        root = Path(__file__).resolve().parents[1]
        schema = json.loads((root / "contracts/release-artifact-inventory.schema.json").read_text())
        pattern = re.compile(schema["$defs"]["path"]["pattern"])
        for path in ("LiveBlock.exe", "resources/model.manifest.json", "assets/a-b_1.js"):
            self.assertEqual(_validate_relative_path(path), path)
            self.assertIsNotNone(pattern.fullmatch(path))
        for path in ("", ".", "../x", "a/../x", "a/./x", "/absolute", "a//b", "a/", "a\\b"):
            with self.subTest(path=path):
                with self.assertRaises(ValueError):
                    _validate_relative_path(path)
                self.assertIsNone(pattern.fullmatch(path))

    def test_inventory_rejects_unknown_manifest_fields_and_bad_producer_identity(self):
        document = self.inventory()
        document["passed"] = True
        with self.assertRaisesRegex(ValueError, "unknown"):
            verify_inventory(self.root, document)
        with self.assertRaisesRegex(ValueError, "nonempty"):
            create_inventory(self.root, platform="", artifact_type="test", commit=COMMIT)
        with self.assertRaisesRegex(ValueError, "JSON object"):
            verify_inventory(self.root, 42)  # type: ignore[arg-type]


class ReleasePackagingPolicyTests(unittest.TestCase):
    def test_tauri_package_targets_and_resources_are_explicit(self):
        root = Path(__file__).resolve().parents[1]
        expectations = {
            "windows": ({"msi", "nsis"}, ["resources/*"]),
            "linux": ({"deb", "rpm", "appimage"}, ["resources/**/*"]),
        }
        for platform, (targets, resources) in expectations.items():
            with self.subTest(platform=platform):
                config = json.loads((root / f"platform/{platform}/src-tauri/tauri.conf.json").read_text())
                bundle = config["bundle"]
                self.assertTrue(bundle["active"])
                self.assertEqual(set(bundle["targets"]), targets)
                self.assertEqual(bundle["resources"], resources)
                serialized = json.dumps(bundle)
                self.assertNotIn("tools/requirements.txt", serialized)
                self.assertNotIn("tools/", serialized)
                self.assertNotIn("datasets/", serialized)


class ReleaseSbomTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.cargo = self.root / "linux-metadata.json"
        self.npm = self.root / "package-lock.json"
        self.decisions = self.root / "license-decisions.json"

    def tearDown(self):
        self.temp.cleanup()

    def write_inputs(self, *, cargo_license="MIT OR Apache-2.0", npm_license="MIT"):
        self.cargo.write_text(json.dumps({
            "packages": [
                {
                    "name": "liveblock-linux", "version": "0.1.0",
                    "license": "MIT", "source": None,
                },
                {
                    "name": "dependency", "version": "1.2.3",
                    "license": cargo_license,
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                },
            ]
        }))
        self.npm.write_text(json.dumps({
            "lockfileVersion": 3,
            "packages": {
                "": {"name": "frontend", "version": "0.1.0"},
                "node_modules/web-dependency": {
                    "name": "web-dependency", "version": "4.5.6",
                    "license": npm_license, "integrity": "sha512-test",
                },
            },
        }))

    def test_sbom_is_deterministic_and_privacy_minimized(self):
        self.write_inputs()
        first_sbom, first_report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        second_sbom, second_report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        self.assertEqual((first_sbom, first_report), (second_sbom, second_report))
        self.assertEqual(first_sbom["bomFormat"], "CycloneDX")
        self.assertTrue(first_report["passed"])
        self.assertEqual(first_report["componentCount"], 3)
        serialized = json.dumps(first_sbom)
        self.assertNotIn(str(self.root), serialized)
        self.assertNotIn(str(self.root), json.dumps(first_report))
        self.assertIn("pkg:cargo/dependency@1.2.3", serialized)
        self.assertIn("pkg:npm/web-dependency@4.5.6", serialized)

        cargo = json.loads(self.cargo.read_text())
        cargo["workspace_root"] = "/a/different/checkout"
        cargo["target_directory"] = "/another/target"
        self.cargo.write_text(json.dumps(cargo))
        relocated_sbom, relocated_report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        self.assertEqual(first_sbom, relocated_sbom)
        self.assertEqual(first_report, relocated_report)

    def test_unknown_restricted_and_mandatory_restricted_licenses_fail(self):
        for license_name in (
            "", "GPL-3.0-only", "AGPL-3.0-only", "Proprietary",
            "UNLICENSED", "LicenseRef-Proprietary", "MIT AND GPL-3.0-only",
            "MIT OR MadeUp-1.0", "MIT OR GPL-3.0-only",
        ):
            with self.subTest(license=license_name):
                self.write_inputs(cargo_license=license_name)
                _sbom, report = create_sbom([self.cargo], self.npm, commit=COMMIT)
                self.assertFalse(report["passed"])
                self.assertEqual(report["failures"][0]["name"], "dependency")
                if "MadeUp" in license_name:
                    dependency = next(item for item in _sbom["components"] if item["name"] == "dependency")
                    self.assertEqual(dependency["licenses"], [{"license": {"name": license_name}}])

    def test_cdla_permissive_is_recognized_spdx(self):
        self.write_inputs(cargo_license="CDLA-Permissive-2.0")
        _sbom, report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        self.assertTrue(report["passed"])
        self.assertEqual(report["failures"], [])

    def test_dual_permissive_lgpl_choice_passes_with_review_notice(self):
        self.write_inputs(cargo_license="MIT OR Apache-2.0 OR LGPL-2.1-or-later")
        sbom, report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        self.assertTrue(report["passed"])
        self.assertEqual(report["reviewRequired"][0]["name"], "dependency")
        dependency = next(item for item in sbom["components"] if item["name"] == "dependency")
        self.assertEqual(
            dependency["licenses"],
            [{"expression": "MIT OR Apache-2.0 OR LGPL-2.1-or-later"}],
        )

    def test_component_bound_or_choice_removes_only_selected_review_branch(self):
        expression = "MIT OR Apache-2.0 OR LGPL-2.1-or-later"
        self.write_inputs(cargo_license=expression)
        self.decisions.write_text(json.dumps({
            "schemaVersion": 1,
            "decisions": [{
                "purl": "pkg:cargo/dependency@1.2.3",
                "declaredExpression": expression,
                "selectedLicense": "MIT",
                "rationale": "elect the declared permissive branch",
            }],
        }))
        sbom, report = create_sbom(
            [self.cargo], self.npm, commit=COMMIT,
            license_decisions=self.decisions,
        )
        self.assertEqual(report["reviewRequired"], [])
        self.assertEqual(report["licenseSelections"][0]["selectedLicense"], "MIT")
        dependency = next(item for item in sbom["components"] if item["name"] == "dependency")
        self.assertEqual(
            next(item for item in dependency["properties"] if item["name"] == "liveblock:selectedLicense")["value"],
            "MIT",
        )
        self.assertEqual(dependency["licenses"], [{"expression": expression}])

    def test_license_decisions_fail_closed_on_mismatch_unused_or_restricted_expression(self):
        cases = (
            ("MIT OR LGPL-2.1-or-later", "Apache-2.0", "declared OR branch"),
            ("MIT OR GPL-3.0-only", "MIT", "cannot override restricted"),
        )
        for expression, selected, message in cases:
            with self.subTest(expression=expression):
                self.write_inputs(cargo_license=expression)
                self.decisions.write_text(json.dumps({
                    "schemaVersion": 1,
                    "decisions": [{
                        "purl": "pkg:cargo/dependency@1.2.3",
                        "declaredExpression": expression,
                        "selectedLicense": selected,
                        "rationale": "test",
                    }],
                }))
                with self.assertRaisesRegex(ValueError, message):
                    create_sbom(
                        [self.cargo], self.npm, commit=COMMIT,
                        license_decisions=self.decisions,
                    )

        self.write_inputs()
        self.decisions.write_text(json.dumps({
            "schemaVersion": 1,
            "decisions": [{
                "purl": "pkg:cargo/missing@9.9.9",
                "declaredExpression": "MIT OR Apache-2.0",
                "selectedLicense": "MIT",
                "rationale": "test",
            }],
        }))
        with self.assertRaisesRegex(ValueError, "did not match"):
            create_sbom(
                [self.cargo], self.npm, commit=COMMIT,
                license_decisions=self.decisions,
            )

    def test_denied_cli_preserves_machine_readable_failure_evidence(self):
        self.write_inputs(cargo_license="GPL-3.0-only")
        sbom = self.root / "sbom.json"
        report = self.root / "licenses.json"
        script = Path(__file__).with_name("release_evidence.py")
        result = subprocess.run([
            sys.executable, str(script), "sbom",
            "--cargo-metadata", str(self.cargo),
            "--npm-lock", str(self.npm),
            "--output", str(sbom),
            "--license-report", str(report),
            "--commit", COMMIT,
        ], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertTrue(sbom.is_file())
        self.assertFalse(json.loads(report.read_text())["passed"])
        self.assertNotIn("Traceback", result.stderr)

    def test_source_qualified_identity_is_order_independent_and_cannot_hide_license(self):
        self.write_inputs()
        metadata = json.loads(self.cargo.read_text())
        restricted = dict(metadata["packages"][1])
        restricted["source"] = "git+https://example.invalid/dependency"
        restricted["license"] = "GPL-3.0-only"
        metadata["packages"].append(restricted)
        self.cargo.write_text(json.dumps(metadata))
        first_sbom, first_report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        metadata["packages"].reverse()
        self.cargo.write_text(json.dumps(metadata))
        second_sbom, second_report = create_sbom([self.cargo], self.npm, commit=COMMIT)
        self.assertEqual((first_sbom, first_report), (second_sbom, second_report))
        self.assertFalse(first_report["passed"])
        matching = [item for item in first_sbom["components"] if item["name"] == "dependency"]
        self.assertEqual(len(matching), 2)
        self.assertNotEqual(matching[0]["bom-ref"], matching[1]["bom-ref"])


if __name__ == "__main__":
    unittest.main()
