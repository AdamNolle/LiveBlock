import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "Sources"
DESIGN_KIT = (SOURCES / "DesignKit.swift").read_text()
THEME = (SOURCES / "Theme.swift").read_text()
ONBOARDING = (SOURCES / "OnboardingView.swift").read_text()
SETTINGS = (SOURCES / "SettingsView.swift").read_text()
CONTROL = (SOURCES / "ControlPanelView.swift").read_text()
HUD = (SOURCES / "MiniHUDView.swift").read_text()
APP_VIEW_FILES = [
    SOURCES / "ControlPanelView.swift",
    SOURCES / "SettingsView.swift",
    SOURCES / "RegionLibraryView.swift",
    SOURCES / "PerAppRulesView.swift",
    SOURCES / "MLDetectorView.swift",
]


class MacAccessibilityContractTests(unittest.TestCase):
    def test_every_custom_toggle_has_voiceover_name_and_stable_identifier(self):
        combined = "\n".join(path.read_text() for path in APP_VIEW_FILES)
        self.assertEqual(combined.count("LBToggle("), 7)
        self.assertEqual(combined.count("accessibilityName:"), 7)
        toggle_identifiers = re.findall(
            r"accessibilityIdentifier:\s*\"([^\"]*(?:\\\([^)]*\))?[^\"]*)\"",
            combined,
        )
        self.assertGreaterEqual(len(toggle_identifiers), 7)
        self.assertIn("var accessibilityName: String", DESIGN_KIT)
        self.assertIn("var accessibilityIdentifier: String", DESIGN_KIT)
        self.assertIn('.accessibilityValue(Text(isOn ? "On" : "Off"))', DESIGN_KIT)

    def test_reduce_motion_and_decorative_elements_are_respected(self):
        self.assertIn("@Environment(\\.accessibilityReduceMotion)", DESIGN_KIT)
        self.assertIn("if reduceMotion", DESIGN_KIT)
        self.assertIn("@Environment(\\.accessibilityReduceMotion)", THEME)
        self.assertIn("if pulse && !reduceMotion", THEME)
        self.assertIn(".accessibilityHidden(true)", THEME)
        self.assertIn(".accessibilityHidden(true)", DESIGN_KIT)

    def test_critical_surfaces_publish_stable_accessibility_identifiers(self):
        for identifier in (
            "control-panel.capture-toggle",
            "control-panel.open-settings",
            "control-panel.mark-region",
            "control-panel.capture-labeling-frame",
            "control-panel.open-labeling",
            "control-panel.open-training",
        ):
            self.assertIn(identifier, CONTROL)
        self.assertIn("mini-hud.capture-toggle", HUD)
        self.assertIn("onboarding.allow-screen-recording", ONBOARDING)
        self.assertIn("onboarding.allow-accessibility", ONBOARDING)

    def test_user_visible_product_and_runtime_claims_are_current(self):
        visible_sources = "\n".join(path.read_text() for path in SOURCES.glob("*.swift"))
        self.assertNotRegex(visible_sources, r'Text\([^\n]*LiveBlocker')
        for stale in (
            "ScreenCaptureKit · 60 fps",
            "Apple ScreenCaptureKit at 60 Hz",
            "sandboxed GPU pipeline",
            "lets blocks snap to real UI elements",
        ):
            self.assertNotIn(stale, visible_sources)
        self.assertIn("ScreenCaptureKit · %.0f fps measured", SETTINGS)
        self.assertIn('Text("LiveBlock")', ONBOARDING)
        self.assertIn('Text("LiveBlock")', SETTINGS)

    def test_macos_source_deprecations_are_absent(self):
        combined = "\n".join(path.read_text() for path in SOURCES.glob("*.swift"))
        self.assertNotRegex(combined, r"\+\s*Text\(")
        self.assertNotIn("NSUserNotification()", combined)
        self.assertNotIn("NSUserNotificationCenter.", combined)

    def test_onboarding_explains_only_permissions_used_by_runtime_features(self):
        self.assertIn("case screenCapture, accessibility", ONBOARDING)
        self.assertIn("requestAccessibility", ONBOARDING)
        self.assertIn("openSystemSettings(.accessibility)", ONBOARDING)
        self.assertIn("global keyboard shortcuts", ONBOARDING)
        self.assertNotIn("snap to real UI elements", ONBOARDING)
        self.assertIn('Text("Two required grants.")', ONBOARDING)


if __name__ == "__main__":
    unittest.main()
