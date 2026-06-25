import SwiftUI
import AppKit

/// On-demand full-screen overlay for adding, moving, and resizing regions.
struct RegionEditorView: View {
    @ObservedObject var controller: AppController

    @State private var active: ActiveDrag = .none
    @State private var hoveredRegionID: UUID? = nil
    @State private var revision: Int = 0

    private enum ActiveDrag: Equatable {
        case none
        case creating(start: CGPoint, current: CGPoint)
        case moving(regionID: UUID, originalRect: CGRect, translation: CGSize)
        case resizing(regionID: UUID, originalRect: CGRect, handle: ResizeHandle, translation: CGSize)

        var draggedRegionID: UUID? {
            switch self {
            case .moving(let id, _, _), .resizing(let id, _, _, _): return id
            default: return nil
            }
        }
    }

    private enum ResizeHandle {
        case topLeft, topRight, bottomLeft, bottomRight
        case top, bottom, left, right
    }

    private let handleSize: CGFloat = 14
    private let minRegionPoints: CGFloat = 16

    @State private var showHintCard: Bool = true

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .top) {
                // 1. Canvas — captures clicks for drawing new regions.
                canvasLayer(in: geo.size)

                // 2. Regions — body drag + handles.
                ForEach(controller.regionStore.current) { region in
                    regionView(region: region, canvasSize: geo.size)
                }

                // 3. Live preview of the rectangle being drawn from scratch —
                //    the design's "marquee": spotlight dim + accent rect +
                //    corner handles + mono "W × H" dimension label.
                if case .creating(let start, let current) = active {
                    marqueePreview(start: start, current: current)
                        .allowsHitTesting(false)
                }

                // 4. Top toolbar with size buttons + Done / Quit.
                toolbar
                    .padding(.top, 12)
                    .frame(maxWidth: .infinity, alignment: .top)

                // 5. Centered "Drag to mark" hint card — shown only while
                //    no regions exist, dismissable by drawing or clicking ×.
                if showHintCard && controller.regionCount == 0 && active == .none {
                    hintCard
                        .frame(maxWidth: 460)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                        .allowsHitTesting(true)
                }

                // 6. Hidden keyboard shortcuts (Esc only).
                Button("", action: { controller.closeEditor() })
                    .keyboardShortcut(.escape, modifiers: [])
                    .opacity(0)
                    .frame(width: 0, height: 0)
                    .allowsHitTesting(false)
            }
            .id(revision)
        }
        .ignoresSafeArea()
        .preferredColorScheme(.dark)
        .onAppear {
            // Auto-dismiss the hint card after 8 s so it doesn't linger.
            showHintCard = (controller.regionCount == 0)
            if showHintCard {
                DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                    showHintCard = false
                }
            }
        }
    }

    private var hintCard: some View {
        VStack(spacing: 8) {
            Image(systemName: "rectangle.dashed")
                .font(.system(size: 36, weight: .semibold))
                .foregroundStyle(Theme.accent)
            Text("Drag any rectangle on screen to block it")
                .font(Theme.display(size: 18, weight: .bold))
                .foregroundStyle(Theme.ink1)
            Text("Or use the size buttons above to drop a region of a fixed size. Press \u{2318}\u{21E9} to confirm and close, Esc to cancel.")
                .font(Theme.ui(size: 12))
                .foregroundStyle(Theme.ink3)
                .multilineTextAlignment(.center)
            LBButton(title: "Got it", variant: .accent, size: .sm) {
                showHintCard = false
            }
            .padding(.top, 4)
        }
        .padding(20)
        .lbCard(Theme.surface, radius: Theme.Radius.r5, stroke: Theme.line2)
        .shadow(color: .black.opacity(0.5), radius: 28, y: 18)
    }

    // MARK: - Canvas (create new region)

    @ViewBuilder
    private func canvasLayer(in size: CGSize) -> some View {
        // Translucent scrim — keeps the screen behind visible while marking,
        // tinted with the design's deep base (#08090E).
        Color(hex: 0x08090E).opacity(0.28)
            .contentShape(Rectangle())
            .gesture(createGesture(size: size))
    }

    // MARK: - Marquee preview (drag-to-create) — design ScreenFloatingClassic

    @ViewBuilder
    private func marqueePreview(start: CGPoint, current: CGPoint) -> some View {
        let rect = CGRect(x: min(start.x, current.x),
                          y: min(start.y, current.y),
                          width: abs(current.x - start.x),
                          height: abs(current.y - start.y))
        ZStack {
            // Spotlight dim — everything outside the marquee darkens (the design's
            // `box-shadow: 0 0 0 9999px rgba(8,9,14,0.55)` cutout).
            Color(hex: 0x08090E).opacity(0.5)
                .mask {
                    Rectangle()
                        .overlay(
                            Rectangle()
                                .frame(width: rect.width, height: rect.height)
                                .position(x: rect.midX, y: rect.midY)
                                .blendMode(.destinationOut)
                        )
                        .compositingGroup()
                }

            // Marquee rect — 2px accent border + accentSoft fill.
            RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous)
                .fill(Theme.accentSoft)
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous)
                        .stroke(Theme.accent, lineWidth: 2)
                )
                .frame(width: rect.width, height: rect.height)
                .position(x: rect.midX, y: rect.midY)

            // Corner handles — white fill, accent border.
            ForEach(Array(marqueeCorners(rect).enumerated()), id: \.offset) { _, pt in
                RoundedRectangle(cornerRadius: 2, style: .continuous)
                    .fill(.white)
                    .overlay(
                        RoundedRectangle(cornerRadius: 2, style: .continuous)
                            .stroke(Theme.accent, lineWidth: 2)
                    )
                    .frame(width: 10, height: 10)
                    .position(x: pt.x, y: pt.y)
            }

            // Live dimension label — mono "W × H", accent chip above the rect.
            Text("\(Int(rect.width)) × \(Int(rect.height))")
                .font(Theme.mono(size: 11, weight: .semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(Theme.accent,
                            in: RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
                .position(x: rect.midX, y: max(12, rect.minY - 16))
        }
    }

    private func marqueeCorners(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
         CGPoint(x: r.minX, y: r.maxY), CGPoint(x: r.maxX, y: r.maxY)]
    }

    private func createGesture(size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 5)
            .onChanged { value in
                // Only initiate a create-drag if nothing else owns the drag.
                switch active {
                case .none, .creating:
                    active = .creating(start: value.startLocation, current: value.location)
                default:
                    break
                }
            }
            .onEnded { value in
                guard case .creating(let start, _) = active else { return }
                defer { active = .none }
                let rect = CGRect(
                    x: min(start.x, value.location.x),
                    y: min(start.y, value.location.y),
                    width: abs(value.location.x - start.x),
                    height: abs(value.location.y - start.y)
                )
                guard rect.width >= minRegionPoints,
                      rect.height >= minRegionPoints,
                      size.width > 0, size.height > 0 else { return }

                let region = NormalizedRegion(
                    x: Double(rect.minX / size.width),
                    y: Double(rect.minY / size.height),
                    width: Double(rect.width / size.width),
                    height: Double(rect.height / size.height)
                )
                controller.addRegion(region)
                revision &+= 1
            }
    }

    // MARK: - Region view (body + handles + delete)

    @ViewBuilder
    private func regionView(region: NormalizedRegion, canvasSize: CGSize) -> some View {
        let rect = displayRect(for: region, canvasSize: canvasSize)
        let isHovered = hoveredRegionID == region.id
        let isActive = active.draggedRegionID == region.id
        let showHandles = isHovered || isActive

        ZStack(alignment: .topTrailing) {
            // Body — fills the region. Hover for handles. Drag to move.
            Rectangle()
                .fill(Theme.accent.opacity(isHovered ? 0.20 : 0.12))
                .overlay(
                    Rectangle().stroke(Theme.accent, lineWidth: 2)
                )
                .frame(width: rect.width, height: rect.height)
                .position(x: rect.midX, y: rect.midY)
                .gesture(moveGesture(region: region, canvasSize: canvasSize))
                .onHover { hovering in
                    if hovering { hoveredRegionID = region.id }
                    else if hoveredRegionID == region.id { hoveredRegionID = nil }
                }

            // 8 resize handles (only when hovered or actively dragging this region)
            if showHandles {
                ForEach(allHandles, id: \.self) { handle in
                    handleView(for: handle, regionRect: rect, region: region, canvasSize: canvasSize)
                }
            }

            // Delete button
            Button {
                controller.deleteRegion(id: region.id)
                revision &+= 1
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .imageScale(.large)
                    .foregroundStyle(.white)
                    .background(Circle().fill(Color.black.opacity(0.75)))
            }
            .buttonStyle(.plain)
            .position(x: rect.maxX - 12, y: rect.minY + 12)
            .opacity(showHandles ? 1 : 0.5)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var allHandles: [ResizeHandle] {
        [.topLeft, .top, .topRight, .right, .bottomRight, .bottom, .bottomLeft, .left]
    }

    @ViewBuilder
    private func handleView(for handle: ResizeHandle,
                            regionRect: CGRect,
                            region: NormalizedRegion,
                            canvasSize: CGSize) -> some View {
        let pt = handlePosition(handle, in: regionRect)
        Circle()
            .fill(Color.white)
            .overlay(Circle().stroke(Theme.accent, lineWidth: 2))
            .frame(width: handleSize, height: handleSize)
            .position(x: pt.x, y: pt.y)
            .gesture(resizeGesture(region: region, handle: handle, canvasSize: canvasSize))
            .onHover { _ in
                NSCursor.crosshair.set()
            }
    }

    private func handlePosition(_ handle: ResizeHandle, in r: CGRect) -> CGPoint {
        switch handle {
        case .topLeft:     return CGPoint(x: r.minX, y: r.minY)
        case .top:         return CGPoint(x: r.midX, y: r.minY)
        case .topRight:    return CGPoint(x: r.maxX, y: r.minY)
        case .right:       return CGPoint(x: r.maxX, y: r.midY)
        case .bottomRight: return CGPoint(x: r.maxX, y: r.maxY)
        case .bottom:      return CGPoint(x: r.midX, y: r.maxY)
        case .bottomLeft:  return CGPoint(x: r.minX, y: r.maxY)
        case .left:        return CGPoint(x: r.minX, y: r.midY)
        }
    }

    // MARK: - Gestures

    private func moveGesture(region: NormalizedRegion, canvasSize: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 2)
            .onChanged { value in
                let original = region.rect(in: canvasSize)
                active = .moving(regionID: region.id,
                                 originalRect: original,
                                 translation: value.translation)
            }
            .onEnded { value in
                let original = region.rect(in: canvasSize)
                let proposed = original.offsetBy(dx: value.translation.width,
                                                 dy: value.translation.height)
                commit(rect: clampToCanvas(proposed, canvasSize: canvasSize),
                       regionID: region.id,
                       canvasSize: canvasSize)
            }
    }

    private func resizeGesture(region: NormalizedRegion,
                               handle: ResizeHandle,
                               canvasSize: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                let original = region.rect(in: canvasSize)
                active = .resizing(regionID: region.id,
                                   originalRect: original,
                                   handle: handle,
                                   translation: value.translation)
            }
            .onEnded { value in
                let original = region.rect(in: canvasSize)
                let resized = applyResize(original: original,
                                          handle: handle,
                                          translation: value.translation)
                let clamped = clampToCanvas(resized, canvasSize: canvasSize)
                commit(rect: clamped, regionID: region.id, canvasSize: canvasSize)
            }
    }

    private func commit(rect: CGRect, regionID: UUID, canvasSize: CGSize) {
        defer {
            active = .none
            revision &+= 1
        }
        guard rect.width >= minRegionPoints,
              rect.height >= minRegionPoints,
              canvasSize.width > 0, canvasSize.height > 0 else { return }

        let normalized = NormalizedRegion(
            id: regionID,
            x: Double(rect.minX / canvasSize.width),
            y: Double(rect.minY / canvasSize.height),
            width: Double(rect.width / canvasSize.width),
            height: Double(rect.height / canvasSize.height)
        )
        controller.regionStore.replace(id: regionID, with: normalized)
        controller.refreshRegionCount()
    }

    // MARK: - Geometry helpers

    /// Display rect, accounting for any in-progress drag.
    private func displayRect(for region: NormalizedRegion, canvasSize: CGSize) -> CGRect {
        let stored = region.rect(in: canvasSize)
        switch active {
        case .moving(let id, let original, let translation) where id == region.id:
            return clampToCanvas(original.offsetBy(dx: translation.width,
                                                   dy: translation.height),
                                  canvasSize: canvasSize)
        case .resizing(let id, let original, let handle, let translation) where id == region.id:
            let resized = applyResize(original: original, handle: handle, translation: translation)
            return clampToCanvas(resized, canvasSize: canvasSize)
        default:
            return stored
        }
    }

    private func applyResize(original: CGRect,
                             handle: ResizeHandle,
                             translation: CGSize) -> CGRect {
        var minX = original.minX
        var minY = original.minY
        var maxX = original.maxX
        var maxY = original.maxY

        switch handle {
        case .topLeft:     minX += translation.width; minY += translation.height
        case .top:         minY += translation.height
        case .topRight:    maxX += translation.width; minY += translation.height
        case .right:       maxX += translation.width
        case .bottomRight: maxX += translation.width; maxY += translation.height
        case .bottom:      maxY += translation.height
        case .bottomLeft:  minX += translation.width; maxY += translation.height
        case .left:        minX += translation.width
        }

        // Enforce a minimum size by pushing the moving edge back if it crosses.
        if maxX - minX < minRegionPoints {
            switch handle {
            case .topLeft, .left, .bottomLeft: minX = maxX - minRegionPoints
            default: maxX = minX + minRegionPoints
            }
        }
        if maxY - minY < minRegionPoints {
            switch handle {
            case .topLeft, .top, .topRight: minY = maxY - minRegionPoints
            default: maxY = minY + minRegionPoints
            }
        }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    private func clampToCanvas(_ r: CGRect, canvasSize: CGSize) -> CGRect {
        var x = max(0, r.minX)
        var y = max(0, r.minY)
        var w = r.width
        var h = r.height
        if x + w > canvasSize.width { w = canvasSize.width - x }
        if y + h > canvasSize.height { h = canvasSize.height - y }
        if x + w > canvasSize.width { x = canvasSize.width - w }
        if y + h > canvasSize.height { y = canvasSize.height - h }
        return CGRect(x: x, y: y, width: max(minRegionPoints, w), height: max(minRegionPoints, h))
    }

    // MARK: - Toolbar — port of the design's "floating marking tool"

    /// Drop a normalized region of the given relative size at screen center.
    private func dropRegion(width: Double, height: Double) {
        let w = min(max(width, 0.02), 1.0)
        let h = min(max(height, 0.02), 1.0)
        let x = max(0, (1.0 - w) / 2.0)
        let y = max(0, (1.0 - h) / 2.0)
        let region = NormalizedRegion(x: x, y: y, width: w, height: h)
        controller.addRegion(region)
        revision &+= 1
        showHintCard = false
    }

    /// Drops a region covering the full screen, then closes the editor.
    private func addFullScreenRegion() {
        let region = NormalizedRegion(x: 0, y: 0, width: 1, height: 1)
        controller.addRegion(region)
        revision &+= 1
        controller.closeEditor()
    }

    private var toolbar: some View {
        HStack(spacing: 8) {
            LiveBlockerLogo(size: 26, cornerRadius: 7)

            tbDivider

            // Drop a region of a fixed size at the center, no drag needed.
            LBButton(title: "S", variant: .ghost, size: .sm, systemIcon: "plus") {
                dropRegion(width: 0.18, height: 0.10)
            }
            .help("Drop a small region (~18\u{00D7}10%)")

            LBButton(title: "M", variant: .ghost, size: .sm, systemIcon: "plus") {
                dropRegion(width: 0.32, height: 0.18)
            }
            .help("Drop a medium region (~32\u{00D7}18%)")

            LBButton(title: "L", variant: .ghost, size: .sm, systemIcon: "plus") {
                dropRegion(width: 0.52, height: 0.32)
            }
            .help("Drop a large region (~52\u{00D7}32%)")

            LBButton(title: "FS", variant: .ghost, size: .sm, systemIcon: "plus") {
                addFullScreenRegion()
            }
            .help("Block the whole screen")

            tbDivider

            // Nudge hint — the design's arrow keycaps (decorative hotkey cue).
            HStack(spacing: 5) {
                Caption("Nudge")
                Kbd("\u{2190}", size: 10)
                Kbd("\u{2192}", size: 10)
                Kbd("\u{2191}", size: 10)
                Kbd("\u{2193}", size: 10)
            }

            tbDivider

            // Confirm — save + close. Accent action with ↵ keycap.
            LBButton(title: "Confirm", variant: .accent, size: .sm,
                     systemIcon: "checkmark", kbd: "\u{21A9}") {
                controller.closeEditor()
            }
            .help("Save and close (\u{2318}\u{21A9} or Esc)")
            .keyboardShortcut(.return, modifiers: [.command])

            // Cancel — close. Ghost action with esc keycap.
            LBButton(title: "Cancel", variant: .ghost, size: .sm,
                     systemIcon: "xmark", kbd: "esc") {
                controller.closeEditor()
            }
            .help("Close (Esc)")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(
            ZStack {
                Capsule(style: .continuous).fill(.ultraThinMaterial)
                Capsule(style: .continuous).fill(Color(hex: 0x0E0F15).opacity(0.72))
            }
        )
        .overlay(
            Capsule(style: .continuous).strokeBorder(Theme.line2, lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.55), radius: 24, y: 16)
        .frame(maxWidth: 720)
    }

    private var tbDivider: some View {
        Rectangle()
            .fill(Theme.line)
            .frame(width: 1, height: 22)
    }
}
