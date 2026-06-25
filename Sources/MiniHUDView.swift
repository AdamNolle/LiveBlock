import SwiftUI

/// Mini HUD — the tiny always-on status pill. Mirrors `ScreenHUD`
/// (design/project/screens-flow.jsx): a single 36pt-tall capsule with a
/// translucent dark-blur fill, a pulsing "Blocking" StatusDot + label, a
/// run of Theme.mono live stats (regions · ms · today), and a ghost Pause
/// button — all separated by 1px hairline dividers.
///
/// Window/drag behaviour lives in `MiniHUDWindow` (movable-by-background,
/// floating panel); this view only paints the pill.
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

    private var isBlocking: Bool { controller.isRunning && !isPausedActive }

    private var statusWord: String {
        if !controller.isRunning { return "Idle" }
        return isPausedActive ? "Paused" : "Blocking"
    }

    private var statusColor: Color {
        if !controller.isRunning { return Theme.ink4 }
        return isPausedActive ? Theme.warn : Theme.accent
    }

    /// Per-frame processing time derived from the live capture rate.
    private var frameMS: String {
        let fps = controller.captureManager.framesPerSecond
        guard fps > 0 else { return "0.0" }
        return String(format: "%.1f", 1000 / fps)
    }

    // Pill fill: translucent dark blur (design rgba(15,16,22,0.92)).
    private var pillBackground: some View {
        ZStack {
            Capsule(style: .continuous).fill(.ultraThinMaterial)
            Capsule(style: .continuous).fill(Color(hex: 0x0F1016).opacity(0.92))
        }
    }

    var body: some View {
        HStack(spacing: 0) {
            // ── status ──────────────────────────────────────────────
            HStack(spacing: 8) {
                StatusDot(color: statusColor, size: 7, pulse: isBlocking)
                Text(statusWord)
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .tracking(-0.06)
                    .foregroundStyle(Theme.ink1)
            }
            .padding(.horizontal, 12)

            divider

            // ── live stats (mono) ───────────────────────────────────
            HStack(spacing: 8) {
                stat("\(controller.regionCount)", "regions", color: Theme.ink1)
                dot
                stat(frameMS, "ms", color: Theme.ml)
                dot
                stat("\(controller.screenshotCount)", "today", color: Theme.accent)
            }
            .padding(.horizontal, 12)

            divider

            // ── pause / resume ──────────────────────────────────────
            LBButton(
                title: isBlocking ? "Pause" : "Resume",
                variant: .ghost,
                size: .sm,
                systemIcon: isBlocking ? "pause.fill" : "play.fill"
            ) { controller.toggleCapture() }
            .help(isBlocking ? "Pause blocking" : "Resume blocking")
        }
        .frame(height: 36)
        .padding(.horizontal, 4)
        .background(pillBackground)
        .overlay(
            Capsule(style: .continuous).strokeBorder(Color.white.opacity(0.10), lineWidth: 1)
        )
        .clipShape(Capsule(style: .continuous))
        .shadow(color: .black.opacity(0.4), radius: 15, y: 10)
        .fixedSize()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .preferredColorScheme(.dark)
    }

    // 1px vertical hairline (design: width 1, line colour, 7pt vertical inset).
    private var divider: some View {
        Rectangle()
            .fill(Theme.line)
            .frame(width: 1)
            .padding(.vertical, 7)
    }

    // Centre dot separator between stats.
    private var dot: some View {
        Text("·").foregroundStyle(Theme.ink5)
    }

    private func stat(_ number: String, _ label: String, color: Color) -> some View {
        HStack(spacing: 4) {
            Text(number)
                .font(Theme.mono(size: 11))
                .foregroundStyle(color)
            Text(label)
                .font(Theme.ui(size: 11, weight: .regular))
                .foregroundStyle(Theme.ink3)
        }
    }
}
