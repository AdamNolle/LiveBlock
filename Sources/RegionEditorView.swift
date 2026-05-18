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

                // 3. Live preview of the rectangle being drawn from scratch.
                if case .creating(let start, let current) = active {
                    Rectangle()
                        .stroke(Color.accentColor, lineWidth: 2)
                        .background(Color.accentColor.opacity(0.18))
                        .frame(width: abs(current.x - start.x),
                               height: abs(current.y - start.y))
                        .position(x: (start.x + current.x) / 2,
                                  y: (start.y + current.y) / 2)
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
                .foregroundStyle(Theme.block)
            Text("Drag any rectangle on screen to block it")
                .font(Theme.display(size: 18, weight: .bold))
                .foregroundStyle(Color.primary)
            Text("Or use the size buttons above to drop a region of a fixed size. Press \u{2318}\u{21E9} to confirm and close, Esc to cancel.")
                .font(Theme.ui(size: 12))
                .foregroundStyle(Color.secondary)
                .multilineTextAlignment(.center)
            Button("Got it") { showHintCard = false }
                .buttonStyle(.glassProminent)
                .tint(Theme.block)
                .controlSize(.small)
                .padding(.top, 4)
        }
        .padding(20)
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    // MARK: - Canvas (create new region)

    @ViewBuilder
    private func canvasLayer(in size: CGSize) -> some View {
        Color.black.opacity(0.22)
            .contentShape(Rectangle())
            .gesture(createGesture(size: size))
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
                .fill(Color.red.opacity(isHovered ? 0.18 : 0.10))
                .overlay(
                    Rectangle().stroke(Color.red.opacity(0.9),
                                       style: StrokeStyle(lineWidth: 2, dash: [6]))
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
            .overlay(Circle().stroke(Color.red, lineWidth: 2))
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
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                LiveBlockerLogo(size: 32, cornerRadius: 8)
                Divider().frame(height: 28)

                // Drop a region of a fixed size at the center, no drag needed.
                Button {
                    dropRegion(width: 0.18, height: 0.10)
                } label: {
                    sizeLabel("S", caption: "Small")
                }
                .buttonStyle(.glass).controlSize(.small)
                .help("Drop a small region (~18\u{00D7}10%)")

                Button {
                    dropRegion(width: 0.32, height: 0.18)
                } label: {
                    sizeLabel("M", caption: "Medium")
                }
                .buttonStyle(.glass).controlSize(.small)
                .help("Drop a medium region (~32\u{00D7}18%)")

                Button {
                    dropRegion(width: 0.52, height: 0.32)
                } label: {
                    sizeLabel("L", caption: "Large")
                }
                .buttonStyle(.glass).controlSize(.small)
                .help("Drop a large region (~52\u{00D7}32%)")

                Button {
                    addFullScreenRegion()
                } label: {
                    sizeLabel("FS", caption: "Full screen")
                }
                .buttonStyle(.glass).controlSize(.small)
                .help("Block the whole screen")

                Divider().frame(height: 28)

                Button {
                    controller.closeEditor()
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "checkmark")
                        Text("Done")
                    }
                }
                .buttonStyle(.glassProminent).tint(Theme.success)
                .help("Save and close (\u{2318}\u{21A9} or Esc)")
                .keyboardShortcut(.return, modifiers: [.command])

                Button {
                    controller.closeEditor()
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.glass).controlSize(.small)
                .help("Close (Esc)")
            }
            .padding(10)
            .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.xl))
        }
        .frame(maxWidth: 720)
    }

    private func sizeLabel(_ id: String, caption: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: "plus")
                .font(.system(size: 9, weight: .bold))
            Text(id)
                .font(Theme.ui(size: 12, weight: .bold))
        }
        .help(caption)
    }
}
