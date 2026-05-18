import SwiftUI

/// ML detector tuning. Threshold slider + live detection log.
struct MLDetectorView: View {
    @ObservedObject var controller: AppController
    @Binding var minConfidence: Double

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("ML detector")
                        .font(Theme.display(size: 26, weight: .bold))
                        .foregroundStyle(Color.primary)
                    Text("On-device YOLOv8n · 80 COCO classes · ~12 MB. Train your own ad detector on the Training Dashboard.")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Color.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
                Button("Train", systemImage: "brain.head.profile") {
                    controller.showTrainingDashboard()
                }
                .buttonStyle(.glassProminent).tint(Theme.train)
            }

            HStack(alignment: .top, spacing: 18) {
                preview
                detectionsPanel
            }
        }
    }

    // MARK: - Preview

    private var preview: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("PIPELINE")
                .font(Theme.ui(size: 11, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(Color.secondary)

            HStack(spacing: 16) {
                pipelineStat(value: controller.isRunning
                                ? String(format: "%.0f", controller.captureManager.framesPerSecond)
                                : "—",
                             label: "fps",
                             color: Theme.block)
                pipelineStat(value: "\(controller.captureManager.currentPatches.count)",
                             label: "live patches",
                             color: Theme.train)
                pipelineStat(value: "\(controller.captureManager.lastDetectionLabels.count)",
                             label: "detections",
                             color: Theme.detect)
            }
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))

            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Auto-block threshold")
                        .font(Theme.ui(size: 12, weight: .medium))
                        .foregroundStyle(Color.primary)
                    Spacer()
                    Text("\(Int(minConfidence * 100))%")
                        .font(Theme.mono(size: 12, weight: .semibold))
                        .foregroundStyle(Theme.block)
                }
                Slider(value: $minConfidence, in: 0.5...0.99) {
                    Text("Threshold")
                } onEditingChanged: { _ in
                    controller.captureManager.setMinimumConfidence(Float(minConfidence))
                }
                .tint(Theme.block)
                HStack {
                    Text("more aggressive")
                        .font(Theme.ui(size: 10))
                        .foregroundStyle(Color.secondary)
                    Spacer()
                    Text("fewer false positives")
                        .font(Theme.ui(size: 10))
                        .foregroundStyle(Color.secondary)
                }
            }
        }
        .padding(16)
        .glassEffect(in: RoundedRectangle(cornerRadius: 20))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func pipelineStat(value: String, label: String, color: Color) -> some View {
        VStack(spacing: 2) {
            Text(value)
                .font(Theme.mono(size: 22, weight: .bold))
                .foregroundStyle(color)
            Text(label.uppercased())
                .font(Theme.ui(size: 9, weight: .semibold))
                .tracking(0.5)
                .foregroundStyle(Color.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Live detections

    private var detectionsPanel: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("RECENT DETECTIONS")
                .font(Theme.ui(size: 11, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(Color.secondary)

            let labels = controller.captureManager.lastDetectionLabels
            if labels.isEmpty {
                Text(controller.isRunning
                     ? "No detections in the last frame. Lower the threshold or move to a busier scene."
                     : "Start capture to see live detections.")
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Color.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))
            } else {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(labels.prefix(8).enumerated()), id: \.offset) { _, label in
                        HStack(spacing: 10) {
                            RoundedRectangle(cornerRadius: 2).fill(Theme.detect)
                                .frame(width: 4, height: 18)
                            Text(label)
                                .font(Theme.mono(size: 12))
                                .foregroundStyle(Color.primary)
                                .lineLimit(1)
                            Spacer()
                        }
                        .padding(.horizontal, 12).padding(.vertical, 6)
                    }
                }
                .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))
            }

            // Review queue — opens the labeling window with the user's queued screenshots.
            HStack(spacing: 10) {
                Text("\(controller.screenshotCount)")
                    .font(Theme.mono(size: 22, weight: .bold))
                    .foregroundStyle(Theme.warn)
                Text(controller.screenshotCount == 1
                     ? "screenshot waiting to be labeled"
                     : "screenshots waiting to be labeled")
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Color.primary)
                Spacer()
                Button {
                    controller.showLabelingWindow()
                } label: {
                    HStack(spacing: 4) { Text("Label"); Image(systemName: "arrow.right") }
                }
                .buttonStyle(.borderless).controlSize(.small)
            }
            .padding(12)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))
            .padding(.top, 4)
        }
        .padding(16)
        .glassEffect(in: RoundedRectangle(cornerRadius: 20))
        .frame(width: 320, alignment: .topLeading)
    }
}
