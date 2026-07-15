import AppKit
import Foundation

/// Frame-free, process-free support snapshot suitable for attaching to a bug
/// report. Never add screenshots, window titles, frontmost app metadata, user
/// paths, labels, regions, or model inputs to this payload.
struct DiagnosticsReport: Encodable, Equatable {
    struct Runtime: Encodable, Equatable {
        let appVersion: String
        let appBuild: String
        let osVersion: String
        let architecture: String
    }

    struct PermissionSnapshot: Encodable, Equatable {
        let screenRecording: Bool
        let accessibility: Bool
    }

    struct CaptureSnapshot: Encodable, Equatable {
        let desired: Bool
        let running: Bool
        let detectionEnabled: Bool
        let pauseReason: String
        let hasStartError: Bool
        let framesPerSecond: Double
        let renderMilliseconds: Double
        let droppedRenderFrames: Int
        let reusedStaticFrames: Int
        let systemSuspensions: [String]
    }

    struct DisplaySnapshot: Encodable, Equatable {
        let id: UInt32
        let pointX: Double
        let pointY: Double
        let pointWidth: Double
        let pointHeight: Double
        let pixelWidth: Int
        let pixelHeight: Int
        let isMain: Bool
        let isSelected: Bool
    }

    struct PrivacyDeclaration: Encodable, Equatable {
        let containsFrameData = false
        let containsProcessList = false
        let containsWindowTitles = false
        let containsUserPaths = false
    }

    let schema: Int
    let generatedAt: Date
    let runtime: Runtime
    let permissions: PermissionSnapshot
    let capture: CaptureSnapshot
    let displays: [DisplaySnapshot]
    let privacy: PrivacyDeclaration

    static func pauseReasonCode(_ reason: PauseReason) -> String {
        switch reason {
        case .none: return "none"
        case .stopped: return "stopped"
        case .userPaused: return "user_paused"
        case .fullscreenApp: return "fullscreen_app"
        case .excludedApp: return "excluded_app"
        case .permissionDenied: return "permission_denied"
        case .startError: return "start_error"
        }
    }

    @MainActor
    static func capture(from controller: AppController, generatedAt: Date = Date()) -> DiagnosticsReport {
        let info = Bundle.main.infoDictionary
        let manager = controller.captureManager
        let displays = controller.availableDisplays.map { display in
            DisplaySnapshot(
                id: display.id,
                pointX: Double(display.pointFrame.origin.x),
                pointY: Double(display.pointFrame.origin.y),
                pointWidth: Double(display.pointFrame.width),
                pointHeight: Double(display.pointFrame.height),
                pixelWidth: display.pixelWidth,
                pixelHeight: display.pixelHeight,
                isMain: display.isMain,
                isSelected: display.id == controller.selectedDisplayID
            )
        }
        return DiagnosticsReport(
            schema: 1,
            generatedAt: generatedAt,
            runtime: Runtime(
                appVersion: info?["CFBundleShortVersionString"] as? String ?? "unknown",
                appBuild: info?["CFBundleVersion"] as? String ?? "unknown",
                osVersion: ProcessInfo.processInfo.operatingSystemVersionString,
                architecture: architectureName
            ),
            permissions: PermissionSnapshot(
                screenRecording: controller.screenRecordingGranted,
                accessibility: controller.accessibilityGranted
            ),
            capture: CaptureSnapshot(
                desired: manager.captureDesired,
                running: manager.isRunning,
                detectionEnabled: manager.detectionEnabled,
                pauseReason: pauseReasonCode(controller.pauseReason),
                hasStartError: manager.lastStartError != nil,
                framesPerSecond: manager.framesPerSecond,
                renderMilliseconds: manager.renderMilliseconds,
                droppedRenderFrames: manager.droppedRenderFrames,
                reusedStaticFrames: manager.reusedStaticFrames,
                systemSuspensions: manager.systemSuspensionReasons.map { reason in
                    switch reason {
                    case .systemSleep: return "system_sleep"
                    case .displaysAsleep: return "displays_asleep"
                    case .sessionLocked: return "session_locked"
                    }
                }.sorted()
            ),
            displays: displays,
            privacy: PrivacyDeclaration()
        )
    }

    func write(to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        try encoder.encode(self).write(to: url, options: .atomic)
    }
}

private var architectureName: String {
#if arch(arm64)
    return "arm64"
#elseif arch(x86_64)
    return "x86_64"
#else
    return "unknown"
#endif
}
