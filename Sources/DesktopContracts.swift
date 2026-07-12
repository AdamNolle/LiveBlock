import Foundation

/// Swift view of the shared Rust desktop contracts. The Rust values are the
/// source of truth used by Windows/Linux capability IPC and macOS diagnostics.
struct DesktopCapabilitySnapshot: Codable, Equatable {
    let contractVersion: UInt32
    let platform: String
    let supportMode: String
    let captureBackend: String
    let inferenceBackends: [String]
    let overlayBackend: String
    let clickThroughOverlay: Bool
    let captureExclusion: Bool
    let globalHotkeys: Bool
    let localFrameProcessing: Bool
    let telemetryEnabled: Bool
    let productionTrainingRuntime: Bool
    let releaseReady: Bool
    let limitations: [String]
}

struct DesktopHotkeySnapshot: Codable, Equatable {
    let action: String
    let macos: String
    let windowsLinux: String
}

struct DesktopBehaviorSnapshot: Codable, Equatable {
    let contractVersion: UInt32
    let runtimeClasses: [String]
    let hotkeys: [DesktopHotkeySnapshot]
    let panicClearsCaptureIntent: Bool
    let panicCancelsRecovery: Bool
    let panicClearsOverlays: Bool
    let panicClosesEditor: Bool
    let runtimeInferenceLocal: Bool
    let frameTelemetryNetworked: Bool
}

enum DesktopContracts {
    static func macOSCapabilities() -> DesktopCapabilitySnapshot? {
        decode(macos_capabilities_json().toString())
    }

    static func behavior() -> DesktopBehaviorSnapshot? {
        decode(desktop_behavior_contract_json().toString())
    }

    static func validateMacOS() -> Bool {
        guard let capabilities = macOSCapabilities(), let behavior = behavior() else { return false }
        return capabilities.contractVersion == 1
            && capabilities.platform == "macos"
            && capabilities.localFrameProcessing
            && !capabilities.telemetryEnabled
            && !capabilities.productionTrainingRuntime
            && behavior.contractVersion == 1
            && behavior.runtimeClasses == ["Logo", "Ad banner", "Sponsored"]
            && behavior.hotkeys.count == 4
            && behavior.panicClearsCaptureIntent
            && behavior.panicCancelsRecovery
            && behavior.panicClearsOverlays
            && behavior.panicClosesEditor
            && behavior.runtimeInferenceLocal
            && !behavior.frameTelemetryNetworked
    }

    private static func decode<T: Decodable>(_ json: String) -> T? {
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }
}
