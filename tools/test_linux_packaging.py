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
        self.appindicator = json.loads(
            (
                ROOT
                / "platform/linux/flatpak/shared-modules/libayatana-appindicator/libayatana-appindicator-gtk3.json"
            ).read_text()
        )
        self.app_module = next(
            module
            for module in self.flatpak["modules"]
            if isinstance(module, dict) and module["name"] == "liveblock-linux"
        )
        self.workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.rpm_lifecycle = (ROOT / "tools/test_linux_rpm_lifecycle.sh").read_text()
        self.packaging_docs = (ROOT / "docs/LINUX_PACKAGING.md").read_text()

    def test_runtime_permissions_are_portal_and_dri_minimized(self):
        finish = set(self.flatpak["finish-args"])
        self.assertIn("--socket=wayland", finish)
        self.assertIn("--socket=fallback-x11", finish)
        self.assertIn("--device=dri", finish)
        self.assertIn("--talk-name=org.freedesktop.portal.Desktop", finish)
        self.assertIn("--talk-name=org.kde.StatusNotifierWatcher", finish)
        self.assertNotIn("--share=network", finish)
        self.assertNotIn("--device=all", finish)
        self.assertFalse(any(value.startswith("--filesystem=") for value in finish))
        self.assertFalse(any("FileChooser" in value or "Notifications" in value for value in finish))

    def test_flatpak_uses_real_workspace_paths_and_resource_directory(self):
        commands = "\n".join(self.app_module["build-commands"])
        self.assertIn("cd platform/_shared-frontend", commands)
        self.assertIn("cd platform/linux/src-tauri", commands)
        self.assertIn("platform/linux/target/release/liveblock-linux", commands)
        self.assertIn("/app/lib/LiveBlock/resources", commands)
        self.assertIn("platform/linux/src-tauri/icons/icon.png", commands)
        self.assertNotIn("platform/linux/icons/icon.png", commands)
        self.assertNotIn("cargo tauri build --bundles deb", commands)
        self.assertEqual(self.flatpak["command"], "liveblock-linux")

    def test_flatpak_runtime_sources_match_pinned_staging_contract(self):
        sources = self.app_module["sources"]
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
        commands = "\n".join(self.app_module["build-commands"])
        self.assertIn("stage_linux_onnxruntime.py", commands)
        self.assertIn("--architecture ${FLATPAK_ARCH}", commands)

    def test_flatpak_build_inputs_are_lock_derived_and_offline(self):
        module = self.app_module
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

    def test_flatpak_pins_appindicator_runtime_and_sources(self):
        self.assertIn(
            "shared-modules/libayatana-appindicator/libayatana-appindicator-gtk3.json",
            self.flatpak["modules"],
        )
        self.assertEqual(self.appindicator["name"], "libayatana-appindicator")
        self.assertIn(
            "-DCMAKE_INSTALL_LIBDIR=lib", self.appindicator["config-opts"]
        )
        appindicator_source = self.appindicator["sources"][0]
        self.assertEqual(appindicator_source["tag"], "0.5.94")
        self.assertEqual(
            appindicator_source["commit"],
            "31e8bb083b307e1cc96af4874a94707727bd1e79",
        )
        nested = {
            module["name"]: module
            for module in self.appindicator["modules"]
            if isinstance(module, dict)
        }
        self.assertEqual(
            nested["libdbusmenu"]["sources"][0]["sha256"],
            "b9cc4a2acd74509435892823607d966d424bd9ad5d0b00938f27240a1bfa878a",
        )
        self.assertEqual(
            nested["ayatana-ido"]["sources"][0]["commit"],
            "f968079b09e2310fefc3fc307359025f1c74b3eb",
        )
        self.assertIn(
            "-DCMAKE_INSTALL_LIBDIR=lib",
            nested["ayatana-ido"]["config-opts"],
        )
        self.assertEqual(
            nested["libayatana-indicator"]["sources"][0]["commit"],
            "611bb384b73fa6311777ba4c41381a06f5b99dad",
        )
        self.assertIn(
            "-DCMAKE_INSTALL_LIBDIR=lib",
            nested["libayatana-indicator"]["config-opts"],
        )

    def test_flatpak_sdk_includes_libclang_for_pipewire_bindgen(self):
        self.assertIn(
            "org.freedesktop.Sdk.Extension.llvm18",
            self.flatpak["sdk-extensions"],
        )
        self.assertEqual(
            self.flatpak["build-options"]["env"]["LIBCLANG_PATH"],
            "/usr/lib/sdk/llvm18/lib",
        )
        self.assertIn(
            "/usr/lib/sdk/llvm18/bin",
            self.flatpak["build-options"]["append-path"],
        )

    def test_flatpak_pins_native_layer_shell_dependency(self):
        module = next(
            module
            for module in self.flatpak["modules"]
            if isinstance(module, dict) and module["name"] == "gtk-layer-shell"
        )
        source = module["sources"][0]
        self.assertEqual(source["tag"], "v0.8.2")
        self.assertEqual(
            source["commit"], "91e5ef02b557f93337bcc11ffe8c0a251aa9ab52"
        )
        self.assertIn("--libdir=lib", module["config-opts"])
        self.assertIn("-Dtests=false", module["config-opts"])

    def test_native_bundle_is_recursive_and_targets_expected_formats(self):
        bundle = self.tauri["bundle"]
        self.assertEqual(bundle["targets"], ["deb", "rpm", "appimage"])
        self.assertEqual(
            set(bundle["linux"]["deb"]["depends"]),
            {
                "libwebkit2gtk-4.1-0",
                "libgtk-3-0",
                "libayatana-appindicator3-1",
                "libgtk-layer-shell0",
                "libpipewire-0.3-0",
                "libx11-6",
                "libxcomposite1",
                "libxfixes3",
                "libxinerama1",
                "libxkbcommon0",
                "libwayland-client0",
            },
        )
        self.assertEqual(
            set(bundle["linux"]["rpm"]["depends"]),
            {
                "webkit2gtk4.1",
                "gtk3",
                "libappindicator-gtk3",
                "gtk-layer-shell",
                "pipewire-libs",
                "libX11",
                "libXcomposite",
                "libXfixes",
                "libXinerama",
                "libxkbcommon",
                "wayland-libs",
            },
        )
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

    def test_flatpak_ci_builds_without_network_permission_and_runs_lifecycle(self):
        self.assertIn("Flatpak offline build and lifecycle", self.workflow)
        self.assertIn("verify_flatpak_sources.py", self.workflow)
        self.assertIn("flatpak-builder --user --install-deps-from=flathub", self.workflow)
        self.assertIn("--disable-rofiles-fuse", self.workflow)
        self.assertIn("flatpak build-bundle", self.workflow)
        self.assertIn("com.adamnolle.LiveBlock master", self.workflow)
        self.assertIn("--artifact-type flatpak-build-only", self.workflow)
        self.assertIn("Install, inspect, launch, and uninstall build-only Flatpak", self.workflow)
        self.assertIn("sandboxNetworkPermission", self.workflow)
        self.assertIn("portalAndCompositorCertification", self.workflow)
        self.assertIn("flatpak-build-only-evidence", self.workflow)

    def test_fedora_rpm_lifecycle_is_pinned_bounded_and_inventoried(self):
        image = (
            "registry.fedoraproject.org/fedora@sha256:"
            "e70db1fd517c7d6990715fb200b1aae95ef6a1ef1492dc12c8ca25cc129a5094"
        )
        self.assertIn("Fedora 44 RPM lifecycle", self.workflow)
        self.assertIn("needs: linux", self.workflow)
        self.assertIn(image, self.workflow)
        self.assertIn("test_linux_rpm_lifecycle.sh", self.workflow)
        self.assertIn("--artifact-type rpm-lifecycle-build-only", self.workflow)
        self.assertIn("rpm-lifecycle-build-only-evidence", self.workflow)
        self.assertIn("rpm -V", self.rpm_lifecycle)
        self.assertIn("rpm -ql --dump", self.rpm_lifecycle)
        self.assertIn("timeout --kill-after=5s", self.rpm_lifecycle)
        self.assertIn("Loaded ONNX Runtime dylib with version '1.18.1'", self.rpm_lifecycle)
        self.assertIn("trusted model keyring is empty", self.rpm_lifecycle)
        self.assertIn("X11 global shortcuts registered", self.rpm_lifecycle)
        self.assertIn("uninstallRemovedApplicationPayload", self.rpm_lifecycle)
        self.assertIn('"nativeFedoraHost": False', self.rpm_lifecycle)
        self.assertIn('"hardwareCertification": False', self.rpm_lifecycle)

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
