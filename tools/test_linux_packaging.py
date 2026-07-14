from __future__ import annotations

import json
import unittest
from pathlib import Path

import stage_linux_onnxruntime as staging

ROOT = Path(__file__).resolve().parents[1]


class LinuxPackagingContractTests(unittest.TestCase):
    def setUp(self):
        self.flatpak = json.loads(
            (ROOT / "platform/linux/flatpak/com.adamnolle.LiveBlock.json").read_text()
        )
        self.tauri = json.loads(
            (ROOT / "platform/linux/src-tauri/tauri.conf.json").read_text()
        )
        self.workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.packaging_docs = (ROOT / "docs/LINUX_PACKAGING.md").read_text()

    def test_runtime_permissions_are_portal_and_dri_minimized(self):
        finish = set(self.flatpak["finish-args"])
        self.assertIn("--socket=wayland", finish)
        self.assertIn("--socket=fallback-x11", finish)
        self.assertIn("--device=dri", finish)
        self.assertIn("--talk-name=org.freedesktop.portal.Desktop", finish)
        self.assertNotIn("--share=network", finish)
        self.assertNotIn("--device=all", finish)
        self.assertFalse(any(value.startswith("--filesystem=") for value in finish))
        self.assertFalse(any("FileChooser" in value or "Notifications" in value for value in finish))

    def test_flatpak_uses_real_workspace_paths_and_resource_directory(self):
        commands = "\n".join(self.flatpak["modules"][0]["build-commands"])
        self.assertIn("cd platform/_shared-frontend", commands)
        self.assertIn("cd platform/linux/src-tauri", commands)
        self.assertIn("platform/linux/target/release/liveblock-linux", commands)
        self.assertIn("/app/lib/LiveBlock/resources", commands)
        self.assertNotIn("cargo tauri build --bundles deb", commands)
        self.assertEqual(self.flatpak["command"], "liveblock-linux")

    def test_flatpak_runtime_sources_match_pinned_staging_contract(self):
        sources = self.flatpak["modules"][0]["sources"]
        by_arch = {
            source["only-arches"][0]: source
            for source in sources
            if isinstance(source, dict) and source.get("type") == "file"
        }
        for architecture, spec in staging.SPECS.items():
            source = by_arch[architecture]
            self.assertEqual(source["url"], spec.url)
            self.assertEqual(source["sha256"], spec.archive_sha256)
            self.assertEqual(
                source["dest-filename"], f"onnxruntime-{architecture}.tgz"
            )
        commands = "\n".join(self.flatpak["modules"][0]["build-commands"])
        self.assertIn("stage_linux_onnxruntime.py", commands)
        self.assertIn("--architecture ${FLATPAK_ARCH}", commands)

    def test_flatpak_build_inputs_are_lock_derived_and_offline(self):
        module = self.flatpak["modules"][0]
        self.assertIn("cargo-sources.json", module["sources"])
        self.assertIn("node-sources.json", module["sources"])
        environment = self.flatpak["build-options"]["env"]
        self.assertEqual(environment["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(environment["npm_config_offline"], "true")
        self.assertEqual(
            environment["npm_config_cache"],
            "/run/build/liveblock-linux/flatpak-node/npm-cache",
        )
        self.assertIn("npm ci --offline", "\n".join(module["build-commands"]))

    def test_native_bundle_is_recursive_and_targets_expected_formats(self):
        bundle = self.tauri["bundle"]
        self.assertEqual(bundle["targets"], ["deb", "rpm", "appimage"])
        self.assertIn("resources/**/*", bundle["resources"])
        self.assertEqual(
            self.tauri["build"]["beforeBuildCommand"],
            "cd ../_shared-frontend && npm ci && npm run build",
        )
        self.assertNotIn("npm install", self.tauri["build"]["beforeBuildCommand"])

    def test_native_ci_builds_and_closes_all_package_bytes(self):
        self.assertIn("tauri build --bundles deb,rpm,appimage --ci", self.workflow)
        self.assertIn("rpm2cpio", self.workflow)
        self.assertIn("--appimage-extract", self.workflow)
        self.assertIn("--artifact-type native-packages-build-only", self.workflow)
        self.assertIn("for format in deb rpm appimage", self.workflow)
        self.assertIn(
            '--output "$evidence/$format-payload-inventory.json"', self.workflow
        )
        self.assertIn("LICENSE.onnxruntime.txt", self.workflow)
        self.assertNotIn("onnxruntime/LICENSE.txt", self.workflow)
        self.assertIn("runtime-staging-manifest.json", self.workflow)
        self.assertIn("packages.sha256", self.workflow)

    def test_hosted_lifecycle_evidence_is_bounded_and_build_only(self):
        self.assertIn("Exercise build-only deb and AppImage lifecycle", self.workflow)
        self.assertIn("timeout --kill-after=5s 12s env", self.workflow)
        self.assertIn("xvfb-run -a dbus-run-session", self.workflow)
        self.assertIn("Loaded ONNX Runtime dylib with version '1.18.1'", self.workflow)
        self.assertIn("trusted model keyring is empty", self.workflow)
        self.assertIn("X11 global shortcuts registered", self.workflow)
        self.assertIn("debCleanInstall", self.workflow)
        self.assertIn("debUninstallRemovedSystemPayload", self.workflow)
        self.assertIn('"rpmLifecycle": "not-run-on-ubuntu"', self.workflow)
        self.assertIn('"productionModelAndTrustRoots": False', self.workflow)
        self.assertIn("RPM install/launch/uninstall remains untested", self.packaging_docs)


if __name__ == "__main__":
    unittest.main()
