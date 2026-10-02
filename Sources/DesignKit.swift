import SwiftUI

// LiveBlocker v4 — DesignKit.
//
// Faithful SwiftUI ports of the React/JSX primitives in
// `design/project/primitives.jsx`. Everything is built on `Theme` tokens
// (Sources/Theme.swift) so the dark dashboard stays consistent. These replace
// macOS Liquid Glass chrome and system controls throughout the app.
//
// Match the VISUAL output of the prototypes — not the JSX code structure.

// MARK: - Pill / chip

enum PillTone {
    case neutral, accent, success, info, ml, warn, ghost

    /// (background, foreground, border)
    var colors: (bg: Color, fg: Color, bd: Color) {
        switch self {
        case .neutral: return (Theme.surface3,  Theme.ink2,    Theme.line)
        case .accent:  return (Theme.accentSoft, Theme.accent,  .clear)
        case .success: return (Theme.successSoft, Theme.success, .clear)
        case .info:    return (Theme.infoSoft,   Theme.info,    .clear)
        case .ml:      return (Theme.mlSoft,     Theme.ml,      .clear)
        case .warn:    return (Theme.warnSoft,   Theme.warn,    .clear)
        case .ghost:   return (.clear,           Theme.ink3,    Theme.line2)
        }
    }
}

enum LBSize { case sm, md }

/// `Pill` — radius-pill chip. tone = *Soft bg + solid fg, weight 600.
/// Optional leading `StatusDot` (tinted to the foreground colour).
struct LBPill: View {
    var text: String
    var tone: PillTone = .neutral
    var size: LBSize = .md
    var dot: Bool = false
    var pulse: Bool = false

    var body: some View {
        let c = tone.colors
        let sm = size == .sm
        HStack(spacing: 6) {
            if dot {
                StatusDot(color: c.fg, size: 6, pulse: pulse)
            }
            Text(text)
                .font(Theme.ui(size: sm ? 11 : 12, weight: .semibold))
                .tracking(-0.06)
                .foregroundStyle(c.fg)
                .lineLimit(1)
        }
        .padding(.horizontal, sm ? 7 : 10)
        .padding(.vertical, sm ? 2 : 4)
        .background(c.bg)
        .clipShape(Capsule(style: .continuous))
        .overlay(Capsule(style: .continuous).strokeBorder(c.bd, lineWidth: 1))
    }
}

// MARK: - Button

enum LBButtonVariant { case primary, secondary, ghost, outline, accent }

enum LBButtonSize {
    case sm, md, lg

    var height: CGFloat { switch self { case .sm: 28; case .md: 34; case .lg: 40 } }
    var hPadding: CGFloat { switch self { case .sm: 11; case .md: 14; case .lg: 18 } }
    var fontSize: CGFloat { switch self { case .sm: 12; case .md: 13; case .lg: 14 } }
    var gap: CGFloat { switch self { case .sm: 6; case .md: 8; case .lg: 9 } }
    var iconSize: CGFloat { switch self { case .sm: 13; case .md: 14; case .lg: 15 } }
    var kbdSize: CGFloat { switch self { case .sm: 10; case .md: 11; case .lg: 12 } }
}

/// `Btn` — radius r3, weight 600. variants:
/// primary = accent bg / white · secondary = surface3 + line2 ·
/// ghost = clear · outline = 1px line2 · accent = accentSoft bg + accent fg.
struct LBButton: View {
    var title: String
    var variant: LBButtonVariant = .primary
    var size: LBButtonSize = .md
    var systemIcon: String? = nil
    var kbd: String? = nil
    var fullWidth: Bool = false
    var action: () -> Void = {}

    private var style: (bg: Color, fg: Color, bd: Color) {
        switch variant {
        case .primary:   return (Theme.accent,     .white,     Theme.accent)
        case .secondary: return (Theme.surface3,   Theme.ink1, Theme.line2)
        case .ghost:     return (.clear,           Theme.ink2, .clear)
        case .outline:   return (.clear,           Theme.ink1, Theme.line2)
        case .accent:    return (Theme.accentSoft, Theme.accent, .clear)
        }
    }

    var body: some View {
        let s = style
        Button(action: action) {
            HStack(spacing: size.gap) {
                if let icon = systemIcon {
                    Image(systemName: icon)
                        .font(.system(size: size.iconSize, weight: .semibold))
                }
                Text(title)
                    .font(Theme.ui(size: size.fontSize, weight: .semibold))
                    .tracking(-0.06)
                if let kbd {
                    Kbd(kbd, size: size.kbdSize,
                        bg: variant == .primary ? Color.white.opacity(0.18) : Color.white.opacity(0.06),
                        fg: variant == .primary ? Color.white.opacity(0.9) : Theme.ink3,
                        border: variant == .primary ? Color.white.opacity(0.14) : Theme.line)
                        .padding(.leading, 2)
                }
            }
            .foregroundStyle(s.fg)
            .frame(maxWidth: fullWidth ? .infinity : nil)
            .frame(height: size.height)
            .padding(.horizontal, size.hPadding)
            .background(s.bg)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                    .strokeBorder(s.bd, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Kbd (keycap)

/// `Kbd` — mono keycap, surface3 bg, r1, 1px line.
struct Kbd: View {
    var key: String
    var size: CGFloat = 11
    var bg: Color = Theme.surface3
    var fg: Color = Theme.ink2
    var border: Color = Theme.line

    init(_ key: String, size: CGFloat = 11,
         bg: Color = Theme.surface3, fg: Color = Theme.ink2, border: Color = Theme.line) {
        self.key = key
        self.size = size
        self.bg = bg
        self.fg = fg
        self.border = border
    }

    var body: some View {
        Text(key)
            .font(Theme.mono(size: size, weight: .medium))
            .foregroundStyle(fg)
            .frame(minWidth: size + 6, minHeight: size + 6)
            .padding(.horizontal, 5)
            .background(bg)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous)
                    .strokeBorder(border, lineWidth: 1)
            )
            .accessibilityHidden(true)
    }
}

// MARK: - Toggle

enum LBToggleSize {
    case sm, md, lg
    var width: CGFloat { switch self { case .sm: 30; case .md: 36; case .lg: 44 } }
    var height: CGFloat { switch self { case .sm: 18; case .md: 20; case .lg: 24 } }
    var knob: CGFloat { height - 4 }
}

/// `Toggle` — pill track (on = accent, off = surface3), white knob, 0.18s slide.
struct LBToggle: View {
    @Binding var isOn: Bool
    var accessibilityName: String
    var accessibilityIdentifier: String
    var size: LBToggleSize = .md

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Button {
            if reduceMotion {
                isOn.toggle()
            } else {
                withAnimation(.easeInOut(duration: 0.18)) { isOn.toggle() }
            }
        } label: {
            ZStack(alignment: isOn ? .trailing : .leading) {
                Capsule(style: .continuous)
                    .fill(isOn ? Theme.accent : Theme.surface3)
                    .overlay(
                        Capsule(style: .continuous)
                            .strokeBorder(isOn ? Color.clear : Theme.line2, lineWidth: 1)
                    )
                Circle()
                    .fill(.white)
                    .frame(width: size.knob, height: size.knob)
                    .shadow(color: .black.opacity(0.4), radius: 1.5, y: 1)
                    .padding(.horizontal, 2)
            }
            .frame(width: size.width, height: size.height)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(accessibilityName))
        .accessibilityValue(Text(isOn ? "On" : "Off"))
        .accessibilityIdentifier(accessibilityIdentifier)
    }
}

// MARK: - Slider

/// `Slider` — 2px track fill, white knob with 2px accent ring.
struct LBSlider: View {
    @Binding var value: Double
    var range: ClosedRange<Double> = 0...1
    var accent: Color = Theme.accent

    private let knobSize: CGFloat = 18

    private func pct(_ v: Double) -> Double {
        let span = range.upperBound - range.lowerBound
        guard span > 0 else { return 0 }
        return min(1, max(0, (v - range.lowerBound) / span))
    }

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let p = pct(value)
            let knobX = p * (w - knobSize) + knobSize / 2
            ZStack(alignment: .leading) {
                // track
                Capsule(style: .continuous)
                    .fill(Theme.surface3)
                    .frame(height: 2)
                // fill
                Capsule(style: .continuous)
                    .fill(accent)
                    .frame(width: max(0, knobX), height: 2)
                // knob
                Circle()
                    .fill(.white)
                    .frame(width: knobSize, height: knobSize)
                    .overlay(Circle().strokeBorder(accent, lineWidth: 2))
                    .shadow(color: .black.opacity(0.4), radius: 3, y: 2)
                    .position(x: knobX, y: geo.size.height / 2)
            }
            .frame(height: geo.size.height)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { g in
                        let usable = max(1, w - knobSize)
                        let raw = (g.location.x - knobSize / 2) / usable
                        let clamped = min(1, max(0, raw))
                        value = range.lowerBound + clamped * (range.upperBound - range.lowerBound)
                    }
            )
        }
        .frame(height: 22)
    }
}

// MARK: - Field / input

/// `Field` — surface2, 1px line, r3. Optional leading SF Symbol + trailing Kbd.
struct LBField: View {
    @Binding var text: String
    var placeholder: String = ""
    var systemIcon: String? = nil
    var trailingKbd: String? = nil

    var body: some View {
        HStack(spacing: 8) {
            if let icon = systemIcon {
                Image(systemName: icon)
                    .font(.system(size: 13, weight: .regular))
                    .foregroundStyle(Theme.ink3)
            }
            TextField("", text: $text, prompt: Text(placeholder).foregroundColor(Theme.ink4))
                .textFieldStyle(.plain)
                .font(Theme.ui(size: 13, weight: .regular))
                .foregroundStyle(Theme.ink1)
                .frame(maxWidth: .infinity)
            if let trailingKbd {
                Kbd(trailingKbd)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 34)
        .background(Theme.surface2)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                .strokeBorder(Theme.line, lineWidth: 1)
        )
    }
}

// MARK: - Segmented control

/// `Segmented` — surface3 track, active seg = surface4 + r2, weight 600.
struct LBSegmented: View {
    var options: [String]
    @Binding var selection: Int

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(options.enumerated()), id: \.offset) { idx, label in
                let active = idx == selection
                Button {
                    withAnimation(.easeOut(duration: 0.15)) { selection = idx }
                } label: {
                    Text(label)
                        .font(Theme.ui(size: 12, weight: .semibold))
                        .tracking(-0.06)
                        .foregroundStyle(active ? Theme.ink1 : Theme.ink3)
                        .frame(height: 26)
                        .padding(.horizontal, 12)
                        .background(
                            RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous)
                                .fill(active ? Theme.surface4 : .clear)
                        )
                }
                .buttonStyle(.plain)
            }
        }
        .padding(3)
        .frame(height: 32)
        .background(Theme.surface3)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Radius.r3, style: .continuous)
                .strokeBorder(Theme.line, lineWidth: 1)
        )
    }
}

// MARK: - Stat block

enum LBStatSize {
    case sm, md, lg, xl
    var num: CGFloat { switch self { case .sm: 26; case .md: 36; case .lg: 48; case .xl: 60 } }
    var unit: CGFloat { switch self { case .sm: 12; case .md: 14; case .lg: 16; case .xl: 18 } }
    var label: CGFloat { switch self { case .sm: 11; case .md: 12; case .lg: 13; case .xl: 13 } }
}

/// `Stat` — big mono number, tight tracking, ink1. Optional unit / label / sub.
struct LBStat: View {
    var value: String
    var unit: String? = nil
    var label: String? = nil
    var sub: String? = nil
    var size: LBStatSize = .md

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let label {
                Text(label)
                    .font(Theme.ui(size: size.label, weight: .medium))
                    .tracking(-0.06)
                    .foregroundStyle(Theme.ink3)
                    .padding(.bottom, 6)
            }
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(value)
                    .font(Theme.mono(size: size.num, weight: .semibold))
                    .tracking(-size.num * 0.04)
                    .foregroundStyle(Theme.ink1)
                if let unit {
                    Text(unit)
                        .font(Theme.mono(size: size.unit, weight: .regular))
                        .foregroundStyle(Theme.ink3)
                }
            }
            if let sub {
                Text(sub)
                    .font(Theme.ui(size: 12, weight: .regular))
                    .foregroundStyle(Theme.ink3)
                    .padding(.top, 6)
            }
        }
    }
}

// MARK: - Sparkline

/// `Sparkline` — line width 1.4, optional gradient fill (0.32→0) + last-point dot.
struct Sparkline: View {
    var points: [Double]
    var color: Color = Theme.accent
    var fill: Bool = false

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            let pts = mapped(in: CGSize(width: w, height: h))
            ZStack {
                if fill, pts.count > 1 {
                    fillPath(pts, height: h)
                        .fill(
                            LinearGradient(
                                colors: [color.opacity(0.32), color.opacity(0)],
                                startPoint: .top, endPoint: .bottom
                            )
                        )
                }
                linePath(pts)
                    .stroke(color, style: StrokeStyle(lineWidth: 1.4, lineCap: .round, lineJoin: .round))
                if let last = pts.last {
                    Circle()
                        .fill(color)
                        .frame(width: 6, height: 6)
                        .position(last)
                }
            }
        }
    }

    private func mapped(in size: CGSize) -> [CGPoint] {
        guard !points.isEmpty else { return [] }
        let maxV = points.max() ?? 0
        let minV = points.min() ?? 0
        let range = (maxV - minV) == 0 ? 1 : (maxV - minV)
        let n = points.count
        return points.enumerated().map { i, v in
            let x = n == 1 ? size.width / 2 : (CGFloat(i) / CGFloat(n - 1)) * size.width
            let y = size.height - (CGFloat((v - minV) / range)) * (size.height - 6) - 3
            return CGPoint(x: x, y: y)
        }
    }

    private func linePath(_ pts: [CGPoint]) -> Path {
        Path { p in
            guard let first = pts.first else { return }
            p.move(to: first)
            for pt in pts.dropFirst() { p.addLine(to: pt) }
        }
    }

    private func fillPath(_ pts: [CGPoint], height: CGFloat) -> Path {
        Path { p in
            guard let first = pts.first, let last = pts.last else { return }
            p.move(to: first)
            for pt in pts.dropFirst() { p.addLine(to: pt) }
            p.addLine(to: CGPoint(x: last.x, y: height))
            p.addLine(to: CGPoint(x: first.x, y: height))
            p.closeSubpath()
        }
    }
}

// MARK: - Progress bar

/// `Progress` — track surface3, fill color, height 6, fully rounded.
struct LBProgress: View {
    var value: Double // 0..1
    var color: Color = Theme.accent

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule(style: .continuous)
                    .fill(Theme.surface3)
                Capsule(style: .continuous)
                    .fill(color)
                    .frame(width: max(0, min(1, value)) * geo.size.width)
            }
        }
        .frame(height: 6)
    }
}

// MARK: - Section header

/// `SectionHeader` — 17/600 ink1 title + 13 ink3 subtitle.
struct LBSectionHeader: View {
    var title: String
    var subtitle: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(Theme.ui(size: 17, weight: .semibold))
                .tracking(-0.18)
                .foregroundStyle(Theme.ink1)
            if let subtitle {
                Text(subtitle)
                    .font(Theme.ui(size: 13, weight: .regular))
                    .foregroundStyle(Theme.ink3)
                    .lineSpacing(2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// MARK: - App badge / icon

/// `AppIcon` — colored rounded square (r2), centered white initial, weight 700.
struct AppBadge: View {
    var initial: String
    var color: Color
    var size: CGFloat = 22

    var body: some View {
        Text(initial)
            .font(Theme.ui(size: size * 0.5, weight: .bold))
            .tracking(-size * 0.02)
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color)
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
    }
}

// MARK: - Text helpers

/// `.caption` — 12/500 ink3.
struct Caption: View {
    var text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(Theme.ui(size: 12, weight: .medium))
            .tracking(-0.06)
            .foregroundStyle(Theme.ink3)
    }
}

/// `.eyebrow` — 11/600 uppercase, tracked, ink3.
struct Eyebrow: View {
    var text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text.uppercased())
            .font(Theme.ui(size: 11, weight: .semibold))
            .tracking(0.44)
            .foregroundStyle(Theme.ink3)
    }
}
