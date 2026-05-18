import SwiftUI

/// LiveBlocker brand layer.
///
/// Chrome (surfaces, buttons, toggles) is delegated to macOS 26 Liquid Glass —
/// `.glassEffect`, `.buttonStyle(.glass)`, `.buttonStyle(.glassProminent)`,
/// the system Toggle. This file holds only the brand-specific bits: accent
/// tints used on `.glassProminent` buttons, font helpers (with system
/// fallback), geometry tokens, and three brand views (logo, wordmark,
/// status dot).
enum Theme {

    // MARK: - Accent tints (macOS system blue palette)

    static let block       = Color(red: 0.039, green: 0.518, blue: 1.000)  // #0a84ff macOS system blue
    static let blockSoft   = Color(red: 0.847, green: 0.910, blue: 1.000)  // #d8e8ff
    static let train       = Color(red: 0.102, green: 0.310, blue: 0.549)  // #1a4f8c deep blue
    static let detect      = Color(red: 0.369, green: 0.361, blue: 0.902)  // #5e5ce6 cool indigo
    static let success     = Color(red: 0.169, green: 0.749, blue: 0.424)  // #2bbf6c (semantic — unchanged)
    static let warn        = Color(red: 0.941, green: 0.655, blue: 0.176)  // #f0a72d (semantic — unchanged)
    static var warning: Color { warn }  // legacy alias

    // MARK: - Geometry

    enum Radius {
        static let small: CGFloat = 10
        static let medium: CGFloat = 16
        static let large: CGFloat = 22
        static let xl: CGFloat = 28
        static let pill: CGFloat = 999
    }

    enum Spacing {
        static let xs: CGFloat = 4
        static let s: CGFloat = 8
        static let m: CGFloat = 12
        static let l: CGFloat = 16
        static let xl: CGFloat = 24
    }

    static let spring: Animation = .spring(response: 0.4, dampingFraction: 0.8)
    static let snappy: Animation = .spring(response: 0.24, dampingFraction: 0.85)

    // MARK: - Fonts (Bricolage / Geist if bundled, else system fallback)

    static func display(size: CGFloat, weight: Font.Weight = .bold) -> Font {
        Font.custom("Bricolage Grotesque", size: size).weight(weight)
    }
    static func ui(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("Geist", size: size).weight(weight)
    }
    static func mono(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("Geist Mono", size: size).weight(weight)
    }
}

// MARK: - LiveBlocker logo

/// 12×12 deterministic pixel grid + four black corner brackets + red
/// record dot. Pure SwiftUI Canvas — composes on top of any background.
struct LiveBlockerLogo: View {
    var size: CGFloat = 56
    var cornerRadius: CGFloat = 14

    private static let palette: [Color] = [
        Color(hex: 0xFF3B30), Color(hex: 0xFF9500), Color(hex: 0xFFCC00),
        Color(hex: 0x34C759), Color(hex: 0x00C7BE), Color(hex: 0x30B0C7),
        Color(hex: 0x0A84FF), Color(hex: 0x5E5CE6), Color(hex: 0xBF5AF2),
        Color(hex: 0xFF375F), Color(hex: 0xFF6482), Color(hex: 0xFFD60A),
        Color(hex: 0x32D74B), Color(hex: 0x64D2FF), Color(hex: 0x5AC8FA),
        Color(hex: 0xAF52DE), Color(hex: 0xFF453A), Color(hex: 0xFFD426),
        Color(hex: 0xA8E10C), Color(hex: 0xFF6B35), Color(hex: 0xEC407A),
    ]

    private static let cells: [Color] = {
        var seed: Int = 67
        func rnd() -> Double {
            seed = (seed &* 9301 &+ 49297) % 233280
            return Double(seed) / 233280.0
        }
        return (0..<144).map { _ in palette[Int(rnd() * Double(palette.count))] }
    }()

    var body: some View {
        let s = size
        let scale = s / 120.0
        Canvas { ctx, _ in
            for y in 0..<12 {
                for x in 0..<12 {
                    let r = CGRect(x: Double(x) * 10 * scale,
                                   y: Double(y) * 10 * scale,
                                   width: 10 * scale,
                                   height: 10 * scale)
                    ctx.fill(Path(r), with: .color(Self.cells[y * 12 + x]))
                }
            }
            let stroke: CGFloat = 7 * scale
            let brackets = Path { p in
                p.move(to: CGPoint(x: 16 * scale, y: 36 * scale))
                p.addLine(to: CGPoint(x: 16 * scale, y: 26 * scale))
                p.addQuadCurve(to: CGPoint(x: 26 * scale, y: 16 * scale),
                               control: CGPoint(x: 16 * scale, y: 16 * scale))
                p.addLine(to: CGPoint(x: 36 * scale, y: 16 * scale))
                p.move(to: CGPoint(x: 84 * scale, y: 16 * scale))
                p.addLine(to: CGPoint(x: 94 * scale, y: 16 * scale))
                p.addQuadCurve(to: CGPoint(x: 104 * scale, y: 26 * scale),
                               control: CGPoint(x: 104 * scale, y: 16 * scale))
                p.addLine(to: CGPoint(x: 104 * scale, y: 36 * scale))
                p.move(to: CGPoint(x: 104 * scale, y: 84 * scale))
                p.addLine(to: CGPoint(x: 104 * scale, y: 94 * scale))
                p.addQuadCurve(to: CGPoint(x: 94 * scale, y: 104 * scale),
                               control: CGPoint(x: 104 * scale, y: 104 * scale))
                p.addLine(to: CGPoint(x: 84 * scale, y: 104 * scale))
                p.move(to: CGPoint(x: 36 * scale, y: 104 * scale))
                p.addLine(to: CGPoint(x: 26 * scale, y: 104 * scale))
                p.addQuadCurve(to: CGPoint(x: 16 * scale, y: 94 * scale),
                               control: CGPoint(x: 16 * scale, y: 104 * scale))
                p.addLine(to: CGPoint(x: 16 * scale, y: 84 * scale))
            }
            ctx.stroke(brackets,
                       with: .color(Color(hex: 0x0C0A14)),
                       style: StrokeStyle(lineWidth: stroke, lineCap: .round, lineJoin: .round))
            let outer = Path(ellipseIn: CGRect(x: 75 * scale, y: 17 * scale,
                                                width: 28 * scale, height: 28 * scale))
            ctx.fill(outer, with: .color(Color(hex: 0x0C0A14)))
            let inner = Path(ellipseIn: CGRect(x: 78.5 * scale, y: 20.5 * scale,
                                                width: 21 * scale, height: 21 * scale))
            ctx.fill(inner, with: .color(Color(hex: 0xFF3B30)))
        }
        .frame(width: s, height: s)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .strokeBorder(Color.black.opacity(0.25), lineWidth: 1)
        )
    }
}

// MARK: - Wordmark

struct Wordmark: View {
    var size: CGFloat = 22

    private var attributed: AttributedString {
        let live = AttributedString("Live")
        var block = AttributedString("Block")
        block.foregroundColor = Theme.block
        let er = AttributedString("er")
        return live + block + er
    }

    var body: some View {
        HStack(spacing: 10) {
            LiveBlockerLogo(size: size + 12, cornerRadius: (size + 12) * 0.22)
            Text(attributed)
                .font(Theme.display(size: size, weight: .bold))
                .foregroundStyle(.primary)
                .tracking(-1)
        }
    }
}

// MARK: - Status dot

struct StatusDot: View {
    var color: Color = Theme.block
    var size: CGFloat = 8
    var pulse: Bool = false

    @State private var animating = false

    var body: some View {
        ZStack {
            Circle()
                .fill(color)
                .frame(width: size, height: size)
                .shadow(color: color.opacity(0.5), radius: size * 0.5)
            Circle()
                .fill(color.opacity(0.20))
                .frame(width: size * 2, height: size * 2)
            if pulse {
                Circle()
                    .stroke(color.opacity(0.4), lineWidth: 1)
                    .frame(width: size, height: size)
                    .scaleEffect(animating ? 2.6 : 1.0)
                    .opacity(animating ? 0 : 1)
            }
        }
        .onAppear {
            guard pulse else { return }
            withAnimation(.easeOut(duration: 1.5).repeatForever(autoreverses: false)) {
                animating = true
            }
        }
    }
}

// MARK: - Color hex helper

extension Color {
    init(hex: UInt32) {
        let r = Double((hex >> 16) & 0xFF) / 255.0
        let g = Double((hex >>  8) & 0xFF) / 255.0
        let b = Double( hex        & 0xFF) / 255.0
        self.init(.sRGB, red: r, green: g, blue: b, opacity: 1)
    }
}
