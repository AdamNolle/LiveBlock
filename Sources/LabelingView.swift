import SwiftUI
import AppKit

struct LabelingView: View {
    @ObservedObject var labeling: LabelingController
    @ObservedObject var controller: AppController

    @State private var dragStart: CGPoint? = nil
    @State private var currentDragRect: CGRect? = nil
    @State private var hoveredBoxID: UUID? = nil

    var body: some View {
        ZStack {
            
            VStack(spacing: Theme.Spacing.m) {
                header
                if labeling.totalCount == 0 {
                    emptyState
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    canvas
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    footer
                }
            }
            .padding(Theme.Spacing.l)
        }
        .onAppear { labeling.refresh() }
    }

    // MARK: - Header (Wordmark + status + jump-to-unlabeled + capture button)

    private var header: some View {
        HStack(spacing: Theme.Spacing.m) {
            Wordmark(size: 16)
            VStack(alignment: .leading, spacing: 2) {
                Text("Labeling").font(Theme.ui(size: 13, weight: .semibold))
                    .foregroundStyle(Color.primary)
                if labeling.totalCount > 0 {
                    HStack(spacing: 4) {
                        Text("\(labeling.labeledCount)")
                            .foregroundStyle(Theme.success)
                            .font(Theme.mono(size: 11, weight: .semibold))
                        Text("/ \(labeling.totalCount) labeled  ·  \(labeling.totalCount - labeling.labeledCount) remaining")
                            .font(Theme.ui(size: 11))
                            .foregroundStyle(Color.secondary)
                    }
                } else {
                    Text("No screenshots yet — capture some first")
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Color.secondary)
                }
            }
            Spacer()

            if !labeling.currentSuggestions.isEmpty {
                Button {
                    labeling.acceptAllSuggestions()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "checkmark.seal.fill")
                        Text("Accept all (\(labeling.currentSuggestions.count))")
                    }
                }
                .buttonStyle(.glassProminent).controlSize(.small).tint(Theme.block)
                .help("Promote all yellow proposals to confirmed boxes")
            }

            // Once the user has enough labeled examples, surface a one-click
            // shortcut into the Training Dashboard so they don't have to dig.
            if labeling.labeledCount >= 20 {
                Button {
                    controller.showTrainingDashboard()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "brain.head.profile")
                        Text("Train now")
                    }
                }
                .buttonStyle(.glassProminent).controlSize(.small).tint(Theme.train)
                .help("You have enough labeled examples to train a useful model")
            }

            Button {
                labeling.goToFirstUnlabeled()
            } label: {
                HStack(spacing: 4) { Image(systemName: "forward.end.alt.fill"); Text("Next unlabeled") }
            }
            .buttonStyle(.glass).controlSize(.small)
            .disabled(labeling.totalCount == 0)

            Button {
                controller.captureScreenshotForLabeling()
            } label: {
                HStack(spacing: 4) { Image(systemName: "camera"); Text("Capture") }
            }
            .buttonStyle(.glassProminent).controlSize(.small).tint(Theme.block)
            .keyboardShortcut("s", modifiers: [.command, .shift])
            .disabled(!controller.isRunning)
            .help(controller.isRunning ? "⌘⇧S — saves the current frame for labeling"
                                       : "Start LiveBlock capturing first")

            Button {
                labeling.refresh()
            } label: {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.glass).controlSize(.small)
            .keyboardShortcut("r", modifiers: [.command])
        }
        .padding(.horizontal, Theme.Spacing.l)
        .padding(.vertical, Theme.Spacing.m)
        .background(.thinMaterial)
        .overlay(
            Rectangle()
                .fill(Color.black.opacity(0.07))
                .frame(height: 1),
            alignment: .bottom
        )
    }

    // MARK: - Empty state

    private var emptyState: some View {
        VStack(spacing: 14) {
            Image(systemName: "photo.stack")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No screenshots yet")
                .font(.title3)
            Text("Start LiveBlock capturing, then press ⌘⇧S whenever you see an ad on screen.\nThey'll show up here for labeling.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)

            Button {
                controller.captureScreenshotForLabeling()
            } label: {
                Label("Capture one now", systemImage: "camera")
            }
            .keyboardShortcut("s", modifiers: [.command, .shift])
            .disabled(!controller.isRunning)

            if !controller.isRunning {
                Text("LiveBlock isn't capturing — start it from the Control Panel first.")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }

            Button {
                labeling.refresh()
            } label: {
                Label("Refresh folder", systemImage: "arrow.clockwise")
            }
        }
        .padding(40)
    }

    // MARK: - Canvas

    private var canvas: some View {
        GeometryReader { geo in
            ZStack {
                Color.black
                if let img = labeling.currentImage {
                    Image(nsImage: img)
                        .resizable()
                        .interpolation(.medium)
                        .scaledToFit()
                        .background(
                            GeometryReader { imgGeo in
                                let layout = imageLayout(viewSize: geo.size)
                                Color.clear
                                    .onAppear { _ = imgGeo }
                                    .preference(key: LayoutPreferenceKey.self, value: layout)
                            }
                        )
                }

                // Overlay the boxes + drag preview, sized to the visible image rect.
                let layout = imageLayout(viewSize: geo.size)
                ZStack(alignment: .topLeading) {
                    // Yellow proposal boxes — click to accept
                    ForEach(labeling.currentSuggestions) { sug in
                        let rect = sug.rect(in: layout.size)
                        ZStack(alignment: .topTrailing) {
                            Rectangle()
                                .stroke(Theme.warning, style: StrokeStyle(lineWidth: 2, dash: [4]))
                                .background(Theme.warning.opacity(0.12))
                                .contentShape(Rectangle())
                            HStack(spacing: 4) {
                                Button {
                                    labeling.acceptSuggestion(id: sug.id)
                                } label: {
                                    Image(systemName: "checkmark.circle.fill")
                                        .imageScale(.large)
                                        .foregroundStyle(.white)
                                        .background(Circle().fill(Theme.success))
                                }
                                .buttonStyle(.plain)
                                Button {
                                    labeling.dismissSuggestion(id: sug.id)
                                } label: {
                                    Image(systemName: "xmark.circle.fill")
                                        .imageScale(.large)
                                        .foregroundStyle(.white)
                                        .background(Circle().fill(Color.black.opacity(0.7)))
                                }
                                .buttonStyle(.plain)
                            }
                            .padding(2)
                        }
                        .frame(width: rect.width, height: rect.height)
                        .position(x: rect.midX + layout.origin.x,
                                  y: rect.midY + layout.origin.y)
                        .gesture(DragGesture(minimumDistance: 0).onChanged { _ in }.onEnded { _ in })
                    }

                    // Existing boxes
                    ForEach(labeling.currentBoxes) { box in
                        let rect = box.rect(in: layout.size)
                        let isHovered = hoveredBoxID == box.id
                        ZStack(alignment: .topTrailing) {
                            Rectangle()
                                .stroke(Color.green.opacity(isHovered ? 1 : 0.85),
                                        style: StrokeStyle(lineWidth: 2))
                                .background(Color.green.opacity(isHovered ? 0.18 : 0.08))
                                .contentShape(Rectangle())
                            Button {
                                labeling.removeBox(id: box.id)
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                                    .imageScale(.large)
                                    .foregroundStyle(.white)
                                    .background(Circle().fill(Color.black.opacity(0.7)))
                            }
                            .buttonStyle(.plain)
                            .padding(2)
                        }
                        .frame(width: rect.width, height: rect.height)
                        .position(x: rect.midX + layout.origin.x,
                                  y: rect.midY + layout.origin.y)
                        .onHover { hovering in
                            hoveredBoxID = hovering ? box.id : (hoveredBoxID == box.id ? nil : hoveredBoxID)
                        }
                        // Consume drags on the box itself so we don't accidentally
                        // create an overlapping new box when starting a drag from inside.
                        .gesture(DragGesture(minimumDistance: 0).onChanged { _ in }.onEnded { _ in })
                    }
                    // In-progress drag rectangle
                    if let r = currentDragRect {
                        Rectangle()
                            .stroke(Color.accentColor, lineWidth: 2)
                            .background(Color.accentColor.opacity(0.18))
                            .frame(width: r.width, height: r.height)
                            .position(x: r.midX, y: r.midY)
                            .allowsHitTesting(false)
                    }
                }
                .frame(width: geo.size.width, height: geo.size.height, alignment: .topLeading)
                .contentShape(Rectangle())
                .gesture(drawGesture(layout: layout))

                // Hidden keyboard buttons
                Group {
                    Button("", action: { labeling.removeLastBox() })
                        .keyboardShortcut(.delete, modifiers: [])
                    Button("", action: { labeling.goNext() })
                        .keyboardShortcut(.rightArrow, modifiers: [])
                    Button("", action: { labeling.goPrev() })
                        .keyboardShortcut(.leftArrow, modifiers: [])
                    Button("", action: { labeling.markCurrentAsNoAds(); labeling.goNext(saveCurrent: false) })
                        .keyboardShortcut("n", modifiers: [])
                    Button("", action: { _ = labeling.saveLabels() })
                        .keyboardShortcut("s", modifiers: [.command])
                    Button("", action: { labeling.discardCurrent() })
                        .keyboardShortcut(.delete, modifiers: [.command])
                }
                .opacity(0)
                .frame(width: 0, height: 0)
                .allowsHitTesting(false)
            }
        }
    }

    // MARK: - Footer (action buttons + shortcut legend)

    private var footer: some View {
        HStack(spacing: Theme.Spacing.s) {
            Button {
                labeling.goPrev()
            } label: {
                HStack(spacing: 4) { Image(systemName: "chevron.left"); Text("Prev") }
            }
            .buttonStyle(.glass).controlSize(.small)
            .keyboardShortcut(.leftArrow, modifiers: [])

            Button {
                labeling.goNext()
            } label: {
                HStack(spacing: 4) { Text("Save & Next"); Image(systemName: "chevron.right") }
            }
            .buttonStyle(.glassProminent).tint(Theme.block)
            .keyboardShortcut(.rightArrow, modifiers: [])

            Button {
                labeling.markCurrentAsNoAds()
                labeling.goNext(saveCurrent: false)
            } label: {
                HStack(spacing: 4) { Image(systemName: "checkmark.circle"); Text("No ads (N)") }
            }
            .buttonStyle(.glassProminent).controlSize(.small).tint(Theme.train)
            .keyboardShortcut("n", modifiers: [])

            Button {
                labeling.discardCurrent()
            } label: {
                HStack(spacing: 4) { Image(systemName: "trash"); Text("Discard") }
            }
            .buttonStyle(.borderless).controlSize(.small)

            Spacer()

            VStack(alignment: .trailing, spacing: 2) {
                HStack(spacing: 4) {
                    StatusDot(color: labeling.currentIsLabeled ? Theme.success : Theme.warn)
                    Text("\(labeling.currentBoxes.count) box\(labeling.currentBoxes.count == 1 ? "" : "es")  ·  \(labeling.currentIsLabeled ? "saved" : "unsaved")")
                        .font(Theme.ui(size: 11, weight: .medium))
                        .foregroundStyle(labeling.currentIsLabeled ? Color.secondary : Theme.warn)
                }
                Text("Drag to draw  ·  ⌫ delete last  ·  → next  ·  ← prev  ·  ⌘S save  ·  ⌘⌫ discard")
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Color.secondary.opacity(0.6))
            }
        }
        .padding(.horizontal, Theme.Spacing.l)
        .padding(.vertical, Theme.Spacing.m)
        .glassEffect(in: RoundedRectangle(cornerRadius: Theme.Radius.large))
    }

    // MARK: - Image layout helper (where the scaled-to-fit image actually lands)

    private struct ImageLayout: Equatable {
        var origin: CGPoint
        var size: CGSize
    }

    private func imageLayout(viewSize: CGSize) -> ImageLayout {
        let imgSize = labeling.currentImageSize
        guard imgSize.width > 0, imgSize.height > 0,
              viewSize.width > 0, viewSize.height > 0 else {
            return ImageLayout(origin: .zero, size: .zero)
        }
        let imgAspect = imgSize.width / imgSize.height
        let viewAspect = viewSize.width / viewSize.height
        var w: CGFloat, h: CGFloat
        if imgAspect > viewAspect {
            // letterbox top/bottom
            w = viewSize.width
            h = w / imgAspect
        } else {
            h = viewSize.height
            w = h * imgAspect
        }
        let x = (viewSize.width - w) / 2
        let y = (viewSize.height - h) / 2
        return ImageLayout(origin: CGPoint(x: x, y: y),
                           size: CGSize(width: w, height: h))
    }

    private struct LayoutPreferenceKey: PreferenceKey {
        static var defaultValue = ImageLayout(origin: .zero, size: .zero)
        static func reduce(value: inout ImageLayout, nextValue: () -> ImageLayout) {
            value = nextValue()
        }
    }

    // MARK: - Drag-to-draw gesture (clamped to the visible image rect)

    private func drawGesture(layout: ImageLayout) -> some Gesture {
        DragGesture(minimumDistance: 4)
            .onChanged { value in
                guard layout.size.width > 0, layout.size.height > 0 else { return }
                let start = clamp(point: value.startLocation, layout: layout)
                let cur = clamp(point: value.location, layout: layout)
                if dragStart == nil { dragStart = start }
                currentDragRect = CGRect(
                    x: min(start.x, cur.x), y: min(start.y, cur.y),
                    width: abs(cur.x - start.x),
                    height: abs(cur.y - start.y)
                )
            }
            .onEnded { _ in
                defer {
                    dragStart = nil
                    currentDragRect = nil
                }
                guard let r = currentDragRect,
                      r.width > 12, r.height > 12,
                      layout.size.width > 0, layout.size.height > 0 else { return }
                // Convert from view coords → image-local → normalized [0..1]
                let localX = (r.minX - layout.origin.x) / layout.size.width
                let localY = (r.minY - layout.origin.y) / layout.size.height
                let localW = r.width / layout.size.width
                let localH = r.height / layout.size.height
                let box = LabelBox(x: localX, y: localY, width: localW, height: localH)
                labeling.addBox(box)
            }
    }

    private func clamp(point: CGPoint, layout: ImageLayout) -> CGPoint {
        CGPoint(x: max(layout.origin.x, min(layout.origin.x + layout.size.width, point.x)),
                y: max(layout.origin.y, min(layout.origin.y + layout.size.height, point.y)))
    }
}
