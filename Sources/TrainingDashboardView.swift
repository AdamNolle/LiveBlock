import SwiftUI

struct TrainingDashboardView: View {
    @ObservedObject var controller: AppController
    @ObservedObject var training: TrainingController
    @ObservedObject var labeling: LabelingController

    @State private var epochsField: Int = 50
    @State private var batchField: Int = 8
    @State private var imgszField: Int = 640

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.l) {
                    header
                    preconditionCard
                    errorBanner
                    progressTrack
                    statsCard
                    trainingCard
                    logCard
                }
                .padding(Theme.Spacing.xl)
            }
        }
        .preferredColorScheme(.dark)
        .onAppear { training.recheckVenv() }
    }

    /// "Set up training environment" precondition — surfaced *before* the
    /// user clicks Train Now and gets a cryptic error. Detects missing
    /// `tools/.venv` and offers a one-click install.
    @ViewBuilder
    private var preconditionCard: some View {
        if !training.venvInstalled {
            HStack(spacing: 10) {
                Image(systemName: "wrench.and.screwdriver.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(Theme.ml)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Set up training environment")
                        .font(Theme.ui(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.ink1)
                    Text("First-time only: installs Python + ultralytics + coremltools (\u{007E}1\u{2013}3 minutes).")
                        .font(Theme.ui(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                }
                Spacer(minLength: 0)
                LBButton(
                    title: training.isInstallingEnvironment ? "Installing\u{2026}" : "Install now",
                    variant: .accent,
                    size: .sm,
                    systemIcon: training.isInstallingEnvironment ? "hourglass" : "arrow.down.circle.fill"
                ) {
                    training.installEnvironment()
                }
                .disabled(training.isInstallingEnvironment)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .lbCard(Theme.mlSoft, radius: Theme.Radius.r4, stroke: Theme.ml.opacity(0.35))
        }
    }

    /// Surfaces training failures inline so they don't get buried in the log
    /// card. Two recognized "well-known" errors point the user at the fix:
    /// missing `tools/.venv` (run `setup_env.sh`) and missing labeled data
    /// (capture + label first).
    @ViewBuilder
    private var errorBanner: some View {
        if let err = bannerError {
            HStack(spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(Theme.warn)
                VStack(alignment: .leading, spacing: 4) {
                    Text(err.title)
                        .font(Theme.ui(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.ink1)
                    Text(err.detail)
                        .font(Theme.ui(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                        .fixedSize(horizontal: false, vertical: true)
                    if let hint = err.hint {
                        Text(hint)
                            .font(Theme.mono(size: 12, weight: .regular))
                            .foregroundStyle(Theme.ink2)
                            .padding(.top, 2)
                    }
                }
                Spacer(minLength: 0)
                LBButton(title: "Dismiss", variant: .outline, size: .sm) {
                    training.clearTerminalState()
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .lbCard(Theme.warnSoft, radius: Theme.Radius.r4, stroke: Theme.warn.opacity(0.35))
        }
    }

    private struct TrainingErrorPresentation {
        var title: String
        var detail: String
        var hint: String?
    }

    private var bannerError: TrainingErrorPresentation? {
        guard case .finished(success: false, _) = training.state,
              let err = training.lastError else { return nil }
        let lower = err.lowercased()
        if lower.contains(".venv") || lower.contains("missing") && lower.contains("python") {
            return TrainingErrorPresentation(
                title: "Python training environment missing",
                detail: "Training needs a Python virtualenv with ultralytics + coremltools. Run the one-time setup, then click Train Now again.",
                hint: "tools/setup_env.sh"
            )
        }
        if lower.contains("data.yaml") || lower.contains("no labels") || lower.contains("export") {
            return TrainingErrorPresentation(
                title: "No labeled data yet",
                detail: "Capture screenshots (⌘⇧S while blocking is on) and label at least 20 of them in the Label window. Then come back here.",
                hint: nil
            )
        }
        return TrainingErrorPresentation(
            title: "Training failed",
            detail: err,
            hint: nil
        )
    }

    /// 3-step progress indicator: Capture → Label → Train. Each cell shows
    /// the current count and a checkmark when "ready" — gives a fresh user
    /// an obvious next step instead of a wall of stats.
    private var progressTrack: some View {
        HStack(spacing: Theme.Spacing.m) {
            stepCard(
                index: 1,
                title: "Capture",
                detail: "\(labeling.totalCount) screenshot\(labeling.totalCount == 1 ? "" : "s")",
                hint: controller.isRunning ? "Press ⌘⇧S while you see an ad" : "Start capture first",
                done: labeling.totalCount >= 5,
                accent: Theme.accent,
                action: { controller.captureScreenshotForLabeling() }
            )
            chev()
            stepCard(
                index: 2,
                title: "Label",
                detail: "\(labeling.labeledCount) of \(labeling.totalCount) labeled",
                hint: labeling.totalCount == 0
                    ? "Capture some screenshots first"
                    : "Drag a rectangle around each ad",
                done: labeling.labeledCount >= 20,
                accent: Theme.ml,
                action: { controller.showLabelingWindow() }
            )
            chev()
            stepCard(
                index: 3,
                title: "Train",
                detail: training.isBusy ? "Running…" : (training.lastSuccessAt != nil ? "Ready" : "Not yet"),
                hint: labeling.labeledCount < 20
                    ? "Label at least 20 screenshots for a usable model"
                    : "Click Train Now below",
                done: training.lastSuccessAt != nil,
                accent: Theme.info,
                action: nil
            )
        }
    }

    private func chev() -> some View {
        Image(systemName: "chevron.right")
            .font(.system(size: 14, weight: .semibold))
            .foregroundStyle(Theme.ink4)
    }

    private func stepCard(index: Int,
                          title: String,
                          detail: String,
                          hint: String,
                          done: Bool,
                          accent: Color,
                          action: (() -> Void)?) -> some View {
        let card = VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                ZStack {
                    Circle().fill(done ? Theme.successSoft : accent.opacity(0.18))
                        .frame(width: 22, height: 22)
                    if done {
                        Image(systemName: "checkmark")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(Theme.success)
                    } else {
                        Text("\(index)")
                            .font(Theme.ui(size: 11, weight: .bold))
                            .foregroundStyle(accent)
                    }
                }
                Text(title)
                    .font(Theme.ui(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Spacer()
            }
            Text(detail)
                .font(Theme.ui(size: 18, weight: .semibold))
                .foregroundStyle(Theme.ink1)
            Text(hint)
                .font(Theme.ui(size: 12, weight: .regular))
                .foregroundStyle(Theme.ink3)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(Theme.Spacing.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard()

        return Group {
            if let action = action {
                Button(action: action) { card }
                    .buttonStyle(.plain)
            } else {
                card
            }
        }
    }

    private var header: some View {
        HStack(spacing: Theme.Spacing.m) {
            ZStack {
                Circle()
                    .fill(Theme.successSoft)
                    .frame(width: 48, height: 48)
                Image(systemName: "brain.fill")
                    .font(.title)
                    .foregroundStyle(Theme.success)
                    .symbolEffect(.pulse, isActive: training.isBusy)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text("Training Dashboard")
                    .font(Theme.display(size: 22, weight: .bold))
                    .foregroundStyle(Theme.ink1)
                    .tracking(-0.3)
                Text("Build your own ad detector from your labeled screenshots.")
                    .font(Theme.ui(size: 13, weight: .regular))
                    .foregroundStyle(Theme.ink3)
            }
            Spacer()
            if let last = training.lastSuccessAt {
                VStack(alignment: .trailing, spacing: 2) {
                    Eyebrow("Last train")
                    Text(last, style: .relative)
                        .font(Theme.mono(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink2)
                }
            }
        }
    }

    private var statsCard: some View {
        HStack(spacing: Theme.Spacing.m) {
            stat(label: "Screenshots", value: "\(labeling.totalCount)", icon: "photo.stack")
            stat(label: "Labeled", value: "\(labeling.labeledCount)", icon: "checkmark.seal")
            stat(label: "Remaining",
                 value: "\(max(0, labeling.totalCount - labeling.labeledCount))",
                 icon: "tray")
            stat(label: "Regions", value: "\(controller.regionCount)", icon: "rectangle.dashed")
        }
    }

    private func stat(label: String, value: String, icon: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.accent)
                Caption(label)
            }
            Text(value)
                .font(Theme.mono(size: 28, weight: .semibold))
                .tracking(-1)
                .foregroundStyle(Theme.ink1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Theme.Spacing.m)
        .lbCard()
    }

    private var trainingCard: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.m) {
            HStack {
                Text("Training")
                    .font(Theme.ui(size: 17, weight: .semibold))
                    .tracking(-0.18)
                    .foregroundStyle(Theme.ink1)
                Spacer()
                stateLabel
            }
            HStack(spacing: Theme.Spacing.m) {
                stepper("Epochs", value: $epochsField, range: 1...500, step: 5)
                stepper("Batch", value: $batchField, range: 1...64, step: 1)
                stepper("Img sz", value: $imgszField, range: 320...1280, step: 32)
            }
            actionRow
            if case .training(let p) = training.state {
                progressRow(progress: p)
            }
        }
        .padding(Theme.Spacing.l)
        .lbCard(Theme.surface, radius: Theme.Radius.r5)
    }

    private func stepper(_ label: String, value: Binding<Int>, range: ClosedRange<Int>, step: Int) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Caption(label)
            HStack(spacing: 6) {
                Text("\(value.wrappedValue)")
                    .font(Theme.mono(size: 14, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Spacer(minLength: 0)
                Stepper(value: value, in: range, step: step) {
                    EmptyView()
                }
                .labelsHidden()
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard(Theme.surface2, radius: Theme.Radius.r3, stroke: Theme.line)
    }

    private var trainNowDisabledReason: String? {
        if !training.venvInstalled { return "Install the training environment first." }
        if labeling.labeledCount < 20 { return "Label at least 20 screenshots first (currently \(labeling.labeledCount))." }
        return nil
    }

    private var actionRow: some View {
        HStack(spacing: Theme.Spacing.s) {
            if training.isBusy {
                LBButton(title: "Cancel", variant: .primary, size: .lg,
                         systemIcon: "stop.fill", fullWidth: true) {
                    training.cancel()
                }
            } else {
                LBButton(title: "Train Now", variant: .primary, size: .lg,
                         systemIcon: "play.fill", fullWidth: true) {
                    training.startTraining(epochs: epochsField, imgsz: imgszField, batch: batchField)
                }
                .disabled(labeling.labeledCount < 20 || !training.venvInstalled)
                .opacity(labeling.labeledCount < 20 || !training.venvInstalled ? 0.5 : 1)
                .help(trainNowDisabledReason ?? "Train a model on your labeled screenshots")
            }
            LBButton(title: "Label more", variant: .secondary, size: .lg,
                     systemIcon: "rectangle.and.pencil.and.ellipsis", fullWidth: true) {
                controller.showLabelingWindow()
            }
        }
    }

    @ViewBuilder
    private func progressRow(progress: TrainingController.TrainingProgress) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Epoch \(progress.epoch) / \(progress.totalEpochs)")
                    .font(Theme.ui(size: 13, weight: .medium))
                    .foregroundStyle(Theme.ink1)
                Spacer()
                if let map50 = progress.map50 {
                    Text(String(format: "mAP@50  %.3f", map50))
                        .font(Theme.mono(size: 12, weight: .regular))
                        .foregroundStyle(Theme.success)
                }
                if let map = progress.map50_95 {
                    Text(String(format: "mAP@50-95  %.3f", map))
                        .font(Theme.mono(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                }
            }
            LBProgress(value: training.progressFraction, color: Theme.info)
            HStack(spacing: 16) {
                Label(String(format: "box %.3f", progress.box_loss), systemImage: "square.dashed")
                    .font(Theme.mono(size: 11, weight: .regular))
                    .foregroundStyle(Theme.ink3)
                Label(String(format: "cls %.3f", progress.cls_loss), systemImage: "tag")
                    .font(Theme.mono(size: 11, weight: .regular))
                    .foregroundStyle(Theme.ink3)
            }
        }
    }

    private var stateLabel: some View {
        Group {
            switch training.state {
            case .idle:
                LBPill(text: "Idle", tone: .neutral, dot: true)
            case .exporting:
                LBPill(text: "Exporting", tone: .ml, dot: true)
            case .training:
                LBPill(text: "Training", tone: .accent, dot: true, pulse: true)
            case .installing:
                LBPill(text: "Installing", tone: .info, dot: true, pulse: true)
            case .rebuilding:
                LBPill(text: "Rebuilding", tone: .info, dot: true, pulse: true)
            case .finished(let success, _):
                LBPill(text: success ? "Done" : "Failed",
                       tone: success ? .success : .accent,
                       dot: true)
            }
        }
    }

    /// Parsed user-facing milestones from the raw log torrent. Recognizes:
    /// "Setup OK", "Export OK", "Epoch N/M", "mAP@50 0.61", "Pipeline OK", "FAILED".
    private var milestones: [String] {
        var out: [String] = []
        for line in training.logTail {
            if line.hasPrefix("=== Training pipeline started") { out.append("Started training pipeline.") }
            else if line.hasPrefix("=== Installing training environment") { out.append("Installing Python + ultralytics\u{2026}") }
            else if line.hasPrefix("=== Environment installed OK") { out.append("Training environment ready.") }
            else if line.hasPrefix("=== Pipeline OK") { out.append("Training complete and model installed.") }
            else if line.hasPrefix("FAILED") { out.append(line) }
            else if line.contains("Hot-reloaded model") { out.append("New model loaded into the running app.") }
        }
        // Add the latest epoch summary if we have one.
        if case .training(let p) = training.state, p.totalEpochs > 0 {
            var msg = "Epoch \(p.epoch) / \(p.totalEpochs)"
            if let m50 = p.map50 { msg += " \u{00B7} mAP@50 \(String(format: "%.2f", m50))" }
            out.append(msg)
        }
        return out.suffix(8).map { $0 }
    }

    private var logCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Progress")
                    .font(Theme.ui(size: 17, weight: .semibold))
                    .tracking(-0.18)
                    .foregroundStyle(Theme.ink1)
                Spacer()
                if let path = training.datasetPath {
                    Text(path.lastPathComponent)
                        .font(Theme.mono(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                }
            }

            // Parsed user-facing milestones (no ultralytics torrent).
            VStack(alignment: .leading, spacing: 4) {
                if milestones.isEmpty {
                    Text("Click \u{201C}Train Now\u{201D} above to start. Progress will appear here.")
                        .font(Theme.ui(size: 12, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                } else {
                    ForEach(Array(milestones.enumerated()), id: \.offset) { _, m in
                        HStack(spacing: 8) {
                            Image(systemName: m.hasPrefix("FAILED") ? "xmark.octagon.fill" : "checkmark.circle.fill")
                                .foregroundStyle(m.hasPrefix("FAILED") ? Theme.warn : Theme.success)
                                .font(.system(size: 12))
                            Text(m)
                                .font(Theme.ui(size: 12, weight: .regular))
                                .foregroundStyle(Theme.ink2)
                        }
                    }
                }
            }

            DisclosureGroup("Show technical log") {
                ScrollView(.vertical) {
                    ScrollViewReader { proxy in
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(training.logTail.enumerated()), id: \.offset) { idx, line in
                                Text(line)
                                    .font(Theme.mono(size: 11, weight: .regular))
                                    .foregroundStyle(line.contains("FAILED") ? Theme.accent
                                                    : line.contains("OK") || line.contains("ok") ? Theme.success
                                                    : Theme.ink2)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 1)
                                    .id(idx)
                            }
                        }
                        .onChange(of: training.logTail.count) { _, _ in
                            if let last = training.logTail.indices.last {
                                withAnimation { proxy.scrollTo(last, anchor: .bottom) }
                            }
                        }
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 160, maxHeight: 240)
                .padding(.top, 6)
                .background(Theme.surface2)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
            }
            .font(Theme.ui(size: 12, weight: .medium))
            .tint(Theme.ink3)
            .foregroundStyle(Theme.ink3)
        }
        .padding(Theme.Spacing.l)
        .lbCard(Theme.surface, radius: Theme.Radius.r5)
    }
}
