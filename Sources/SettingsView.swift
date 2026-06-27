import SwiftUI

/// Settings window — port of the design's `ScreenMainSettings`:
/// sidebar (220 pt) + content (flex). Each sidebar item swaps the right-pane
/// view; the active item uses a pressed-inset neumorphic surface.
struct SettingsView: View {
    @ObservedObject var controller: AppController
    @State private var section: Section = .blocking
    // Unified with VisionProcessor's default and the MLDetectorView slider
    // range (0.5...0.99) via AppController.defaultMinConfidence. Previously this
    // defaulted to 0.84 while VisionProcessor sat at 0.35 and nothing reconciled
    // them at launch — the confidence-desync bug.
    @AppStorage("minConfidence") private var minConfidenceStored: Double = AppController.defaultMinConfidence
    @State private var minConfidence: Double = AppController.defaultMinConfidence

    @AppStorage("pauseOnFullscreen") private var pauseOnFullscreen: Bool = true

    enum Section: String, CaseIterable, Identifiable {
        case blocking, regions, ml, perApp, shortcuts, about
        var id: String { rawValue }
        var label: String {
            switch self {
            case .blocking: "Blocking"
            case .regions: "Region library"
            case .ml: "ML detector"
            case .perApp: "Per-app rules"
            case .shortcuts: "Shortcuts"
            case .about: "About"
            }
        }
        var icon: String {
            switch self {
            case .blocking: "circle.slash"
            case .regions: "rectangle.dashed"
            case .ml: "brain.head.profile"
            case .perApp: "shield"
            case .shortcuts: "command"
            case .about: "info.circle"
            }
        }
    }

    var body: some View {
        // The native macOS Settings scene already provides a title bar with
        // traffic lights — adding a `LavenderBar` here produced a duplicate
        // strip. We rely on the system chrome and put the Block/Train pills
        // into the sidebar header instead.
        ZStack {
            
            HStack(alignment: .top, spacing: 0) {
                sidebar
                content
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 6) {
            Wordmark(size: 16).padding(.bottom, 12)

            VStack(spacing: 2) {
                ForEach(Section.allCases) { s in
                    sidebarRow(s)
                }
            }
            .padding(.top, 8)

            Spacer(minLength: 0)

            // Capture status block
            VStack(alignment: .leading, spacing: 6) {
                Text("CAPTURE")
                    .font(Theme.ui(size: 10, weight: .semibold))
                    .tracking(0.6)
                    .foregroundStyle(Color.secondary)
                HStack(spacing: 6) {
                    StatusDot(color: Theme.success, pulse: controller.isRunning)
                    Text(controller.isRunning ? "ScreenCaptureKit · 60 fps" : "Idle")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Color.primary)
                }
                Text("\(controller.regionCount) region\(controller.regionCount == 1 ? "" : "s") · \(controller.screenshotCount) caps")
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Color.secondary)
            }
            .padding(12)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))
        }
        .padding(.horizontal, 14).padding(.vertical, 20)
        .frame(width: 220, alignment: .topLeading)
    }

    @ViewBuilder
    private func sidebarRow(_ s: Section) -> some View {
        let active = section == s
        Button {
            withAnimation(Theme.snappy) { section = s }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: s.icon)
                    .frame(width: 16)
                    .foregroundStyle(active ? Theme.block : Color.secondary)
                Text(s.label)
                    .font(Theme.ui(size: 13, weight: active ? .semibold : .medium))
                    .foregroundStyle(active ? Theme.block : Color.primary)
                Spacer()
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
            .background {
                if active {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Color(.windowBackgroundColor))
                        .overlay(
                            InsetGlow(cornerRadius: 12)
                                .allowsHitTesting(false)
                        )
                }
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: - Content

    @ViewBuilder
    private var content: some View {
        ScrollView(.vertical) {
            VStack(spacing: 20) {
                switch section {
                case .blocking: blockingPane
                case .regions: RegionLibraryView(controller: controller)
                case .ml: MLDetectorView(
                    controller: controller,
                    minConfidence: Binding(
                        get: { minConfidenceStored },
                        set: { newValue in
                            minConfidenceStored = newValue
                            minConfidence = newValue
                            controller.captureManager.setMinimumConfidence(Float(newValue))
                        }
                    )
                )
                case .perApp: perAppPane
                case .shortcuts: shortcutsPane
                case .about: aboutPane
                }
            }
            .padding(.horizontal, 28).padding(.vertical, 24)
            .frame(maxWidth: .infinity, alignment: .top)
        }
    }

    // MARK: - Blocking pane (default)

    private var blockingPane: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Blocking")
                        .font(Theme.display(size: 28, weight: .bold))
                        .foregroundStyle(Color.primary)
                    Text("Replace marked regions with mirror-blend fill before each frame is composited to your display.")
                        .font(Theme.ui(size: 13))
                        .foregroundStyle(Color.secondary)
                }
                Spacer()
                HStack(spacing: 10) {
                    Text(controller.isRunning ? "ACTIVE" : "PAUSED")
                        .font(Theme.ui(size: 12, weight: .semibold))
                        .tracking(0.6)
                        .foregroundStyle(controller.isRunning ? Theme.success : Theme.warn)
                    Toggle("", isOn: Binding(
                        get: { controller.isRunning },
                        set: { _ in controller.toggleCapture() }))
                        .labelsHidden()
                }
            }

            // Real, live counters only — no fake data.
            HStack(spacing: 14) {
                statCard(label: "Screenshots saved",
                         value: "\(controller.screenshotCount)",
                         suffix: controller.screenshotCount == 1 ? "screenshot" : "screenshots",
                         color: Theme.block,
                         flex: 1)
                statCard(label: "Active regions",
                         value: "\(controller.regionCount)",
                         suffix: controller.regionCount == 1 ? "region" : "regions",
                         color: Theme.train, flex: 1)
                statCard(label: "Live patches",
                         value: "\(controller.captureManager.currentPatches.count)",
                         suffix: "this frame",
                         color: Theme.detect, flex: 1)
            }

            // Real, used toggles only.
            VStack(spacing: 0) {
                settingsRow(label: "Pause when game / fullscreen video is active",
                            desc: "Skip blocking while the frontmost app is fullscreen",
                            isOn: Binding(
                                get: { pauseOnFullscreen },
                                set: { newValue in
                                    pauseOnFullscreen = newValue
                                    controller.updatePauseOnFullscreen(newValue)
                                }
                            ))
            }
            .padding(6)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))

            // Auto-capture interval slider — was unreachable before.
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Auto-capture interval")
                            .font(Theme.ui(size: 13, weight: .medium))
                            .foregroundStyle(Color.primary)
                        Text("How often \u{201C}Start auto-capture\u{201D} grabs a screenshot for labeling")
                            .font(Theme.ui(size: 11))
                            .foregroundStyle(Color.secondary)
                    }
                    Spacer()
                    Text("\(Int(controller.autoCaptureIntervalSeconds)) s")
                        .font(Theme.mono(size: 12, weight: .semibold))
                        .foregroundStyle(Theme.detect)
                        .frame(width: 56, alignment: .trailing)
                }
                Slider(value: Binding(
                    get: { controller.autoCaptureIntervalSeconds },
                    set: { controller.autoCaptureIntervalSeconds = $0 }
                ), in: 5...300, step: 5)
            }
            .padding(16)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))

            // Privacy footer — replaces the "telemetry" toggle since the
            // toggle did nothing and the answer is "we don't, period."
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "lock.shield.fill")
                    .foregroundStyle(Theme.success)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Local only").font(Theme.ui(size: 12, weight: .semibold))
                        .foregroundStyle(Color.primary)
                    Text("LiveBlock makes zero network calls. No frames, telemetry, or analytics ever leave your Mac.")
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
            }
            .padding(12)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))
        }
    }

    private func settingsRow(label: String, desc: String, isOn: Binding<Bool>) -> some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(Theme.ui(size: 13, weight: .medium))
                    .foregroundStyle(Color.primary)
                Text(desc).font(Theme.ui(size: 11))
                    .foregroundStyle(Color.secondary)
            }
            Spacer()
            Toggle("", isOn: isOn)
                .labelsHidden()
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
    }

    // MARK: - Reusable building blocks

    private func statCard<Extra: View>(label: String,
                                        value: String,
                                        suffix: String,
                                        color: Color,
                                        flex: Double,
                                        @ViewBuilder extra: () -> Extra = { EmptyView() }) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(label.uppercased())
                .font(Theme.ui(size: 11, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(Color.secondary)
            HStack(alignment: .lastTextBaseline, spacing: 8) {
                Text(value)
                    .font(Theme.display(size: 44, weight: .bold))
                    .foregroundStyle(color)
                Text(suffix)
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Color.secondary)
            }
            extra()
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(in: RoundedRectangle(cornerRadius: 20))
    }

    // MARK: - Other panes

    private var perAppPane: some View {
        PerAppRulesPane(rules: controller.perAppRules,
                        currentBundleID: controller.frontmostBundleID,
                        currentName: controller.frontmostName)
    }

    /// Surfaces version, runtime data folder, and a "Show in Finder" path.
    /// Replaces the dead "Performance" placeholder pane.
    private var aboutPane: some View {
        let info = Bundle.main.infoDictionary
        let version = (info?["CFBundleShortVersionString"] as? String) ?? "?"
        let build = (info?["CFBundleVersion"] as? String) ?? "?"
        let supportDir = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("LiveBlock", isDirectory: true)

        return VStack(alignment: .leading, spacing: 18) {
            Text("About").font(Theme.display(size: 28, weight: .bold)).foregroundStyle(Color.primary)

            VStack(alignment: .leading, spacing: 10) {
                infoRow(label: "Version", value: "\(version) (build \(build))")
                infoRow(label: "Detector", value: "yolov8n CoreML (generic COCO weights — train your own from the Training tab)")
                infoRow(label: "Capture", value: "Apple ScreenCaptureKit at 60 Hz")
                infoRow(label: "Inpainter", value: "Mirror-blend (sample band, reflect across edge, cross-fade)")
                infoRow(label: "Network", value: "None. Zero outbound traffic. No telemetry.")
            }
            .padding(16)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))

            HStack(spacing: 8) {
                Button {
                    if let dir = supportDir {
                        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                        NSWorkspace.shared.open(dir)
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "folder")
                        Text("Show data folder in Finder")
                    }
                }
                .buttonStyle(.glass)
                .help(supportDir?.path ?? "")
                Button {
                    controller.restartOnboarding()
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "sparkles")
                        Text("Show onboarding again")
                    }
                }
                .buttonStyle(.glass)
                Spacer()
            }
        }
    }

    private func infoRow(label: String, value: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(label)
                .font(Theme.ui(size: 11, weight: .semibold))
                .tracking(0.4)
                .foregroundStyle(Color.secondary)
                .frame(width: 90, alignment: .leading)
            Text(value)
                .font(Theme.ui(size: 12))
                .foregroundStyle(Color.primary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }
    private var shortcutsPane: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Shortcuts").font(Theme.display(size: 28, weight: .bold)).foregroundStyle(Color.primary)
            VStack(spacing: 0) {
                ForEach([
                    ("Toggle Capture", "⌘⇧L"),
                    ("Edit Regions", "⌘⇧B"),
                    ("Capture for Labeling", "⌘⇧S"),
                    ("Panic Disable", "⌘⇧⌥ ."),
                ], id: \.0) { (label, key) in
                    HStack {
                        Text(label).font(Theme.ui(size: 13)).foregroundStyle(Color.primary)
                        Spacer()
                        Text(key).font(Theme.mono(size: 12)).foregroundStyle(Color.primary)
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                    Divider().background(Color.black.opacity(0.05))
                }
            }
            .padding(6)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))
        }
    }

}

/// Sidebar's active-row indication. Uses a thin accent stroke; the
/// background fill is provided by the parent `Color(.windowBackgroundColor)`
/// so the row reads as selected against the sidebar's translucent material.
private struct InsetGlow: View {
    let cornerRadius: CGFloat
    var body: some View {
        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
            .strokeBorder(Color.accentColor.opacity(0.25), lineWidth: 1)
    }
}
