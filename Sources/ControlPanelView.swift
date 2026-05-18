import SwiftUI

/// LiveBlocker control panel — port of the design's menu-bar-dropdown screen.
struct ControlPanelView: View {
    @ObservedObject var controller: AppController

    var body: some View {
        ZStack {

            VStack(spacing: Theme.Spacing.l) {
                header
                statusBanner
                primaryActions
                regionList
                Spacer(minLength: 0)
                footer
            }
            .padding(Theme.Spacing.l)
        }
    }

    // MARK: - Status banner — single source of truth the user can always trust

    @ViewBuilder
    private var statusBanner: some View {
        let state = bannerState
        HStack(spacing: 10) {
            Image(systemName: state.icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(state.tint)
                .frame(width: 22)
            VStack(alignment: .leading, spacing: 2) {
                Text(state.title)
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Color.primary)
                if let detail = state.detail {
                    Text(detail)
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            if let action = state.action {
                Button {
                    action.run()
                } label: {
                    Text(action.label)
                        .font(Theme.ui(size: 11, weight: .semibold))
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .tint(state.tint)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(state.tint.opacity(0.12))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(state.tint.opacity(0.35), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private struct BannerAction {
        let label: String
        let run: () -> Void
    }
    private struct BannerState {
        var icon: String
        var tint: Color
        var title: String
        var detail: String?
        var action: BannerAction?
    }

    private var bannerState: BannerState {
        let cm = controller.captureManager

        // Highest-priority: missing permission. Nothing else matters until this is fixed.
        if !controller.screenRecordingGranted {
            return BannerState(
                icon: "exclamationmark.shield.fill",
                tint: Theme.warn,
                title: "Screen Recording permission needed",
                detail: "Grant LiveBlock permission so it can see what's on your screen.",
                action: BannerAction(label: "Open Settings",
                                     run: { Permissions.openSystemSettings(.screenRecording) })
            )
        }

        // start() blew up — surface the actual error rather than letting the
        // toggle silently flip back.
        if let err = cm.lastStartError, !err.isEmpty {
            return BannerState(
                icon: "xmark.octagon.fill",
                tint: Theme.warn,
                title: "Couldn't start blocking",
                detail: err,
                action: BannerAction(label: "Try again",
                                     run: { controller.toggleCapture() })
            )
        }

        // Capture is running — we're either green-blocking or amber-paused.
        if controller.isRunning {
            switch controller.pauseReason {
            case .fullscreenApp(let name):
                return BannerState(
                    icon: "pause.circle.fill",
                    tint: Theme.warn,
                    title: "Paused — \(name) is fullscreen",
                    detail: "LiveBlock pauses during fullscreen apps to stay out of the way. Toggle in Settings.",
                    action: nil
                )
            case .excludedApp(let name):
                return BannerState(
                    icon: "pause.circle.fill",
                    tint: Theme.warn,
                    title: "Paused — \(name) is excluded",
                    detail: "You added this app to the excluded list. Manage in Settings → Per-App Rules.",
                    action: nil
                )
            case .userPaused:
                return BannerState(
                    icon: "pause.circle.fill",
                    tint: Theme.warn,
                    title: "Paused",
                    detail: "Click Resume to start blocking again.",
                    action: nil
                )
            default:
                let regions = controller.regionCount
                let live = cm.currentPatches.count
                let fps = Int(cm.framesPerSecond.rounded())
                return BannerState(
                    icon: "checkmark.shield.fill",
                    tint: Theme.success,
                    title: "Blocking · \(fps) fps",
                    detail: "\(regions) region\(regions == 1 ? "" : "s") · \(live) live patch\(live == 1 ? "" : "es")",
                    action: nil
                )
            }
        }

        // Idle — green light, ready to go.
        return BannerState(
            icon: "shield.lefthalf.filled",
            tint: Theme.block,
            title: "Ready",
            detail: controller.regionCount > 0
                ? "\(controller.regionCount) region\(controller.regionCount == 1 ? "" : "s") drawn. Click Block to start."
                : "Draw a region first, or just click Block to use the detector.",
            action: BannerAction(label: "Block",
                                 run: { controller.toggleCapture() })
        )
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 10) {
            LiveBlockerLogo(size: 36, cornerRadius: 10)
            VStack(alignment: .leading, spacing: 2) {
                Text("LiveBlock")
                    .font(Theme.display(size: 15, weight: .bold))
                    .foregroundStyle(Color.primary)
                HStack(spacing: 5) {
                    StatusDot(color: stateColor, size: 6, pulse: controller.isRunning)
                    Text(stateLine)
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Color.secondary)
                }
            }
            Spacer()
            Toggle("", isOn: Binding(
                get: { controller.isRunning },
                set: { _ in controller.toggleCapture() }
            ))
            .labelsHidden()
            .help(controller.isRunning ? "Stop blocking (\u{2318}\u{21E7}L)" : "Start blocking (\u{2318}\u{21E7}L)")
        }
    }

    // MARK: - Primary actions

    private var primaryActions: some View {
        HStack(spacing: 8) {
            Button {
                controller.toggleEditor()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "rectangle.dashed")
                    Text(controller.isEditorOpen ? "Close editor" : "Draw region")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent).tint(Theme.block)
            .keyboardShortcut("b", modifiers: [.command, .shift])
            .help("Drag a rectangle on screen to block (\u{2318}\u{21E7}B)")

            Button {
                controller.captureScreenshotForLabeling()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "camera")
                    Text("Capture")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent).tint(Theme.detect)
            .keyboardShortcut("s", modifiers: [.command, .shift])
            .help("Snapshot the screen for labeling (\u{2318}\u{21E7}S). Starts capture if needed.")

            Button {
                controller.showTrainingDashboard()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "brain.head.profile")
                    Text("Train")
                }
                .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent).tint(Theme.train)
            .help("Open the Training Dashboard")
        }
    }

    // MARK: - Region list (label + clear-regions entry points)

    private var regionList: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("THIS SESSION")
                .font(Theme.ui(size: 10, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(Color.secondary)
            VStack(spacing: 4) {
                rowButton(label: "Label screenshots  (\(controller.screenshotCount))",
                          systemImage: "rectangle.and.pencil.and.ellipsis",
                          accent: Theme.detect,
                          help: "Open the labeling window") {
                    controller.showLabelingWindow()
                }
                rowButton(label: "Manage regions  (\(controller.regionCount))",
                          systemImage: "square.dashed.inset.filled",
                          accent: Theme.train,
                          help: "View, rename, toggle, or delete saved regions") {
                    if #available(macOS 14, *) {
                        NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
                    } else {
                        NSApp.sendAction(Selector(("showPreferencesWindow:")), to: nil, from: nil)
                    }
                    NSApp.activate(ignoringOtherApps: true)
                }
                if controller.regionCount > 0 {
                    rowButton(label: "Clear regions  (\(controller.regionCount))",
                              systemImage: "trash",
                              accent: Theme.warn,
                              role: .destructive,
                              help: "Delete every saved region") {
                        controller.clearRegionsWithConfirm()
                    }
                }
            }
            .padding(4)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))
        }
    }

    @ViewBuilder
    private func rowButton(label: String,
                           systemImage: String,
                           accent: Color,
                           enabled: Bool = true,
                           role: ButtonRole? = nil,
                           help: String? = nil,
                           action: @escaping () -> Void) -> some View {
        Button(role: role, action: action) {
            HStack(spacing: 10) {
                Image(systemName: systemImage)
                    .frame(width: 16)
                    .foregroundStyle(accent)
                Text(label)
                    .font(Theme.ui(size: 12, weight: .medium))
                    .foregroundStyle(Color.primary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Color.secondary.opacity(0.6))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(Color.white.opacity(0.5))
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.5)
        .help(help ?? label)
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 8) {
            Button {
                if #available(macOS 14, *) {
                    NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
                } else {
                    NSApp.sendAction(Selector(("showPreferencesWindow:")), to: nil, from: nil)
                }
                NSApp.activate(ignoringOtherApps: true)
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "gearshape")
                    Text("Preferences")
                }
            }
            .buttonStyle(.glass).controlSize(.small)
            .keyboardShortcut(",", modifiers: [.command])
            .help("Open Preferences (\u{2318},)")

            Spacer()

            Button(role: .destructive) {
                controller.panicDisable()
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "stop.circle.fill")
                    Text("Panic")
                }
            }
            .buttonStyle(.glass).controlSize(.small)
            .tint(Theme.warn)
            .help("Stop capture and hide overlays now (\u{2318}\u{21E7}\u{2325}.)")

            Button {
                controller.quit()
            } label: {
                Image(systemName: "power")
            }
            .buttonStyle(.glass).controlSize(.small)
            .keyboardShortcut("q", modifiers: [.command])
            .help("Quit LiveBlock")
        }
    }

    // MARK: - State

    private var stateColor: Color { controller.isRunning ? Theme.success : Theme.warn }
    private var stateLine: String {
        controller.isRunning ? "Active · \(controller.regionCount) region\(controller.regionCount == 1 ? "" : "s")"
                             : "Idle"
    }
}
