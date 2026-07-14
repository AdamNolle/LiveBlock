from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import verify_flatpak_sources as verifier

ROOT = Path(__file__).resolve().parents[1]
FLATPAK = ROOT / "platform/linux/flatpak"


class FlatpakGeneratedSourceTests(unittest.TestCase):
    def test_committed_sources_match_lockfiles_and_manifest(self):
        verifier.verify()

    def test_cargo_archive_checksum_tamper_is_rejected(self):
        sources = json.loads((FLATPAK / "cargo-sources.json").read_text())
        archive = next(source for source in sources if source.get("type") == "archive")
        archive["sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "cargo-sources.json"
            path.write_text(json.dumps(sources))
            with self.assertRaisesRegex(ValueError, "Cargo archive metadata drift"):
                verifier._validate_cargo(ROOT / "platform/linux/Cargo.lock", path)

    def test_node_integrity_tamper_is_rejected(self):
        sources = json.loads((FLATPAK / "node-sources.json").read_text())
        source = next(
            source
            for source in sources
            if source.get("type") == "file" and source.get("url", "").endswith(".tgz")
        )
        hash_field = next(name for name in ("sha512", "sha256", "sha1") if name in source)
        source[hash_field] = "0" * len(source[hash_field])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "node-sources.json"
            path.write_text(json.dumps(sources))
            with self.assertRaisesRegex(ValueError, "do not uniquely cover"):
                verifier._validate_node(
                    ROOT / "platform/_shared-frontend/package-lock.json", path
                )

    def test_recorded_output_hash_tamper_is_rejected(self):
        metadata = json.loads((FLATPAK / "generated-sources.lock.json").read_text())
        metadata["cargo"]["outputSha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "generated-sources.lock.json"
            path.write_text(json.dumps(metadata))
            with mock.patch.object(verifier, "METADATA_PATH", path):
                with self.assertRaisesRegex(ValueError, "outputSha256"):
                    verifier._validate_metadata()


if __name__ == "__main__":
    unittest.main()
