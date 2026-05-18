import SwiftUI
import AppKit
import ApplicationServices  // AXIsProcessTrusted
import CoreGraphics  // CGPreflightScreenCaptureAccess / CGRequestScreenCaptureAccess

struct OnboardingView: View {
    @State private var step: Int = 1
    let onFinish: () -> Void
    let onTryFirstBlock: () -> Void

    var body: some View {
        ZStack {
            
            VStack(spacing: 0) {
                Group {
                    switch step {
                    case 1: stepWelcome
                    case 2: stepPermissions
                    default: stepFirstBlock
                    }
                }
                .padding(.horizontal, 36)
                .padding(.top, 28)

                Spacer(minLength: 0)
                footer
                    .padding(.horizontal, 24)
                    .padding(.vertical, 18)
            }
        }
    }

    // MARK: - Step 1: Welcome

    private var stepWelcome: some View {
        VStack(spacing: 20) {
            Spacer().frame(height: 18)
            LiveBlockerLogo(size: 120, cornerRadius: 28)
                .padding(.bottom, 8)
            Text("Block what your\nscreen shouldn't show.")
                .font(Theme.display(size: 32, weight: .bold))
                .foregroundStyle(Color.primary)
                .multilineTextAlignment(.center)
                .lineSpacing(2)
            Text("LiveBlock reads display frames with ScreenCaptureKit and replaces marked regions with edge-extrapolated fill — before they hit your eyes.")
                .font(Theme.ui(size: 13))
                .foregroundStyle(Color.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 360)
                .lineSpacing(2)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Step 2: Permissions

    private var stepPermissions: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Two quick permissions")
                .font(Theme.display(size: 24, weight: .bold))
                .foregroundStyle(Color.primary)
            Text("Both stay on your Mac. Nothing leaves the device.")
                .font(Theme.ui(size: 12))
                .foregroundStyle(Color.secondary)
                .padding(.bottom, 4)

            permissionRow(kind: .screenCapture,
                          name: "Screen Recording",
                          detail: "Required by ScreenCaptureKit",
                          system: "cpu", color: Theme.success)
            permissionRow(kind: .accessibility,
                          name: "Accessibility (optional)",
                          detail: "Lets blocks snap to UI elements",
                          system: "shield", color: Theme.detect)

            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "lock.shield")
                    .foregroundStyle(Theme.detect)
                Text("Frames are processed in a sandboxed Metal pipeline and discarded after compositing. No screenshots, no telemetry of pixel content, ever.")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Color.secondary)
            }
            .padding(12)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))
            .padding(.top, 8)

            Spacer()
        }
    }

    enum PermissionKind { case screenCapture, accessibility }

    private func permissionRow(kind: PermissionKind,
                               name: String,
                               detail: String,
                               system: String,
                               color: Color) -> some View {
        let granted: Bool = {
            switch kind {
            case .screenCapture: return Permissions.screenRecordingGranted()
            case .accessibility: return Permissions.accessibilityGranted()
            }
        }()
        return HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 10).fill(color.opacity(0.15))
                    .frame(width: 40, height: 40)
                Image(systemName: system).foregroundStyle(color)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(name).font(Theme.ui(size: 13, weight: .semibold))
                    .foregroundStyle(Color.primary)
                Text(detail).font(Theme.ui(size: 11))
                    .foregroundStyle(Color.secondary)
            }
            Spacer()
            if granted {
                Label("Granted", systemImage: "checkmark.seal.fill")
                    .font(Theme.ui(size: 11, weight: .semibold))
                    .foregroundStyle(Theme.success)
            } else {
                Button {
                    switch kind {
                    case .screenCapture:
                        // Trigger the system permission prompt the first time
                        // (no-op if user already denied — they then have to
                        // toggle in System Settings).
                        Permissions.requestScreenRecording()
                        Permissions.openSystemSettings(.screenRecording)
                    case .accessibility:
                        // Fires the Accessibility modal the first time;
                        // routes to Settings if user already saw it.
                        if !Permissions.requestAccessibility() {
                            Permissions.openSystemSettings(.accessibility)
                        }
                    }
                } label: {
                    Text("Allow")
                }
                .buttonStyle(.glass).controlSize(.small)
            }
        }
        .padding(14)
        .glassEffect(in: RoundedRectangle(cornerRadius: 16))
    }

    // MARK: - Step 3: First block

    private var stepFirstBlock: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Make your first block")
                .font(Theme.display(size: 24, weight: .bold))
                .foregroundStyle(Color.primary)
            Text("One click below: LiveBlock starts capture and opens the region editor. Drag a rectangle around anything to block it.")
                .font(Theme.ui(size: 12))
                .foregroundStyle(Color.secondary)
                .padding(.bottom, 4)

            ZStack {
                LinearGradient(colors: [
                    Color(red: 0.165, green: 0.122, blue: 0.267),
                    Color(red: 0.290, green: 0.180, blue: 0.431)
                ], startPoint: .topLeading, endPoint: .bottomTrailing)

                Rectangle()
                    .stroke(Theme.block, style: StrokeStyle(lineWidth: 1.5, dash: [5]))
                    .background(Theme.block.opacity(0.10))
                    .frame(width: 220, height: 110)
                    .overlay(
                        Text("Drag any rectangle")
                            .font(Theme.ui(size: 11, weight: .semibold))
                            .foregroundStyle(.white)
                    )
            }
            .frame(height: 200)
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 16))

            VStack(spacing: 10) {
                Button {
                    onTryFirstBlock()
                    onFinish()
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "rectangle.dashed")
                        Text("Start blocking & open the editor")
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                }
                .buttonStyle(.glassProminent).tint(Theme.block)
                .controlSize(.large)

                HStack(spacing: 6) {
                    Text("Tip: anytime, press").font(Theme.mono(size: 10)).foregroundStyle(Color.secondary)
                    ForEach(["\u{2318}", "\u{21E7}", "B"], id: \.self) { k in
                        Text(k)
                            .font(Theme.ui(size: 10, weight: .semibold))
                            .frame(width: 22, height: 22)
                            .glassEffect(in: RoundedRectangle(cornerRadius: 6))
                    }
                    Text("to open the editor again").font(Theme.mono(size: 10))
                        .foregroundStyle(Color.secondary)
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.top, 12)

            Spacer()
        }
    }

    // MARK: - Footer (dots + back/next)

    private var footer: some View {
        HStack(spacing: 12) {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { i in
                    Capsule()
                        .fill(i == step - 1 ? AnyShapeStyle(Theme.block) : AnyShapeStyle(Color.secondary.opacity(0.6)))
                        .frame(width: i == step - 1 ? 22 : 7, height: 7)
                        .animation(Theme.snappy, value: step)
                }
            }
            Spacer()
            if step > 1 {
                Button("Back") { withAnimation(Theme.spring) { step -= 1 } }
                    .buttonStyle(.borderless).controlSize(.small)
            }
            // Step 3 has its own primary "Start blocking & open the editor"
            // button on the page. The footer's secondary path here lets the
            // user defer that and explore later.
            if step == 3 {
                Button("Skip for now") { onFinish() }
                    .buttonStyle(.glass)
                    .controlSize(.small)
            } else {
                Button {
                    withAnimation(Theme.spring) { step += 1 }
                } label: {
                    HStack(spacing: 6) {
                        Text(step == 1 ? "Get started" : "Continue")
                        Image(systemName: "arrow.right")
                    }
                }
                .buttonStyle(.glassProminent).tint(Theme.block)
            }
        }
    }
}
