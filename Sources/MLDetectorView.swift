import SwiftUI

/// ML detector tuning — reskinned to the v4 "Smart detection" screen
/// (design/project/screens-app.jsx · ScreenDetector). A full-width status band
/// (big purple threshold number + "Auto-block on" pill), a full-width threshold
/// slider card, and a 2-column area: live preview scene + "What to catch"
/// categories. All controller wiring (minConfidence / setMinimumConfidence,
/// showTrainingDashboard, showLabelingWindow, live capture state) is preserved.
struct MLDetectorView: View {
    @ObservedObject var controller: AppController
    @Binding var minConfidence: Double

    @State private var detectorRules: [DetectorClassRule] = []

    private let fauxLines: [Double] = [0.14, 0.32, 0.22, 0.38, 0.26, 0.30, 0.18, 0.28, 0.34, 0.22]

    /// Wrap the incoming binding so the slider keeps pushing the threshold into
    /// the capture manager exactly like the old `onEditingChanged` call did.
    private var confBinding: Binding<Double> {
        Binding(
            get: { minConfidence },
            set: { newValue in
                minConfidence = newValue
                controller.captureManager.setMinimumConfidence(Float(newValue))
            }
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            statusBand
            thresholdCard
            HStack(alignment: .top, spacing: 16) {
                previewCard
                categoriesCard
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.bg)
        .preferredColorScheme(.dark)
        .onAppear { reloadDetectorRules() }
    }

    // MARK: - Status band (spans both columns)

    private var statusBand: some View {
        HStack(alignment: .bottom, spacing: 18) {
            VStack(alignment: .leading, spacing: 8) {
                Caption("On-device · open-vocabulary · blocks logos & ads, no training · 24 MB")

                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text("\(Int(minConfidence * 100))%")
                        .font(Theme.mono(size: 44, weight: .semibold))
                        .tracking(-1.5)
                        .foregroundStyle(Theme.ml)
                    Text("confidence threshold")
                        .font(Theme.ui(size: 16, weight: .medium))
                        .foregroundStyle(Theme.ink2)
                    LBPill(text: "Auto-block on", tone: .success, size: .sm, dot: true)
                }

                Text("The detector only auto-blocks when it's at least this sure. Raise it for fewer false positives, lower it to catch more.")
                    .font(Theme.ui(size: 13))
                    .foregroundStyle(Theme.ink3)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: 540, alignment: .leading)
            }

            Spacer(minLength: 8)

            VStack(alignment: .trailing, spacing: 6) {
                LBButton(title: controller.modelUpdateInProgress ? "Verifying…" : "Install signed update",
                         variant: .outline, size: .md, systemIcon: "checkmark.shield") {
                    controller.chooseAndInstallSignedModelUpdate()
                }
                .disabled(controller.modelUpdateInProgress)
                if let status = controller.modelUpdateStatus {
                    Text(status)
                        .font(Theme.ui(size: 10))
                        .foregroundStyle(Theme.ink3)
                        .lineLimit(2)
                        .frame(maxWidth: 240, alignment: .trailing)
                }
            }
            LBButton(title: "Train", variant: .outline, size: .md,
                     systemIcon: "sparkles") {
                controller.showTrainingDashboard()
            }
            LBButton(title: reviewTitle, variant: .primary, size: .md,
                     systemIcon: "arrow.right") {
                controller.showLabelingWindow()
            }
        }
    }

    private var reviewTitle: String {
        let n = controller.screenshotCount
        return n > 0 ? "Review \(n) borderline" : "Review queue"
    }

    // MARK: - Threshold slider card

    private var thresholdCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 14) {
                Text("Confidence threshold")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink2)
                Spacer()
                HStack(spacing: 4) {
                    Text("← more aggressive")
                        .foregroundStyle(Theme.accent)
                    Text("·").foregroundStyle(Theme.ink4)
                    Text("fewer mistakes →")
                        .foregroundStyle(Theme.ml)
                }
                .font(Theme.ui(size: 11, weight: .medium))
            }

            LBSlider(value: confBinding, range: 0.05...0.9, accent: Theme.ml)

            HStack {
                ForEach([5, 25, 45, 65, 90], id: \.self) { mark in
                    Text("\(mark)%")
                        .font(Theme.mono(size: 10))
                        .foregroundStyle(Theme.ink4)
                    if mark != 90 { Spacer() }
                }
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .lbCard()
    }

    // MARK: - Live preview

    private var previewCard: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                StatusDot(color: Theme.accent, size: 7, pulse: true)
                Text("Live preview")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Text("What the detector sees right now")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink4)
                Spacer()
                Text(previewStat)
                    .font(Theme.mono(size: 11))
                    .foregroundStyle(Theme.ink3)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .overlay(alignment: .bottom) {
                Rectangle().fill(Theme.line).frame(height: 1)
            }

            scene
                .frame(maxWidth: .infinity, minHeight: 280, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, minHeight: 320, alignment: .top)
        .lbCard()
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r4, style: .continuous))
    }

    private var previewStat: String {
        let found = controller.captureManager.lastDetectionLabels.count
        if controller.isRunning {
            let fps = controller.captureManager.framesPerSecond
            return "\(found) found · \(String(format: "%.0f", fps)) fps"
        }
        return "paused"
    }

    private var scene: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            ZStack(alignment: .topLeading) {
                LinearGradient(
                    colors: [Color(hex: 0x1C1530), Color(hex: 0x2A1F44)],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )

                // Faux content lines.
                ForEach(Array(fauxLines.enumerated()), id: \.offset) { i, lw in
                    RoundedRectangle(cornerRadius: 1, style: .continuous)
                        .fill(Color.white.opacity(0.12))
                        .frame(width: w * lw, height: 3)
                        .position(x: w * 0.08 + (w * lw) / 2,
                                  y: h * (0.08 + Double(i) * 0.08))
                }

                // Live model detections only (manual regions are intentionally excluded).
                let detections = controller.captureManager.currentDetections
                ForEach(detections.prefix(6)) { detection in
                    let r = detection.normalizedRect
                    detectionBox(label: "\(detection.label) (\(Int(detection.confidence * 100))%)",
                                 width: max(8, r.width * w),
                                 height: max(8, r.height * h),
                                 x: (r.minX + r.width / 2) * w,
                                 y: (r.minY + r.height / 2) * h)
                }

                if detections.isEmpty {
                    Text(controller.isRunning
                         ? "No detections in this frame — lower the threshold or move to a busier scene."
                         : "Start capture to see live detections.")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Theme.ink3)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 28)
                        .frame(width: w, height: h)
                }
            }
        }
    }

    private func detectionBox(label: String, width: CGFloat, height: CGFloat,
                              x: CGFloat, y: CGFloat) -> some View {
        RoundedRectangle(cornerRadius: 4, style: .continuous)
            .fill(Theme.accentSoft)
            .overlay(
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .strokeBorder(Theme.accent, lineWidth: 1.5)
            )
            .overlay(alignment: .topLeading) {
                Text(label)
                    .font(Theme.ui(size: 10, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Theme.accent)
                    .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous))
                    .fixedSize()
                    .offset(x: -1, y: -22)
            }
            .frame(width: width, height: height)
            .position(x: x, y: y)
    }

    // MARK: - Categories ("What to catch")

    private var categoriesCard: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("What to catch")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                LBPill(text: "\(detectorRules.count) classes", tone: .ghost, size: .sm)
                Spacer()
                LBPill(text: "open vocab", tone: .ml, size: .sm)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .overlay(alignment: .bottom) {
                Rectangle().fill(Theme.line).frame(height: 1)
            }

            if detectorRules.isEmpty {
                Text("Detector vocabulary unavailable")
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Theme.ink3)
                    .padding(14)
            } else {
                ForEach(Array(detectorRules.enumerated()), id: \.element.id) { index, rule in
                    categoryRow(rule: rule, first: index == 0)
                }
            }

            Spacer(minLength: 0)
        }
        .frame(width: 320)
        .frame(minHeight: 320, alignment: .top)
        .lbCard()
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r4, style: .continuous))
    }

    private func categoryRow(rule: DetectorClassRule, first: Bool) -> some View {
        let isOn = Binding(
            get: { detectorRules.first(where: { $0.id == rule.id })?.enabled ?? rule.enabled },
            set: { enabled in
                guard controller.captureManager.setDetectorClassEnabled(id: rule.id, enabled: enabled) else { return }
                reloadDetectorRules()
            }
        )
        return HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(isOn.wrappedValue ? Theme.accent : Theme.ink5)
                .frame(width: 3, height: 26)

            VStack(alignment: .leading, spacing: 2) {
                Text(rule.name)
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                Text("Class threshold \(Int(rule.threshold * 100))%")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink3)
            }

            Spacer(minLength: 8)

            LBToggle(isOn: isOn, size: .sm)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .overlay(alignment: .top) {
            if !first { Rectangle().fill(Theme.line).frame(height: 1) }
        }
    }

    private func reloadDetectorRules() {
        detectorRules = controller.captureManager.detectorRules()
    }
}
