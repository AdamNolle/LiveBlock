import SwiftUI

struct TrainingDashboardView: View {
    @ObservedObject var controller: AppController
    @ObservedObject var training: TrainingController
    @ObservedObject var labeling: LabelingController

    @State private var epochsField: Int = 50
    @State private var batchField: Int = 8
    @State private var imgszField: Int = 640

    var body: some View {
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
                    .foregroundStyle(Theme.detect)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Set up training environment")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.primary)
                    Text("First-time only: installs Python + ultralytics + coremltools (\u{007E}1\u{2013}3 minutes).")
                        .font(.caption)
                        .foregroundStyle(Color.secondary)
                }
                Spacer(minLength: 0)
                Button {
                    training.installEnvironment()
                } label: {
                    HStack(spacing: 4) {
                        if training.isInstallingEnvironment {
                            ProgressView().controlSize(.small)
                        } else {
                            Image(systemName: "arrow.down.circle.fill")
                        }
                        Text(training.isInstallingEnvironment ? "Installing\u{2026}" : "Install now")
                    }
                }
                .buttonStyle(.glassProminent).tint(Theme.detect)
                .controlSize(.small)
                .disabled(training.isInstallingEnvironment)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Theme.detect.opacity(0.10))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Theme.detect.opacity(0.35), lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
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
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.primary)
                    Text(err.detail)
                        .font(.caption)
                        .foregroundStyle(Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let hint = err.hint {
                        Text(hint)
                            .font(.caption.monospaced())
                            .foregroundStyle(Color.primary.opacity(0.7))
                            .padding(.top, 2)
                    }
                }
                Spacer(minLength: 0)
                Button("Dismiss") {
                    training.clearTerminalState()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Theme.warn.opacity(0.12))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Theme.warn.opacity(0.35), lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
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
                accent: Theme.block,
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
                accent: Theme.detect,
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
                accent: Theme.train,
                action: nil
            )
        }
    }

    private func chev() -> some View {
        Image(systemName: "chevron.right")
            .font(.system(size: 14, weight: .semibold))
            .foregroundStyle(Color.secondary.opacity(0.5))
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
                    Circle().fill(done ? Theme.success : accent.opacity(0.2))
                        .frame(width: 22, height: 22)
                    if done {
                        Image(systemName: "checkmark")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(.white)
                    } else {
                        Text("\(index)")
                            .font(.system(size: 11, weight: .bold))
                            .foregroundStyle(accent)
                    }
                }
                Text(title)
                    .font(.headline)
                    .foregroundStyle(Color.primary)
                Spacer()
            }
            Text(detail)
                .font(.title3.weight(.semibold))
                .foregroundStyle(Color.primary)
            Text(hint)
                .font(.caption)
                .foregroundStyle(Color.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(Theme.Spacing.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.medium))

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
                    .fill(Theme.success.opacity(0.18))
                    .frame(width: 48, height: 48)
                Image(systemName: "brain.fill")
                    .font(.title)
                    .foregroundStyle(Theme.success)
                    .symbolEffect(.pulse, isActive: training.isBusy)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text("Training Dashboard").font(.title2).bold()
                Text("Build your own ad detector from your labeled screenshots.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if let last = training.lastSuccessAt {
                VStack(alignment: .trailing) {
                    Text("Last train").font(.caption).foregroundStyle(.secondary)
                    Text(last, style: .relative).font(.caption)
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
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Image(systemName: icon)
                    .foregroundStyle(Theme.block)
                Text(label).font(.caption).foregroundStyle(.secondary)
            }
            Text(value).font(.title2.weight(.semibold))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Theme.Spacing.m)
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.medium))
    }

    private var trainingCard: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.m) {
            HStack {
                Text("Training")
                    .font(.headline)
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
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    private func stepper(_ label: String, value: Binding<Int>, range: ClosedRange<Int>, step: Int) -> some View {
        HStack(spacing: 6) {
            Text(label).font(.caption).foregroundStyle(.secondary)
            Stepper(value: value, in: range, step: step) {
                Text("\(value.wrappedValue)").monospacedDigit()
            }
            .labelsHidden()
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        
    }

    private var trainNowDisabledReason: String? {
        if !training.venvInstalled { return "Install the training environment first." }
        if labeling.labeledCount < 20 { return "Label at least 20 screenshots first (currently \(labeling.labeledCount))." }
        return nil
    }

    private var actionRow: some View {
        HStack(spacing: Theme.Spacing.s) {
            if training.isBusy {
                Button {
                    training.cancel()
                } label: {
                    HStack { Image(systemName: "stop.fill"); Text("Cancel") }
                        .frame(maxWidth: .infinity).padding(.vertical, 4)
                }
                .buttonStyle(.glassProminent).tint(Theme.block)
            } else {
                Button {
                    training.startTraining(epochs: epochsField, imgsz: imgszField, batch: batchField)
                } label: {
                    HStack {
                        Image(systemName: "play.fill")
                        Text("Train Now").bold()
                    }
                    .frame(maxWidth: .infinity).padding(.vertical, 4)
                }
                .buttonStyle(.glassProminent).tint(Theme.train)
                .disabled(labeling.labeledCount < 20 || !training.venvInstalled)
                .help(trainNowDisabledReason ?? "Train a model on your labeled screenshots")
            }
            Button {
                controller.showLabelingWindow()
            } label: {
                HStack { Image(systemName: "rectangle.and.pencil.and.ellipsis"); Text("Label more") }
                    .frame(maxWidth: .infinity).padding(.vertical, 4)
            }
            .buttonStyle(.glass).controlSize(.small)
        }
    }

    @ViewBuilder
    private func progressRow(progress: TrainingController.TrainingProgress) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Epoch \(progress.epoch) / \(progress.totalEpochs)")
                    .font(.subheadline.weight(.medium))
                Spacer()
                if let map50 = progress.map50 {
                    Text(String(format: "mAP@50  %.3f", map50))
                        .font(.caption)
                        .foregroundStyle(Theme.success)
                }
                if let map = progress.map50_95 {
                    Text(String(format: "mAP@50-95  %.3f", map))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            ProgressView(value: training.progressFraction)
                .tint(Theme.block)
            HStack(spacing: 16) {
                Label(String(format: "box %.3f", progress.box_loss), systemImage: "square.dashed")
                    .font(.caption2).foregroundStyle(.secondary)
                Label(String(format: "cls %.3f", progress.cls_loss), systemImage: "tag")
                    .font(.caption2).foregroundStyle(.secondary)
            }
        }
    }

    private var stateLabel: some View {
        Group {
            switch training.state {
            case .idle:
                pill("Idle", color: .secondary, icon: "moon.zzz")
            case .exporting:
                pill("Exporting", color: Theme.detect, icon: "arrow.up.doc")
            case .training:
                pill("Training", color: Theme.block, icon: "brain")
            case .installing:
                pill("Installing", color: Theme.block, icon: "shippingbox")
            case .rebuilding:
                pill("Rebuilding", color: Theme.block, icon: "hammer")
            case .finished(let success, _):
                pill(success ? "Done" : "Failed",
                     color: success ? Theme.success : Theme.block,
                     icon: success ? "checkmark.seal" : "xmark.octagon")
            }
        }
    }

    private func pill(_ text: String, color: Color, icon: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: icon)
            Text(text)
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(color)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(Capsule().fill(color.opacity(0.15)))
        .overlay(Capsule().strokeBorder(color.opacity(0.4), lineWidth: 1))
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
                Text("Progress").font(.headline)
                Spacer()
                if let path = training.datasetPath {
                    Text(path.lastPathComponent)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                }
            }

            // Parsed user-facing milestones (no ultralytics torrent).
            VStack(alignment: .leading, spacing: 4) {
                if milestones.isEmpty {
                    Text("Click \u{201C}Train Now\u{201D} above to start. Progress will appear here.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(Array(milestones.enumerated()), id: \.offset) { _, m in
                        HStack(spacing: 8) {
                            Image(systemName: m.hasPrefix("FAILED") ? "xmark.octagon.fill" : "checkmark.circle.fill")
                                .foregroundStyle(m.hasPrefix("FAILED") ? Theme.warn : Theme.success)
                                .font(.system(size: 12))
                            Text(m)
                                .font(.caption)
                                .foregroundStyle(Color.primary)
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
                                    .font(.caption.monospaced())
                                    .foregroundStyle(line.contains("FAILED") ? Theme.block
                                                    : line.contains("OK") || line.contains("ok") ? Theme.success
                                                    : Color.primary.opacity(0.85))
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
            }
            .font(.caption.weight(.medium))
            .foregroundStyle(.secondary)
        }
        .padding(Theme.Spacing.l)
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }
}
