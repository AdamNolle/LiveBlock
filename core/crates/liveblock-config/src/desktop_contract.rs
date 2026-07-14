//! Shared user-visible desktop behavior and capability declarations.
//!
//! Capability profiles describe intended software support, not hardware
//! certification. Real-device evidence remains a separate release gate.

use serde::{Deserialize, Serialize};

use crate::model_manifest::RUNTIME_CLASSES;

pub const DESKTOP_CONTRACT_VERSION: u32 = 1;

/// Source/debug builds may dispatch the explicit training companion. Optimized
/// release builds are always inference-only on every desktop.
pub const fn developer_training_runtime_available() -> bool {
    cfg!(debug_assertions)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPlatform {
    Macos,
    Windows,
    LinuxKdeWayland,
    LinuxWlrootsWayland,
    LinuxX11,
    LinuxGnomeWayland,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportMode {
    Full,
    Limited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCapabilityProfile {
    pub contract_version: u32,
    pub platform: DesktopPlatform,
    pub support_mode: SupportMode,
    pub capture_backend: String,
    pub inference_backends: Vec<String>,
    pub overlay_backend: String,
    pub click_through_overlay: bool,
    pub capture_exclusion: bool,
    pub global_hotkeys: bool,
    pub local_frame_processing: bool,
    pub telemetry_enabled: bool,
    pub production_training_runtime: bool,
    pub release_ready: bool,
    pub limitations: Vec<String>,
}

impl DesktopCapabilityProfile {
    pub fn macos() -> Self {
        Self {
            contract_version: DESKTOP_CONTRACT_VERSION,
            platform: DesktopPlatform::Macos,
            support_mode: SupportMode::Full,
            capture_backend: "screen_capture_kit".into(),
            inference_backends: vec!["coreml".into()],
            overlay_backend: "appkit_core_image".into(),
            click_through_overlay: true,
            capture_exclusion: true,
            global_hotkeys: true,
            local_frame_processing: true,
            telemetry_enabled: false,
            production_training_runtime: false,
            release_ready: false,
            limitations: vec![
                "real-device lifecycle and accessibility certification pending".into(),
            ],
        }
    }

    pub fn windows() -> Self {
        Self {
            contract_version: DESKTOP_CONTRACT_VERSION,
            platform: DesktopPlatform::Windows,
            support_mode: SupportMode::Limited,
            capture_backend: "windows_graphics_capture_experimental".into(),
            inference_backends: vec!["directml_experimental".into(), "cpu_experimental".into()],
            overlay_backend: "layered_webview_capture_excluded_experimental".into(),
            click_through_overlay: false,
            capture_exclusion: false,
            global_hotkeys: false,
            local_frame_processing: true,
            telemetry_enabled: false,
            production_training_runtime: false,
            release_ready: false,
            limitations: vec![
                "capture, overlay, and hotkeys are implemented but real-device GPU, DPI, lifecycle, and anti-cheat validation is incomplete".into(),
            ],
        }
    }

    pub fn linux(platform: DesktopPlatform) -> Self {
        match platform {
            DesktopPlatform::LinuxKdeWayland | DesktopPlatform::LinuxWlrootsWayland => Self {
                contract_version: DESKTOP_CONTRACT_VERSION,
                platform,
                support_mode: SupportMode::Limited,
                capture_backend: "pipewire_portal_bgra_nv12_yuy2_experimental".into(),
                inference_backends: vec!["onnx_cpu_experimental".into()],
                overlay_backend: "gtk_layer_shell_experimental".into(),
                click_through_overlay: true,
                capture_exclusion: false,
                global_hotkeys: true,
                local_frame_processing: true,
                telemetry_enabled: false,
                production_training_runtime: false,
                release_ready: false,
                limitations: vec![
                    "PipeWire capture, layer-shell overlay, and portal shortcuts are implemented but lack real-compositor certification".into(),
                ],
            },
            DesktopPlatform::LinuxX11 => Self {
                contract_version: DESKTOP_CONTRACT_VERSION,
                platform,
                support_mode: SupportMode::Limited,
                capture_backend: "xcomposite_xshm_experimental".into(),
                inference_backends: vec!["onnx_cpu_experimental".into()],
                overlay_backend: "x11_xfixes_clickthrough_experimental".into(),
                click_through_overlay: true,
                capture_exclusion: false,
                global_hotkeys: true,
                local_frame_processing: true,
                telemetry_enabled: false,
                production_training_runtime: false,
                release_ready: false,
                limitations: vec![
                    "XComposite/XShm capture, XFixes overlay, and XGrabKey shortcuts are implemented but lack real-server certification".into(),
                ],
            },
            DesktopPlatform::LinuxGnomeWayland => Self {
                contract_version: DESKTOP_CONTRACT_VERSION,
                platform,
                support_mode: SupportMode::Limited,
                capture_backend: "pipewire_portal_bgra_nv12_yuy2_experimental".into(),
                inference_backends: vec!["onnx_cpu_experimental".into()],
                overlay_backend: "tauri_preview_window_limited".into(),
                click_through_overlay: false,
                capture_exclusion: false,
                global_hotkeys: true,
                local_frame_processing: true,
                telemetry_enabled: false,
                production_training_runtime: false,
                release_ready: false,
                limitations: vec![
                    "GNOME Mutter does not provide feature-equivalent global click-through overlays; only a movable preview window is available".into(),
                ],
            },
            _ => panic!("non-Linux platform passed to Linux profile"),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.contract_version != DESKTOP_CONTRACT_VERSION {
            return Err("unsupported desktop capability contract");
        }
        if !self.local_frame_processing || self.telemetry_enabled {
            return Err("local-data policy mismatch");
        }
        if self.production_training_runtime {
            return Err("production builds must be inference-only");
        }
        if self.release_ready && self.support_mode == SupportMode::Limited {
            return Err("limited support cannot be release-ready");
        }
        if !self.release_ready && self.limitations.is_empty() {
            return Err("non-ready profiles require explicit limitations");
        }
        if self.support_mode == SupportMode::Limited && self.limitations.is_empty() {
            return Err("limited mode requires an explicit limitation");
        }
        if self.platform == DesktopPlatform::LinuxGnomeWayland && self.click_through_overlay {
            return Err("GNOME Wayland cannot claim a global click-through overlay");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopAction {
    ToggleCapture,
    ToggleRegionEditor,
    CaptureForLabeling,
    PanicDisable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyContract {
    pub action: DesktopAction,
    pub macos: String,
    pub windows_linux: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBehaviorContract {
    pub contract_version: u32,
    pub runtime_classes: Vec<String>,
    pub hotkeys: Vec<HotkeyContract>,
    pub panic_clears_capture_intent: bool,
    pub panic_cancels_recovery: bool,
    pub panic_clears_overlays: bool,
    pub panic_closes_editor: bool,
    pub runtime_inference_local: bool,
    pub frame_telemetry_networked: bool,
}

impl Default for DesktopBehaviorContract {
    fn default() -> Self {
        Self {
            contract_version: DESKTOP_CONTRACT_VERSION,
            runtime_classes: RUNTIME_CLASSES
                .iter()
                .map(|value| (*value).into())
                .collect(),
            hotkeys: vec![
                HotkeyContract {
                    action: DesktopAction::ToggleCapture,
                    macos: "Command+Shift+L".into(),
                    windows_linux: "Control+Shift+L".into(),
                },
                HotkeyContract {
                    action: DesktopAction::ToggleRegionEditor,
                    macos: "Command+Shift+B".into(),
                    windows_linux: "Control+Shift+B".into(),
                },
                HotkeyContract {
                    action: DesktopAction::CaptureForLabeling,
                    macos: "Command+Shift+S".into(),
                    windows_linux: "Control+Shift+S".into(),
                },
                HotkeyContract {
                    action: DesktopAction::PanicDisable,
                    macos: "Command+Shift+Option+Period".into(),
                    windows_linux: "Control+Shift+Alt+Period".into(),
                },
            ],
            panic_clears_capture_intent: true,
            panic_cancels_recovery: true,
            panic_clears_overlays: true,
            panic_closes_editor: true,
            runtime_inference_local: true,
            frame_telemetry_networked: false,
        }
    }
}

impl DesktopBehaviorContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        let expected: Vec<String> = RUNTIME_CLASSES
            .iter()
            .map(|value| (*value).into())
            .collect();
        if self.contract_version != DESKTOP_CONTRACT_VERSION || self.runtime_classes != expected {
            return Err("desktop behavior version or vocabulary mismatch");
        }
        if self.hotkeys.len() != 4 {
            return Err("desktop hotkey contract is incomplete");
        }
        if !self.panic_clears_capture_intent
            || !self.panic_cancels_recovery
            || !self.panic_clears_overlays
            || !self.panic_closes_editor
        {
            return Err("panic-disable semantics are incomplete");
        }
        if !self.runtime_inference_local || self.frame_telemetry_networked {
            return Err("local-data behavior mismatch");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_profile_enforces_local_inference_only() {
        let profiles = [
            DesktopCapabilityProfile::macos(),
            DesktopCapabilityProfile::windows(),
            DesktopCapabilityProfile::linux(DesktopPlatform::LinuxKdeWayland),
            DesktopCapabilityProfile::linux(DesktopPlatform::LinuxWlrootsWayland),
            DesktopCapabilityProfile::linux(DesktopPlatform::LinuxX11),
            DesktopCapabilityProfile::linux(DesktopPlatform::LinuxGnomeWayland),
        ];
        for profile in profiles {
            profile.validate().unwrap();
            assert!(profile.local_frame_processing);
            assert!(!profile.telemetry_enabled);
            assert!(!profile.production_training_runtime);
            assert!(!profile.release_ready);
        }
    }

    #[test]
    fn unfinished_windows_and_linux_paths_do_not_claim_full_support() {
        let windows = DesktopCapabilityProfile::windows();
        assert_eq!(windows.support_mode, SupportMode::Limited);
        assert!(!windows.release_ready);
        assert!(!windows.click_through_overlay);
        assert!(!windows.capture_exclusion);
        assert!(!windows.global_hotkeys);
        assert!(windows.capture_backend.contains("experimental"));

        for platform in [
            DesktopPlatform::LinuxKdeWayland,
            DesktopPlatform::LinuxWlrootsWayland,
            DesktopPlatform::LinuxX11,
        ] {
            let profile = DesktopCapabilityProfile::linux(platform);
            assert_eq!(profile.support_mode, SupportMode::Limited);
            assert!(!profile.release_ready);
            assert!(profile.global_hotkeys);
            assert!(profile.capture_backend.contains("experimental"));
        }
    }

    #[test]
    fn gnome_wayland_is_explicitly_limited() {
        let profile = DesktopCapabilityProfile::linux(DesktopPlatform::LinuxGnomeWayland);
        assert_eq!(profile.support_mode, SupportMode::Limited);
        assert!(!profile.click_through_overlay);
        assert!(!profile.limitations.is_empty());
    }

    #[test]
    fn build_profile_controls_training_runtime() {
        assert_eq!(
            developer_training_runtime_available(),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn behavior_contract_locks_vocabulary_hotkeys_and_panic() {
        let behavior = DesktopBehaviorContract::default();
        behavior.validate().unwrap();
        assert_eq!(behavior.runtime_classes, ["Logo", "Ad banner", "Sponsored"]);
        assert_eq!(
            behavior.hotkeys[3].windows_linux,
            "Control+Shift+Alt+Period"
        );
    }
}
