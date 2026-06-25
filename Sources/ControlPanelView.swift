import SwiftUI

/// LiveBlock control panel — v4 dark dashboard.
///
/// Port of the design's `ScreenDashboard` (design/project/screens-app.jsx): the
/// menu-bar / tray quick panel people see most days. Header (Wordmark + a
/// "Blocking" success Pill w/ pulse dot), a big mono hero count of blocked
/// items, a Sparkline with a time axis, a three-tile card-inset quick-stats
/// grid, a primary "Mark region" CTA with a Kbd hint, a "Where it's working"
/// per-region list, and a footer kbd chord.
///
/// Liquid Glass / system controls are replaced by `Theme` surfaces + DesignKit
/// primitives. All controller wiring, bindings and behaviour are preserved.
struct ControlPanelView: View {
    @ObservedObject var controller: AppController

    // Rolling activity series feeding the hero Sparkline. Seeded flat so the
    // line always renders; appended to as the capture engine produces patches.
    @State private var spark: [Double] = Array(repeating: 0, count: 24)
    // Bump to force a re-render when capture-engine publishers (which the view
    // doesn't observe directly) change.
    @State private var liveTick = 0

    private var cm: ScreenCaptureManager { controller.captureManager }

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()

            VStack(spacing: 0) {
                header
                    .padding(.horizontal, 16)
                    .padding(.vertical, 14)
                Divider().overlay(Theme.line)

                ScrollView {
                    VStack(spacing: 16) {
                        if let problem = problemBanner {
                            banner(problem)
                        }
                        hero
                        quickStats
                        markCTA
                        secondaryActions
                        regionList
                    }
                    .padding(16)
                }

                Divider().overlay(Theme.line)
                footer
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
            }
        }
        .frame(minWidth: 360, minHeight: 360)
        .preferredColorScheme(.dark)
        .onReceive(cm.$patchesProduced) { _ in
            // Append current live-patch activity, keep a fixed-length window.
            spark.append(Double(cm.currentPatches.count))
            if spark.count > 24 { spark.removeFirst(spark.count - 24) }
        }
        .onReceive(cm.$framesPerSecond) { _ in liveTick &+= 1 }
        .onReceive(cm.$currentPatches) { _ in liveTick &+= 1 }
        .onReceive(cm.$blocksByRegion) { _ in liveTick &+= 1 }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 10) {
            LiveBlockerLogo(size: 26, cornerRadius: 7)
            Text(wordmark)
                .font(Theme.display(size: 14, weight: .bold))
                .tracking(-0.2)
                .foregroundStyle(Theme.ink1)

            statusPill

            Spacer(minLength: 8)

            // Master capture switch (replaces the old system Toggle).
            LBToggle(isOn: Binding(
                get: { controller.isRunning },
                set: { _ in controller.toggleCapture() }
            ), size: .sm)
            .help(controller.isRunning ? "Stop blocking (\u{2318}\u{21E7}L)" : "Start blocking (\u{2318}\u{21E7}L)")

            Button(action: openSettings) {
                Image(systemName: "gearshape")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(Theme.ink3)
                    .frame(width: 28, height: 28)
            }
            .buttonStyle(.plain)
            .keyboardShortcut(",", modifiers: [.command])
            .help("Open Preferences (\u{2318},)")
        }
    }

    private var wordmark: AttributedString {
        var s = AttributedString("Live")
        var b = AttributedString("Block")
        b.foregroundColor = Theme.accent
        s += b
        s += AttributedString("er")
        return s
    }

    @ViewBuilder
    private var statusPill: some View {
        if controller.isRunning {
            switch controller.pauseReason {
            case .fullscreenApp, .excludedApp, .userPaused:
                LBPill(text: "Paused", tone: .warn, size: .sm, dot: true)
            default:
                LBPill(text: "Blocking", tone: .success, size: .sm, dot: true, pulse: true)
            }
        } else {
            LBPill(text: "Off", tone: .neutral, size: .sm, dot: true)
        }
    }

    // MARK: - Hero — blocked count + sparkline

    private var hero: some View {
        VStack(alignment: .leading, spacing: 0) {
            Caption("Blocked this session")
                .padding(.bottom, 4)

            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("\(cm.patchesProduced)")
                    .font(Theme.mono(size: 52, weight: .semibold))
                    .tracking(-52 * 0.04)
                    .foregroundStyle(Theme.ink1)
                heroBadge
            }

            VStack(spacing: 6) {
                Sparkline(points: spark, color: Theme.accent, fill: true)
                    .frame(height: 56)
                HStack {
                    ForEach(["12am", "6am", "noon", "6pm", "now"], id: \.self) { t in
                        Text(t)
                            .font(Theme.ui(size: 10, weight: .regular))
                            .foregroundStyle(Theme.ink4)
                        if t != "now" { Spacer() }
                    }
                }
            }
            .padding(.top, 14)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var heroBadge: some View {
        let _ = liveTick
        if controller.isRunning, case .none = activePause {
            LBPill(text: "\(Int(cm.framesPerSecond.rounded())) fps", tone: .success, size: .sm)
        } else {
            LBPill(text: "Idle", tone: .neutral, size: .sm)
        }
    }

    private var activePause: PauseReason? {
        switch controller.pauseReason {
        case .fullscreenApp, .excludedApp, .userPaused: return controller.pauseReason
        default: return nil
        }
    }

    // MARK: - Quick stats (three card-inset tiles)

    private var quickStats: some View {
        let _ = liveTick
        let fps = cm.framesPerSecond
        let frameCost = fps > 0.001 ? String(format: "%.1f ms", 1000.0 / fps) : "—"
        return HStack(spacing: 8) {
            quickTile(label: "Regions", value: "\(controller.regionCount)", tone: Theme.success)
            quickTile(label: "Live now", value: "\(cm.currentPatches.count)", tone: Theme.info)
            quickTile(label: "Frame cost", value: frameCost, tone: Theme.ml)
        }
    }

    private func quickTile(label: String, value: String, tone: Color) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label)
                .font(Theme.ui(size: 11, weight: .regular))
                .foregroundStyle(Theme.ink3)
                .lineLimit(1)
            Text(value)
                .font(Theme.mono(size: 16, weight: .semibold))
                .tracking(-0.3)
                .foregroundStyle(tone)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 11)
        .padding(.vertical, 10)
        .lbCard(Theme.surface2, radius: Theme.Radius.r3, stroke: Theme.line)
    }

    // MARK: - Primary CTA

    private var markCTA: some View {
        LBButton(title: controller.isEditorOpen ? "Close editor" : "Mark a new region",
                 variant: .primary,
                 size: .lg,
                 systemIcon: controller.isEditorOpen ? "xmark" : "plus",
                 kbd: "\u{2318}\u{21E7}B",
                 fullWidth: true) {
            controller.toggleEditor()
        }
        .keyboardShortcut("b", modifiers: [.command, .shift])
        .help("Drag a rectangle on screen to block (\u{2318}\u{21E7}B)")
    }

    // MARK: - Secondary actions (preserved capture / label / train entry points)

    private var secondaryActions: some View {
        HStack(spacing: 8) {
            LBButton(title: "Capture", variant: .secondary, size: .md,
                     systemIcon: "camera", fullWidth: true) {
                controller.captureScreenshotForLabeling()
            }
            .keyboardShortcut("s", modifiers: [.command, .shift])
            .help("Snapshot the screen for labeling (\u{2318}\u{21E7}S). Starts capture if needed.")

            LBButton(title: "Label (\(controller.screenshotCount))",
                     variant: .secondary, size: .md,
                     systemIcon: "rectangle.and.pencil.and.ellipsis", fullWidth: true) {
                controller.showLabelingWindow()
            }
            .help("Open the labeling window")

            LBButton(title: "Train", variant: .secondary, size: .md,
                     systemIcon: "brain.head.profile", fullWidth: true) {
                controller.showTrainingDashboard()
            }
            .help("Open the Training Dashboard")
        }
    }

    // MARK: - "Where it's working" — per-region list

    private var regionList: some View {
        let _ = liveTick
        let regions = controller.regionStore.current
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("Where it's working")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink2)
                Rectangle().fill(Theme.line).frame(height: 1)
                if regions.isEmpty {
                    Text("\(regions.count) regions")
                        .font(Theme.mono(size: 11, weight: .regular))
                        .foregroundStyle(Theme.ink4)
                } else {
                    Button {
                        controller.clearRegionsWithConfirm()
                    } label: {
                        Text("Clear")
                            .font(Theme.ui(size: 11, weight: .medium))
                            .foregroundStyle(Theme.warn)
                    }
                    .buttonStyle(.plain)
                    .help("Delete every saved region")
                    Text("\(regions.count) regions")
                        .font(Theme.mono(size: 11, weight: .regular))
                        .foregroundStyle(Theme.ink4)
                }
            }

            if regions.isEmpty {
                emptyRegions
            } else {
                VStack(spacing: 2) {
                    ForEach(Array(regions.enumerated()), id: \.element.id) { idx, region in
                        regionRow(index: idx, region: region, highlight: idx == 0)
                    }
                }
            }
        }
    }

    private var emptyRegions: some View {
        HStack(spacing: 10) {
            Image(systemName: "rectangle.dashed")
                .font(.system(size: 14, weight: .regular))
                .foregroundStyle(Theme.ink4)
            Text("No regions yet — mark one to start blocking.")
                .font(Theme.ui(size: 12, weight: .regular))
                .foregroundStyle(Theme.ink3)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 12)
        .lbCard(Theme.surface2, radius: Theme.Radius.r3, stroke: Theme.line)
    }

    private static let badgePalette: [Color] = [
        Theme.accent, Theme.info, Theme.ml, Theme.success, Theme.warn,
    ]

    private func regionRow(index: Int, region: NormalizedRegion, highlight: Bool) -> some View {
        let blocks = cm.blocksByRegion[region.id] ?? 0
        let color = Self.badgePalette[index % Self.badgePalette.count]
        let initial = String(UnicodeScalar(UInt8(65 + (index % 26))))
        let w = Int((region.width * 100).rounded())
        let h = Int((region.height * 100).rounded())
        return HStack(spacing: 11) {
            AppBadge(initial: initial, color: color, size: 26)
            VStack(alignment: .leading, spacing: 1) {
                Text("Region \(index + 1)")
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .tracking(-0.06)
                    .foregroundStyle(Theme.ink1)
                HStack(spacing: 0) {
                    Text("\(blocks)")
                        .font(Theme.mono(size: 11, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                    Text(" blocked · \(w)%×\(h)%")
                        .font(Theme.ui(size: 11, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                }
            }
            Spacer(minLength: 8)
            LBToggle(isOn: Binding(
                get: { controller.regionEnabled(id: region.id) },
                set: { controller.setRegionEnabled(id: region.id, on: $0) }
            ), size: .sm)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous)
                .fill(highlight ? Theme.surface2 : Color.clear)
        )
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 10) {
            Button(action: openSettings) {
                HStack(spacing: 7) {
                    Image(systemName: "gearshape")
                        .font(.system(size: 13, weight: .regular))
                    Text("Open settings")
                        .font(Theme.ui(size: 12, weight: .medium))
                }
                .foregroundStyle(Theme.ink3)
                .padding(.horizontal, 10)
                .frame(height: 28)
            }
            .buttonStyle(.plain)
            .keyboardShortcut(",", modifiers: [.command])
            .help("Open Preferences (\u{2318},)")

            Button(role: .destructive) {
                controller.panicDisable()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "stop.circle.fill")
                    Text("Panic")
                        .font(Theme.ui(size: 12, weight: .medium))
                }
                .foregroundStyle(Theme.warn)
                .padding(.horizontal, 8)
                .frame(height: 28)
            }
            .buttonStyle(.plain)
            .help("Stop capture and hide overlays now (\u{2318}\u{21E7}\u{2325}.)")

            Spacer(minLength: 0)

            Button {
                controller.quit()
            } label: {
                Image(systemName: "power")
                    .font(.system(size: 13, weight: .regular))
                    .foregroundStyle(Theme.ink3)
                    .frame(width: 28, height: 28)
            }
            .buttonStyle(.plain)
            .keyboardShortcut("q", modifiers: [.command])
            .help("Quit LiveBlock")

            HStack(spacing: 4) {
                Kbd("\u{2318}", size: 10)
                Kbd("\u{21E7}", size: 10)
                Kbd("B", size: 10)
            }
        }
    }

    // MARK: - Problem banner (permission / start error / paused)

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

    /// Only surfaced when something needs the user's attention. The happy path
    /// is communicated by the hero + status pill instead.
    private var problemBanner: BannerState? {
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

        if controller.isRunning {
            switch controller.pauseReason {
            case .fullscreenApp(let name):
                return BannerState(
                    icon: "pause.circle.fill", tint: Theme.warn,
                    title: "Paused — \(name) is fullscreen",
                    detail: "LiveBlock pauses during fullscreen apps to stay out of the way. Toggle in Settings.",
                    action: nil)
            case .excludedApp(let name):
                return BannerState(
                    icon: "pause.circle.fill", tint: Theme.warn,
                    title: "Paused — \(name) is excluded",
                    detail: "You added this app to the excluded list. Manage in Settings → Per-App Rules.",
                    action: nil)
            case .userPaused:
                return BannerState(
                    icon: "pause.circle.fill", tint: Theme.warn,
                    title: "Paused",
                    detail: "Click Resume to start blocking again.",
                    action: nil)
            default:
                return nil
            }
        }
        return nil
    }

    private func banner(_ state: BannerState) -> some View {
        HStack(spacing: 10) {
            Image(systemName: state.icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(state.tint)
                .frame(width: 22)
            VStack(alignment: .leading, spacing: 2) {
                Text(state.title)
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                if let detail = state.detail {
                    Text(detail)
                        .font(Theme.ui(size: 11, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            if let action = state.action {
                LBButton(title: action.label, variant: .secondary, size: .sm) {
                    action.run()
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(state.tint.opacity(0.12))
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                .strokeBorder(state.tint.opacity(0.35), lineWidth: 1)
        )
    }

    // MARK: - Helpers

    private func openSettings() {
        if #available(macOS 14, *) {
            NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
        } else {
            NSApp.sendAction(Selector(("showPreferencesWindow:")), to: nil, from: nil)
        }
        NSApp.activate(ignoringOtherApps: true)
    }
}
