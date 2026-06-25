import SwiftUI

/// LiveBlocker v4 design tokens — "humanized dark dashboard".
///
/// Ported from `design/project/tokens.css`. Calm dark surfaces with a cool-blue
/// undertone, a single restrained red-orange "kill" accent (#ff5039) used only
/// for live/block state, Geist (humanist sans) for UI + headings, Geist Mono
/// (tabular figures) for live numbers / hotkeys, 4–16px radii.
///
/// The app renders dark by default (the design's default theme); the
/// "scene stays dark" rule means capture-preview surfaces are always dark.
/// Apply `.preferredColorScheme(.dark)` at the window root and paint
/// backgrounds with `Theme.bg` / `Theme.surface`.
enum Theme {

    // MARK: - Surfaces
    static let bg        = Color(hex: 0x0C0D12)
    static let surface   = Color(hex: 0x14161E)
    static let surface2  = Color(hex: 0x1A1D27)
    static let surface3  = Color(hex: 0x232734)
    static let surface4  = Color(hex: 0x2C3142)
    static let overlay   = Color(hex: 0x0C0D12).opacity(0.78)

    // MARK: - Borders / hairlines
    static let line       = Color.white.opacity(0.055)
    static let line2      = Color.white.opacity(0.095)
    static let lineStrong = Color.white.opacity(0.16)

    // MARK: - Ink (text)
    static let ink1 = Color(hex: 0xF3F4F7)   // primary headings, numbers
    static let ink2 = Color(hex: 0xD3D6DF)   // body
    static let ink3 = Color(hex: 0x969AA7)   // secondary / captions
    static let ink4 = Color(hex: 0x6A6E7C)   // tertiary / muted
    static let ink5 = Color(hex: 0x44485A)   // faint

    // MARK: - Accent (the kill / block colour) + semantics
    static let accent      = Color(hex: 0xFF5039)
    static let accentHover = Color(hex: 0xFF6A55)
    static let accentSoft  = Color(hex: 0xFF5039).opacity(0.13)
    static let accentRing  = Color(hex: 0xFF5039).opacity(0.32)

    static let success     = Color(hex: 0x4ADE80)
    static let successSoft = Color(hex: 0x4ADE80).opacity(0.13)
    static let info        = Color(hex: 0x60A5FA)
    static let infoSoft    = Color(hex: 0x60A5FA).opacity(0.13)
    static let ml          = Color(hex: 0xA78BFA)   // purple = "smart"
    static let mlSoft      = Color(hex: 0xA78BFA).opacity(0.13)
    static let warn        = Color(hex: 0xFBBF24)
    static let warnSoft    = Color(hex: 0xFBBF24).opacity(0.13)

    // Back-compat aliases (existing views reference these; now mapped to v4).
    static let block     = accent
    static let blockSoft = accentSoft
    static let train     = info
    static let detect    = ml
    static var warning: Color { warn }

    // MARK: - Geometry (v4 radii: 4/6/8/12/16)
    enum Radius {
        static let r1: CGFloat = 4
        static let r2: CGFloat = 6
        static let r3: CGFloat = 8
        static let r4: CGFloat = 12
        static let r5: CGFloat = 16
        static let pill: CGFloat = 999
        // Back-compat aliases remapped to the tighter v4 scale.
        static let small: CGFloat = 8
        static let medium: CGFloat = 12
        static let large: CGFloat = 16
        static let xl: CGFloat = 16
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

    // MARK: - Fonts
    // Geist (humanist sans) for UI + headings; Geist Mono (tabular) for
    // numbers/hotkeys — the bundled stand-ins for Plus Jakarta Sans + JetBrains
    // Mono. Registered at launch by `Theme.registerFonts()`. Falls back to the
    // system font if registration fails.
    static func display(size: CGFloat, weight: Font.Weight = .bold) -> Font {
        Font.custom("Geist", size: size).weight(weight)
    }
    static func ui(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("Geist", size: size).weight(weight)
    }
    static func mono(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("Geist Mono", size: size).weight(weight)
    }

    /// Register the bundled variable fonts so `Font.custom` resolves regardless
    /// of `ATSApplicationFontsPath` quirks. Idempotent; safe to call at launch.
    static func registerFonts() {
        let names = ["Geist-Variable", "GeistMono-Variable", "BricolageGrotesque-Variable"]
        for name in names {
            guard let url = Bundle.main.url(forResource: name, withExtension: "ttf",
                                            subdirectory: "Fonts")
                ?? Bundle.main.url(forResource: name, withExtension: "ttf") else { continue }
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }
}

// MARK: - Card surface modifier

extension View {
    /// v4 `.card`: surface fill + 1px hairline + radius. Replaces Liquid Glass
    /// chrome with the design's flat dark surfaces.
    func lbCard(_ fill: Color = Theme.surface,
                radius: CGFloat = Theme.Radius.r4,
                stroke: Color = Theme.line) -> some View {
        self
            .background(fill)
            .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(stroke, lineWidth: 1)
            )
    }
}

// MARK: - LiveBlocker logo (unchanged — already matches the v4 design)

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
        block.foregroundColor = Theme.accent
        let er = AttributedString("er")
        return live + block + er
    }

    var body: some View {
        HStack(spacing: 10) {
            LiveBlockerLogo(size: size + 12, cornerRadius: (size + 12) * 0.22)
            Text(attributed)
                .font(Theme.display(size: size, weight: .bold))
                .foregroundStyle(Theme.ink1)
                .tracking(-0.4)
        }
    }
}

// MARK: - Status dot (v4 pulse + halo)

struct StatusDot: View {
    var color: Color = Theme.success
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
