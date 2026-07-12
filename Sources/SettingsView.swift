import SwiftUI

/// Settings window — v4 reskin of the design's `ScreenOverview`
/// (design/project/screens-app.jsx): a 230 pt sidebar (surface + hairline,
/// nav rows, live-engine card) + a content column with a 56 pt top bar and a
/// scrolling pane. The default Overview pane is a Protected hero + Blocked-today
/// counter, a KPI strip, the fill-style picker and a Behaviour list.
///
/// Dark dashboard only: `Theme.bg` root + `.preferredColorScheme(.dark)`.
/// All Liquid Glass chrome is replaced by `.lbCard` / DesignKit components.
/// Every @AppStorage / @State binding and controller wiring is preserved.
struct SettingsView: View {
    @ObservedObject var controller: AppController
    @State private var section: Section = .blocking
    @AppStorage("minConfidence") private var minConfidenceStored: Double = 0.25
    @State private var minConfidence: Double = 0.25

    @AppStorage("pauseOnFullscreen") private var pauseOnFullscreen: Bool = true
    // Visual repaint-style selection. The engine ships mirror-blend ("Smart
    // fill"); this persists the user's preferred look for the picker.
    @AppStorage("inpaintFillStyle") private var fillStyle: Int = 0

    enum Section: String, CaseIterable, Identifiable {
        case blocking, regions, ml, perApp, shortcuts, about
        var id: String { rawValue }
        var label: String {
            switch self {
            case .blocking: "Overview"
            case .regions: "Region library"
            case .ml: "Smart detection"
            case .perApp: "Apps & sites"
            case .shortcuts: "Shortcuts"
            case .about: "About"
            }
        }
        var icon: String {
            switch self {
            case .blocking: "square.grid.2x2"
            case .regions: "rectangle.dashed"
            case .ml: "sparkles"
            case .perApp: "square.stack.3d.up"
            case .shortcuts: "command"
            case .about: "info.circle"
            }
        }
        /// Accent tint for the active nav dot/icon.
        var tone: Color {
            switch self {
            case .ml: Theme.ml
            default: Theme.accent
            }
        }
    }

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            HStack(alignment: .top, spacing: 0) {
                sidebar
                content
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                LiveBlockerLogo(size: 26, cornerRadius: 7)
                Text("LiveBlocker")
                    .font(Theme.display(size: 15, weight: .bold))
                    .tracking(-0.27)
                    .foregroundStyle(Theme.ink1)
            }
            .padding(.horizontal, 4)
            .padding(.top, 2)

            VStack(spacing: 1) {
                ForEach(Section.allCases) { s in
                    sidebarRow(s)
                }
            }

            Spacer(minLength: 0)

            // Live engine card (design: card-inset + pulse dot + sparkline).
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    StatusDot(color: Theme.success, size: 7, pulse: controller.isRunning)
                    Text(controller.isRunning ? "Engine active" : "Engine idle")
                        .font(Theme.ui(size: 12, weight: .semibold))
                        .foregroundStyle(Theme.ink1)
                }
                Text(controller.isRunning
                     ? "ScreenCaptureKit · 60 fps"
                     : "Capture paused")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink3)
                Text("\(controller.regionCount) region\(controller.regionCount == 1 ? "" : "s") · \(controller.screenshotCount) caps")
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Theme.ink4)
                Sparkline(points: Self.engineSeries,
                          color: Theme.success, fill: true)
                    .frame(height: 26)
                    .padding(.top, 2)
            }
            .padding(12)
            .lbCard(Theme.surface2, radius: Theme.Radius.r4, stroke: Theme.line)
        }
        .padding(14)
        .frame(width: 230, alignment: .topLeading)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.surface)
        .overlay(alignment: .trailing) {
            Rectangle().fill(Theme.line).frame(width: 1)
        }
    }

    @ViewBuilder
    private func sidebarRow(_ s: Section) -> some View {
        let active = section == s
        Button {
            withAnimation(Theme.snappy) { section = s }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: s.icon)
                    .font(.system(size: 14, weight: .medium))
                    .frame(width: 16)
                    .foregroundStyle(active ? s.tone : Theme.ink4)
                Text(s.label)
                    .font(Theme.ui(size: 13, weight: active ? .semibold : .medium))
                    .tracking(-0.07)
                    .foregroundStyle(active ? Theme.ink1 : Theme.ink2)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous)
                    .fill(active ? Theme.surface2 : .clear)
            )
        }
        .buttonStyle(.plain)
    }

    // MARK: - Content

    @ViewBuilder
    private var content: some View {
        VStack(spacing: 0) {
            // Top bar — section title + live status pill + Mark region.
            HStack(spacing: 12) {
                Text(section.label)
                    .font(Theme.ui(size: 18, weight: .semibold))
                    .tracking(-0.36)
                    .foregroundStyle(Theme.ink1)
                LBPill(text: controller.isRunning ? "On" : "Off",
                       tone: controller.isRunning ? .success : .neutral,
                       size: .sm, dot: true, pulse: controller.isRunning)
                Spacer()
                LBButton(title: "Mark region", variant: .primary, size: .sm,
                         systemIcon: "plus", kbd: "⌘⇧K") {
                    controller.openEditor()
                }
            }
            .padding(.horizontal, 22)
            .frame(height: 56)
            .overlay(alignment: .bottom) {
                Rectangle().fill(Theme.line).frame(height: 1)
            }

            ScrollView(.vertical) {
                Group {
                    switch section {
                    case .blocking: overviewPane
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
                .padding(.horizontal, 22).padding(.vertical, 22)
                .frame(maxWidth: .infinity, alignment: .top)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    // MARK: - Overview pane (default)

    private var overviewPane: some View {
        VStack(alignment: .leading, spacing: 22) {
            heroRow
            displayTargetCard
            kpiStrip
            fillTechnique
            behaviourSection
            autoCaptureCard
            privacyFooter
        }
    }

    // ── Hero: Protected status + Blocked-today counter ──
    private var heroRow: some View {
        HStack(alignment: .top, spacing: 14) {
            protectedCard
                .frame(maxWidth: .infinity, alignment: .leading)
                .layoutPriority(1.5)
            counterCard
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var protectedCard: some View {
        ZStack(alignment: .topTrailing) {
            // Radial accent glow.
            RadialGradient(colors: [Theme.accentSoft, .clear],
                           center: .center, startRadius: 0, endRadius: 150)
                .frame(width: 220, height: 220)
                .offset(x: 60, y: -60)
                .allowsHitTesting(false)

            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 8) {
                    StatusDot(color: controller.isRunning ? Theme.success : Theme.warn,
                              size: 8, pulse: controller.isRunning)
                    Text(controller.isRunning ? "You're protected" : "Protection paused")
                        .font(Theme.ui(size: 12, weight: .semibold))
                        .foregroundStyle(controller.isRunning ? Theme.success : Theme.warn)
                }
                .padding(.bottom, 12)

                (Text("Set, and forgotten.\n")
                    .foregroundColor(Theme.ink1)
                 + Text("The detector keeps catching ads on its own, frame by frame.")
                    .foregroundColor(Theme.ink3))
                    .font(Theme.display(size: 28, weight: .semibold))
                    .tracking(-0.7)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 10) {
                    LBButton(title: "Mark region", variant: .primary, size: .md,
                             systemIcon: "plus") { controller.openEditor() }
                    LBButton(title: "Train detector", variant: .outline, size: .md,
                             systemIcon: "sparkles") { controller.showTrainingDashboard() }
                    Spacer(minLength: 8)
                    Text("Master switch")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Theme.ink3)
                    LBToggle(isOn: Binding(
                        get: { controller.isRunning },
                        set: { _ in controller.toggleCapture() }))
                }
                .padding(.top, 18)
            }
            .padding(EdgeInsets(top: 22, leading: 24, bottom: 22, trailing: 24))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r4, style: .continuous))
    }

    private var displayTargetCard: some View {
        HStack(spacing: 14) {
            Image(systemName: "display.2")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Theme.info)
                .frame(width: 34, height: 34)
                .background(Theme.info.opacity(0.12))
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text("Protected display")
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Text("LiveBlock protects one explicitly selected display and retains it across restarts.")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink3)
            }
            Spacer()
            if let selected = controller.selectedDisplayID {
                Picker("Protected display", selection: Binding(
                    get: { selected },
                    set: { controller.selectDisplay(id: $0) }
                )) {
                    ForEach(controller.availableDisplays) { display in
                        Text(display.menuLabel).tag(display.id)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: 330)
            } else {
                Text("No display available")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.accent)
            }
        }
        .padding(14)
        .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
    }

    private var counterCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            Caption("Screenshots saved")
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(controller.screenshotCount)")
                    .font(Theme.mono(size: 52, weight: .semibold))
                    .tracking(-2)
                    .foregroundStyle(Theme.accent)
                if controller.screenshotCount > 0 {
                    LBPill(text: "live", tone: .success, size: .sm)
                }
            }
            .padding(.top, 6)
            (Text("\(controller.captureManager.currentPatches.count)")
                .font(Theme.mono(size: 12))
                .foregroundColor(Theme.ink2)
             + Text(" live patches this frame · ")
                .foregroundColor(Theme.ink3)
             + Text("\(controller.regionCount)")
                .font(Theme.mono(size: 12))
                .foregroundColor(Theme.ink2)
             + Text(" active regions")
                .foregroundColor(Theme.ink3))
                .font(Theme.ui(size: 12))
                .padding(.top, 10)

            Spacer(minLength: 8)

            Sparkline(points: Self.counterSeries,
                      color: Theme.accent, fill: true)
                .frame(height: 50)
            HStack {
                Text("12am").foregroundStyle(Theme.ink4)
                Spacer()
                Text("noon").foregroundStyle(Theme.ink4)
                Spacer()
                Text("now").foregroundStyle(Theme.ink4)
            }
            .font(Theme.ui(size: 10))
            .padding(.top, 4)
        }
        .padding(EdgeInsets(top: 22, leading: 24, bottom: 22, trailing: 24))
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
    }

    // ── KPI strip ──
    private var kpiStrip: some View {
        HStack(spacing: 12) {
            kpiTile(label: "Active regions",
                    value: "\(controller.regionCount)", unit: nil,
                    sub: "of 256 max",
                    cap: min(1, Double(controller.regionCount) / 256.0),
                    color: Theme.ink1, accent: Theme.ink2)
            kpiTile(label: "Detector threshold",
                    value: "\(Int(minConfidenceStored * 100))", unit: "%",
                    sub: controller.detectionEnabled ? "auto-blocking on" : "auto-blocking off",
                    cap: minConfidenceStored,
                    color: Theme.ml, accent: Theme.ml)
            kpiTile(label: "Live patches",
                    value: "\(controller.captureManager.currentPatches.count)", unit: nil,
                    sub: "this frame",
                    cap: nil,
                    color: Theme.info, accent: Theme.info)
            kpiTile(label: "Screenshots",
                    value: "\(controller.screenshotCount)", unit: nil,
                    sub: "saved for labeling",
                    cap: nil,
                    color: Theme.success, accent: Theme.success)
        }
    }

    private func kpiTile(label: String, value: String, unit: String?,
                         sub: String, cap: Double?, color: Color, accent: Color) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Caption(label)
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                Text(value)
                    .font(Theme.mono(size: 30, weight: .semibold))
                    .tracking(-0.9)
                    .foregroundStyle(color)
                if let unit {
                    Text(unit)
                        .font(Theme.mono(size: 13))
                        .foregroundStyle(Theme.ink3)
                }
            }
            .padding(.top, 8)
            if let cap {
                LBProgress(value: cap, color: accent)
                    .padding(.top, 10)
            }
            Text(sub)
                .font(Theme.ui(size: 11))
                .foregroundStyle(Theme.ink3)
                .padding(.top, 8)
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard(Theme.surface2, radius: Theme.Radius.r4, stroke: Theme.line)
    }

    // ── Fill technique picker ──
    private struct FillOption {
        let title: String
        let desc: String
        let swatch: LinearGradient
    }

    private static let fillOptions: [FillOption] = [
        .init(title: "Smart fill", desc: "Mirror-blend the surrounding pixels",
              swatch: LinearGradient(colors: [Color(hex: 0x4A3E6A), Color(hex: 0x5E4A82)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing)),
        .init(title: "Blur", desc: "Frosted blur of the neighbours",
              swatch: LinearGradient(colors: [Color(hex: 0x3A2F54), Color(hex: 0x6A5392)],
                                     startPoint: .topLeading, endPoint: .bottomTrailing)),
        .init(title: "Average color", desc: "Mean colour of the edge",
              swatch: LinearGradient(colors: [Color(hex: 0x4A3A68), Color(hex: 0x4A3A68)],
                                     startPoint: .top, endPoint: .bottom)),
        .init(title: "Solid black", desc: "Just hide it",
              swatch: LinearGradient(colors: [Color(hex: 0x0A0A0C), Color(hex: 0x0A0A0C)],
                                     startPoint: .top, endPoint: .bottom)),
    ]

    private var fillTechnique: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .bottom) {
                LBSectionHeader(
                    title: "How blocked regions are repainted",
                    subtitle: "When LiveBlock hides a region, it needs to put something there. Pick the look that's least distracting.")
                Text("\(Self.fillOptions.count) styles")
                    .font(Theme.mono(size: 12))
                    .foregroundStyle(Theme.ink4)
            }
            HStack(spacing: 4) {
                ForEach(Array(Self.fillOptions.enumerated()), id: \.offset) { idx, opt in
                    fillTile(idx: idx, opt: opt)
                }
            }
            .padding(4)
            .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
        }
    }

    private func fillTile(idx: Int, opt: FillOption) -> some View {
        let active = fillStyle == idx
        return Button {
            withAnimation(.easeOut(duration: 0.12)) { fillStyle = idx }
            controller.captureManager.setInpaintFillStyle(idx)
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous)
                    .fill(opt.swatch)
                    .frame(height: 60)
                    .overlay(
                        RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous)
                            .strokeBorder(Theme.line, lineWidth: 1))
                    .overlay(alignment: .topTrailing) {
                        if active {
                            Image(systemName: "checkmark")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(.white)
                                .frame(width: 18, height: 18)
                                .background(Theme.accent)
                                .clipShape(Circle())
                                .padding(6)
                        }
                    }
                    .padding(.bottom, 12)
                Text(opt.title)
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .tracking(-0.07)
                    .foregroundStyle(Theme.ink1)
                Text(opt.desc)
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink3)
                    .multilineTextAlignment(.leading)
                    .padding(.top, 3)
                Spacer(minLength: 0)
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                    .fill(active ? Theme.surface2 : .clear))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                    .strokeBorder(active ? Theme.accent : .clear, lineWidth: 1))
        }
        .buttonStyle(.plain)
    }

    // ── Behaviour list ──
    private var behaviourSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            LBSectionHeader(title: "Behaviour",
                            subtitle: "Small choices that change how LiveBlock fits into your day.")
            VStack(spacing: 0) {
                behaviourRow(
                    icon: "sparkles", tone: Theme.ml,
                    label: "Auto-block what the detector finds",
                    desc: "Apply detector suggestions above your confidence threshold.",
                    isOn: Binding(
                        get: { controller.detectionEnabled },
                        set: { controller.setDetectionEnabled($0) }),
                    divider: false)
                behaviourRow(
                    icon: "play.rectangle", tone: Theme.info,
                    label: "Pause inside fullscreen video",
                    desc: "Skip blocking while the frontmost app is fullscreen.",
                    isOn: Binding(
                        get: { pauseOnFullscreen },
                        set: { newValue in
                            pauseOnFullscreen = newValue
                            controller.updatePauseOnFullscreen(newValue)
                        }),
                    divider: true)
            }
            .padding(4)
            .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
        }
    }

    private func behaviourRow(icon: String, tone: Color, label: String, desc: String,
                              isOn: Binding<Bool>, divider: Bool) -> some View {
        HStack(spacing: 14) {
            Image(systemName: icon)
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(tone)
                .frame(width: 32, height: 32)
                .background(tone.opacity(0.12))
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .tracking(-0.07)
                    .foregroundStyle(Theme.ink1)
                Text(desc)
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Theme.ink3)
            }
            Spacer()
            LBToggle(isOn: isOn)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .overlay(alignment: .top) {
            if divider { Rectangle().fill(Theme.line).frame(height: 1) }
        }
    }

    // ── Auto-capture interval ──
    private var autoCaptureCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Auto-capture interval")
                        .font(Theme.ui(size: 13, weight: .semibold))
                        .tracking(-0.07)
                        .foregroundStyle(Theme.ink1)
                    Text("How often \u{201C}Start auto-capture\u{201D} grabs a screenshot for labeling")
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Theme.ink3)
                }
                Spacer()
                Text("\(Int(controller.autoCaptureIntervalSeconds)) s")
                    .font(Theme.mono(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.ml)
                    .frame(width: 56, alignment: .trailing)
            }
            LBSlider(value: Binding(
                get: { controller.autoCaptureIntervalSeconds },
                set: { controller.autoCaptureIntervalSeconds = $0 }
            ), range: 5...300, accent: Theme.ml)
            .padding(.top, 4)
        }
        .padding(16)
        .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
    }

    // ── Privacy footer ──
    private var privacyFooter: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "lock.shield.fill")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Theme.success)
                .frame(width: 32, height: 32)
                .background(Theme.successSoft)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text("Local only")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Text("LiveBlock makes zero network calls. No frames, telemetry, or analytics ever leave your Mac.")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
        }
        .padding(12)
        .lbCard(Theme.surface2, radius: Theme.Radius.r4, stroke: Theme.line)
    }

    // MARK: - Other panes

    private var perAppPane: some View {
        PerAppRulesPane(rules: controller.perAppRules,
                        currentBundleID: controller.frontmostBundleID,
                        currentName: controller.frontmostName)
    }

    /// Surfaces version, runtime data folder, and a "Show in Finder" path.
    private var aboutPane: some View {
        let info = Bundle.main.infoDictionary
        let version = (info?["CFBundleShortVersionString"] as? String) ?? "?"
        let build = (info?["CFBundleVersion"] as? String) ?? "?"
        let supportDir = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("LiveBlock", isDirectory: true)

        return VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 0) {
                infoRow(label: "Version", value: "\(version) (build \(build))", divider: false)
                infoRow(label: "Detector", value: "liveblock-detector CoreML (open-vocabulary — blocks logos & ads with no training)", divider: true)
                infoRow(label: "Capture", value: "Apple ScreenCaptureKit at 60 Hz", divider: true)
                infoRow(label: "Screen access", value: controller.screenRecordingGranted ? "Granted" : "Not granted", divider: true)
                infoRow(label: "Accessibility", value: controller.accessibilityGranted ? "Granted" : "Not granted", divider: true)
                infoRow(label: "Inpainter", value: "Mirror-blend (sample band, reflect across edge, cross-fade)", divider: true)
                infoRow(label: "Network", value: "None. Zero outbound traffic. No telemetry.", divider: true)
            }
            .padding(4)
            .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)

            HStack(spacing: 8) {
                LBButton(title: "Show data folder in Finder", variant: .secondary, size: .md,
                         systemIcon: "folder") {
                    if let dir = supportDir {
                        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                        NSWorkspace.shared.open(dir)
                    }
                }
                LBButton(title: "Show onboarding again", variant: .outline, size: .md,
                         systemIcon: "sparkles") {
                    controller.restartOnboarding()
                }
                LBButton(title: "Export diagnostics…", variant: .outline, size: .md,
                         systemIcon: "stethoscope") {
                    controller.exportDiagnostics()
                }
                .help("Saves frame-free diagnostics; no apps, windows, labels, regions, or user paths")
                Spacer()
            }
        }
    }

    private func infoRow(label: String, value: String, divider: Bool) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(label)
                .font(Theme.ui(size: 11, weight: .semibold))
                .tracking(0.4)
                .foregroundStyle(Theme.ink3)
                .frame(width: 90, alignment: .leading)
            Text(value)
                .font(Theme.ui(size: 12))
                .foregroundStyle(Theme.ink1)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12).padding(.vertical, 12)
        .overlay(alignment: .top) {
            if divider { Rectangle().fill(Theme.line).frame(height: 1) }
        }
    }

    private var shortcutsPane: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(spacing: 0) {
                let rows: [(String, [String])] = [
                    ("Toggle Capture", ["⌘", "⇧", "L"]),
                    ("Edit Regions", ["⌘", "⇧", "B"]),
                    ("Capture for Labeling", ["⌘", "⇧", "S"]),
                    ("Panic Disable", ["⌘", "⇧", "⌥", "."]),
                ]
                ForEach(Array(rows.enumerated()), id: \.offset) { idx, row in
                    HStack {
                        Text(row.0)
                            .font(Theme.ui(size: 13, weight: .medium))
                            .foregroundStyle(Theme.ink1)
                        Spacer()
                        HStack(spacing: 4) {
                            ForEach(row.1, id: \.self) { Kbd($0) }
                        }
                    }
                    .padding(.horizontal, 14).padding(.vertical, 13)
                    .overlay(alignment: .top) {
                        if idx > 0 { Rectangle().fill(Theme.line).frame(height: 1) }
                    }
                }
            }
            .padding(4)
            .lbCard(Theme.surface, radius: Theme.Radius.r4, stroke: Theme.line)
        }
    }

    // MARK: - Illustrative sparkline series

    private static let engineSeries: [Double] =
        [2.1, 2.0, 2.2, 2.1, 2.0, 2.3, 2.1, 2.0, 2.1, 2.2, 2.0, 2.1, 2.0, 2.2, 2.1, 2.0]
    private static let counterSeries: [Double] =
        [12, 15, 11, 18, 22, 19, 28, 25, 32, 30, 38, 42, 36, 48, 55, 52, 61, 58, 72, 68, 82, 79, 91, 87]
}
