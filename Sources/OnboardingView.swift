import SwiftUI
import AppKit
import ApplicationServices  // AXIsProcessTrusted
import CoreGraphics  // CGPreflightScreenCaptureAccess / CGRequestScreenCaptureAccess

struct OnboardingView: View {
    @State private var step: Int = 1
    let onFinish: () -> Void
    let onTryFirstBlock: () -> Void

    var body: some View {
        // v4 OnboardShell — header bar / content / footer, on Theme.bg.
        VStack(spacing: 0) {
            header
            Divider().overlay(Theme.line)

            Group {
                switch step {
                case 1: stepWelcome
                case 2: stepPermissions
                default: stepFirstBlock
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(EdgeInsets(top: 26, leading: 28, bottom: 26, trailing: 28))

            Divider().overlay(Theme.line)
            footer
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.bg)
        .preferredColorScheme(.dark)
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 10) {
            LiveBlockerLogo(size: 20, cornerRadius: 5)
            Text("LiveBlock")
                .font(Theme.ui(size: 13, weight: .semibold))
                .tracking(-0.18)
                .foregroundStyle(Theme.ink1)
            Spacer()
            Text("Step \(step) of 3")
                .font(Theme.mono(size: 11))
                .foregroundStyle(Theme.ink4)
        }
        .padding(.horizontal, 16)
        .frame(height: 44)
        .background(Theme.surface)
    }

    // MARK: - Step 1: Welcome

    private var stepWelcome: some View {
        VStack(alignment: .leading, spacing: 0) {
            LBPill(text: "Ready", tone: .success, size: .sm, dot: true)
                .padding(.bottom, 22)

            LiveBlockerLogo(size: 76, cornerRadius: 18)

            Text("\(Text("Take the ads\n").foregroundColor(Theme.ink1))\(Text("out of your screen.").foregroundColor(Theme.accent))")
            .font(Theme.display(size: 30, weight: .bold))
            .tracking(-0.75)
            .lineSpacing(2)
            .padding(.top, 24)

            Text("LiveBlock reads what's on your display, finds the bits you didn't ask for, and paints over them locally on your machine.")
                .font(Theme.ui(size: 14))
                .foregroundStyle(Theme.ink3)
                .lineSpacing(3)
                .frame(maxWidth: 380, alignment: .leading)
                .padding(.top, 16)

            Spacer(minLength: 16)

            // card-inset privacy reassurance
            HStack(spacing: 12) {
                iconTile(system: "lock.shield", bg: Theme.mlSoft, fg: Theme.ml, size: 32, radius: Theme.Radius.r2, icon: 16)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Nothing leaves this device")
                        .font(Theme.ui(size: 13, weight: .semibold))
                        .foregroundStyle(Theme.ink1)
                    Text("Pixel content stays local and is not sent as telemetry.")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Theme.ink3)
                }
                Spacer(minLength: 0)
            }
            .padding(14)
            .lbCard(Color.white.opacity(0.025), radius: Theme.Radius.r3, stroke: Theme.line)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: - Step 2: Permissions

    private var stepPermissions: some View {
        VStack(alignment: .leading, spacing: 0) {
            Caption("Step 2 — permissions")
            Text("Two required grants.")
                .font(Theme.ui(size: 24, weight: .semibold))
                .tracking(-0.48)
                .foregroundStyle(Theme.ink1)
                .padding(.top, 8)
                .padding(.bottom, 6)
            Text("You can revoke either grant anytime from System Settings.")
                .font(Theme.ui(size: 13))
                .foregroundStyle(Theme.ink3)
                .lineSpacing(2)

            VStack(spacing: 10) {
                permissionRow(kind: .screenCapture,
                              name: "Screen recording",
                              detail: "So LiveBlock can process display frames locally and render selected regions.",
                              system: "cpu", required: true)
                permissionRow(kind: .accessibility,
                              name: "Accessibility",
                              detail: "Enables LiveBlock's global keyboard shortcuts while another app has focus.",
                              system: "keyboard", required: true)
            }
            .padding(.top, 18)

            Spacer(minLength: 16)

            // dashed privacy footer card
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "lock")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(Theme.ml)
                Text("\(Text("Frames are processed locally with GPU or CPU fallback and discarded after processing. ").foregroundColor(Theme.ink3))\(Text("Nothing is saved.").foregroundColor(Theme.ink2))")
                .font(Theme.ui(size: 12))
                .lineSpacing(2)
                Spacer(minLength: 0)
            }
            .padding(12)
            .background(Theme.bg)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                    .strokeBorder(Theme.line2, style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
            )
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    enum PermissionKind {
        case screenCapture, accessibility

        var accessibilityIdentifier: String {
            switch self {
            case .screenCapture: "onboarding.allow-screen-recording"
            case .accessibility: "onboarding.allow-accessibility"
            }
        }
    }

    private func permissionRow(kind: PermissionKind,
                               name: String,
                               detail: String,
                               system: String,
                               required: Bool) -> some View {
        let granted: Bool = {
            switch kind {
            case .screenCapture: return Permissions.screenRecordingGranted()
            case .accessibility: return Permissions.accessibilityGranted()
            }
        }()
        let edge = granted ? Theme.success : Theme.accent
        return HStack(spacing: 14) {
            iconTile(system: system,
                     bg: granted ? Theme.successSoft : Theme.surface3,
                     fg: granted ? Theme.success : Theme.ink2,
                     size: 38, radius: Theme.Radius.r3, icon: 15)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    Text(name)
                        .font(Theme.ui(size: 14, weight: .semibold))
                        .foregroundStyle(Theme.ink1)
                    LBPill(text: required ? "Required" : "Optional", tone: .ghost, size: .sm)
                }
                Text(detail)
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Theme.ink3)
                    .lineSpacing(1)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            if granted {
                LBPill(text: "Granted", tone: .success, size: .sm, dot: true)
            } else {
                LBButton(title: "Allow", variant: .outline, size: .sm) {
                    switch kind {
                    case .screenCapture:
                        // Trigger the system permission prompt the first time
                        // (no-op if user already denied — they then have to
                        // toggle in System Settings).
                        Permissions.requestScreenRecording()
                        Permissions.openSystemSettings(.screenRecording)
                    case .accessibility:
                        if !Permissions.requestAccessibility() {
                            Permissions.openSystemSettings(.accessibility)
                        }
                    }
                }
                .accessibilityLabel("Allow \(name)")
                .accessibilityIdentifier(kind.accessibilityIdentifier)
            }
        }
        .padding(14)
        .background(Theme.surface2)
        .overlay(alignment: .leading) {
            Rectangle().fill(edge).frame(width: 3)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                .strokeBorder(Theme.line, lineWidth: 1)
        )
    }

    // MARK: - Step 3: Set & forget

    private var stepFirstBlock: some View {
        VStack(alignment: .leading, spacing: 0) {
            Caption("Step 3 — set & forget")
            Text("You won't be marking rectangles.")
                .font(Theme.ui(size: 24, weight: .semibold))
                .tracking(-0.48)
                .foregroundStyle(Theme.ink1)
                .padding(.top, 8)
                .padding(.bottom, 6)
            Text("After a couple of corrections, the detector takes over. Most days you'll never open this app.")
                .font(Theme.ui(size: 13))
                .foregroundStyle(Theme.ink3)
                .lineSpacing(2)

            // Faux capture preview with detection boxes
            capturePreview
                .frame(maxWidth: .infinity, minHeight: 150)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                        .strokeBorder(Theme.line, lineWidth: 1)
                )
                .padding(.top, 14)

            // Chord hint card
            HStack(spacing: 8) {
                Kbd("\u{2318}")
                Kbd("\u{21E7}")
                Kbd("B")
                Text("Press anytime to mark something manually. You rarely will.")
                    .font(Theme.ui(size: 12))
                    .foregroundStyle(Theme.ink3)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Theme.surface2)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                    .strokeBorder(Theme.line, lineWidth: 1)
            )
            .padding(.top, 14)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var capturePreview: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            ZStack(alignment: .topLeading) {
                LinearGradient(
                    colors: [Color(hex: 0x14111E), Color(hex: 0x221735)],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )

                // faint "content" scan lines
                ForEach(Array([14.0, 32, 22, 38, 26, 30, 18, 28].enumerated()), id: \.offset) { i, bw in
                    RoundedRectangle(cornerRadius: 1)
                        .fill(Color.white.opacity(0.18))
                        .frame(width: (bw / 100) * w, height: 3)
                        .position(x: 0.08 * w + (bw / 100) * w / 2,
                                  y: (0.10 + Double(i) * 0.09) * h + 1.5)
                }

                // primary detection box — Banner
                detectionBox(x: 0.58, y: 0.12, bw: 0.34, bh: 0.40,
                             label: "Banner · 94%", color: Theme.accent,
                             dashed: false, in: CGSize(width: w, height: h))
                // secondary detection box — Pop-up
                detectionBox(x: 0.14, y: 0.62, bw: 0.26, bh: 0.26,
                             label: "Pop-up · 71%", color: Theme.warn,
                             dashed: true, in: CGSize(width: w, height: h))

                Crosshair(size: 22, color: Theme.accent)
                    .position(x: 0.68 * w + 11, y: 0.24 * h + 11)
            }
        }
    }

    private func detectionBox(x: Double, y: Double, bw: Double, bh: Double,
                              label: String, color: Color, dashed: Bool,
                              in size: CGSize) -> some View {
        let rect = CGRect(x: x * size.width, y: y * size.height,
                          width: bw * size.width, height: bh * size.height)
        return ZStack(alignment: .topLeading) {
            RoundedRectangle(cornerRadius: 4)
                .fill(dashed ? Color.clear : color.opacity(0.13))
                .overlay(
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(color, style: StrokeStyle(lineWidth: 1.5,
                                                                dash: dashed ? [4, 3] : []))
                )
                .frame(width: rect.width, height: rect.height)
            Text(label)
                .font(Theme.ui(size: 10, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(color)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous))
                .fixedSize()
                .offset(y: -22)
        }
        .position(x: rect.midX, y: rect.midY)
    }

    // MARK: - Footer (dots + back / next)

    private var footer: some View {
        HStack(spacing: 10) {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { i in
                    let active = i == step - 1
                    let done = i < step - 1
                    Capsule(style: .continuous)
                        .fill(active || done ? Theme.accent : Theme.surface3)
                        .frame(width: active ? 24 : 6, height: 6)
                        .animation(Theme.snappy, value: step)
                }
            }
            Spacer()
            if step > 1 {
                LBButton(title: "Back", variant: .ghost) {
                    withAnimation(Theme.spring) { step -= 1 }
                }
            }
            if step == 3 {
                LBButton(title: "I'm in", variant: .primary) {
                    onTryFirstBlock()
                    onFinish()
                }
            } else {
                LBButton(title: step == 1 ? "Get started" : "Continue",
                         variant: .primary) {
                    withAnimation(Theme.spring) { step += 1 }
                }
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(Theme.surface)
    }

    // MARK: - Helpers

    private func iconTile(system: String, bg: Color, fg: Color,
                          size: CGFloat, radius: CGFloat, icon: CGFloat) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: radius, style: .continuous).fill(bg)
            Image(systemName: system)
                .font(.system(size: icon, weight: .medium))
                .foregroundStyle(fg)
        }
        .frame(width: size, height: size)
    }
}

// MARK: - Crosshair (detection cursor)

private struct Crosshair: View {
    var size: CGFloat = 22
    var color: Color = Theme.accent

    var body: some View {
        ZStack {
            Circle().strokeBorder(color, lineWidth: 1.5)
                .frame(width: size * 0.5, height: size * 0.5)
            Rectangle().fill(color).frame(width: 1.5, height: size)
            Rectangle().fill(color).frame(width: size, height: 1.5)
        }
        .frame(width: size, height: size)
    }
}
