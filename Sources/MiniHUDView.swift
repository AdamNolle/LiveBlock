import SwiftUI

struct MiniHUDView: View {
    @ObservedObject var controller: AppController

    /// Single source of truth for status text — reads the same `pauseReason`
    /// the Control Panel banner uses, so both surfaces always agree.
    private var isPausedActive: Bool {
        if !controller.isRunning { return false }
        switch controller.pauseReason {
        case .none, .stopped: return false
        default: return true
        }
    }

    private var statusWord: String {
        if !controller.isRunning { return "IDLE" }
        return isPausedActive ? "PAUSED" : "BLOCKING"
    }

    private var statusColor: Color {
        if !controller.isRunning { return Color.secondary }
        return isPausedActive ? Theme.warn : Theme.block
    }

    private var secondaryLine: String {
        switch controller.pauseReason {
        case .fullscreenApp(let name):
            return "Paused — \(name) is fullscreen"
        case .excludedApp(let name):
            return "Excluded — \(name)"
        case .userPaused:
            return "Paused by you"
        case .permissionDenied:
            return "Screen Recording permission needed"
        case .startError:
            return "Couldn't start — check Control Panel"
        case .stopped, .none:
            return "\(controller.screenshotCount) captures saved"
        }
    }

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                LiveBlockerLogo(size: 42, cornerRadius: 11)
                Circle()
                    .fill(Theme.block)
                    .frame(width: 14, height: 14)
                    .overlay(Circle().strokeBorder(Color(.windowBackgroundColor), lineWidth: 2))
                    .offset(x: 17, y: -17)
                    .shadow(color: Theme.block.opacity(0.45), radius: 4)
            }

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(statusWord)
                        .font(Theme.ui(size: 12, weight: .semibold))
                        .tracking(1)
                        .foregroundStyle(statusColor)
                    Text("·")
                        .foregroundStyle(Color.secondary.opacity(0.6))
                    Text(String(format: "%.0f fps", controller.captureManager.framesPerSecond))
                        .font(Theme.mono(size: 11))
                        .foregroundStyle(Color.secondary)
                }
                Text("\(controller.regionCount) region\(controller.regionCount == 1 ? "" : "s") · \(controller.captureManager.currentPatches.count) live")
                    .font(Theme.display(size: 18, weight: .bold))
                    .foregroundStyle(Color.primary)
                Text(secondaryLine)
                    .font(Theme.ui(size: 10))
                    .foregroundStyle(Color.secondary)
            }

            Spacer(minLength: 8)

            // Inline action buttons — every feature reachable by mouse from
            // the always-visible HUD without opening the Control Panel.
            HStack(spacing: 6) {
                hudButton(
                    systemImage: "rectangle.dashed",
                    tint: Theme.block,
                    help: "Draw a region to block"
                ) { controller.toggleEditor() }

                hudButton(
                    systemImage: "camera",
                    tint: Theme.detect,
                    help: "Capture a screenshot for labeling"
                ) { controller.captureScreenshotForLabeling() }

                hudButton(
                    systemImage: controller.isRunning ? "pause.fill" : "play.fill",
                    tint: controller.isRunning ? Theme.warn : Theme.success,
                    help: controller.isRunning ? "Pause blocking" : "Resume blocking"
                ) { controller.toggleCapture() }

                hudButton(
                    systemImage: "macwindow",
                    tint: Color.secondary,
                    help: "Open Control Panel"
                ) { controller.showControlPanel() }
            }
        }
        .padding(14)
        .glassEffect(in: RoundedRectangle(cornerRadius: 24))
        .padding(8)
    }

    private func hudButton(
        systemImage: String,
        tint: Color,
        help: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 32, height: 32)
        }
        .buttonStyle(.glass)
        .help(help)
    }
}
