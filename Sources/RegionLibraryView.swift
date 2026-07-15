import SwiftUI

/// Region library — v4 port of `screens-app.jsx::ScreenLibrary`.
/// A data table of every saved region in the store with a stat summary,
/// search + source filter chrome, and per-row toggle / delete wiring.
///
/// Hosted inside `SettingsView` (which paints `Theme.bg` + `.preferredColorScheme(.dark)`
/// and supplies the page title bar), so this view renders only the in-content
/// header row + table card on the dark surface.
struct RegionLibraryView: View {
    @ObservedObject var controller: AppController
    @State private var search: String = ""
    @State private var filter: Int = 0   // 0 = All · 1 = Manual · 2 = Learned

    // Grid column widths — shared by the head row and every data row so the
    // columns stay aligned (SwiftUI has no CSS-grid, so we frame each cell).
    private let wPreview: CGFloat = 40
    private let wApp: CGFloat = 190
    private let wSource: CGFloat = 96
    private let wSize: CGFloat = 80
    private let wHits: CGFloat = 70
    private let wOn: CGFloat = 50
    private let wMenu: CGFloat = 30

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            header
            tableCard
        }
        .frame(maxWidth: .infinity, alignment: .top)
    }

    // MARK: - Header (Stat + search + filter + New)

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 14) {
            LBStat(value: "\(controller.regionCount)",
                   label: "Saved regions",
                   sub: statSub,
                   size: .lg)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 10) {
                LBField(text: $search,
                        placeholder: "Find a region\u{2026}",
                        systemIcon: "magnifyingglass",
                        trailingKbd: "/")
                    .frame(width: 240)
                LBSegmented(options: ["All", "Manual", "Learned"], selection: $filter)
                LBButton(title: "New", variant: .primary, size: .md, systemIcon: "plus") {
                    controller.toggleEditor()
                }
                .help("Open the region editor and draw a new rectangle")
            }
        }
    }

    private var statSub: String {
        let active = activeCount
        let blocks = totalBlocks
        return "\(active) active \u{00B7} \(blocks) blocks today"
    }

    // MARK: - Table card

    private var tableCard: some View {
        VStack(spacing: 0) {
            headRow
            let regions = filteredRegions
            if regions.isEmpty {
                emptyState
            } else {
                ForEach(Array(regions.enumerated()), id: \.element.id) { idx, r in
                    regionRow(r, index: idx, displayName: regionName(r))
                }
            }
            footRow
        }
        .lbCard(Theme.surface)
    }

    private var headRow: some View {
        HStack(spacing: 12) {
            Color.clear.frame(width: wPreview)
            Text("Region").frame(maxWidth: .infinity, alignment: .leading)
            Text("App \u{00B7} context").frame(width: wApp, alignment: .leading)
            Text("Source").frame(width: wSource, alignment: .leading)
            Text("Size").frame(width: wSize, alignment: .trailing)
            Text("Blocks \u{00B7} 7d").frame(width: wHits, alignment: .trailing)
            Text("On").frame(width: wOn, alignment: .leading)
            Color.clear.frame(width: wMenu)
        }
        .font(Theme.ui(size: 11, weight: .semibold))
        .tracking(-0.04)
        .foregroundStyle(Theme.ink3)
        .padding(.horizontal, 16)
        .frame(height: 38)
        .background(Theme.surface2)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Theme.line).frame(height: 1)
        }
    }

    private var footRow: some View {
        HStack(spacing: 10) {
            let shown = filteredRegions.count
            (Text("Showing ")
             + Text("\(shown)").font(Theme.mono(size: 12, weight: .medium)).foregroundColor(Theme.ink1)
             + Text(" of ")
             + Text("\(controller.regionCount)").font(Theme.mono(size: 12, weight: .medium)).foregroundColor(Theme.ink1))
                .font(Theme.ui(size: 12))
                .foregroundStyle(Theme.ink3)
            Spacer(minLength: 0)
            LBButton(title: "Export\u{2026}", variant: .ghost, size: .sm,
                     systemIcon: "square.and.arrow.up") {}
        }
        .padding(.horizontal, 16)
        .frame(height: 44)
        .overlay(alignment: .top) {
            Rectangle().fill(Theme.line).frame(height: 1)
        }
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "rectangle.dashed")
                .font(.system(size: 30))
                .foregroundStyle(Theme.ink4)
            Text(search.isEmpty ? "No regions yet" : "No matches")
                .font(Theme.ui(size: 13, weight: .semibold))
                .foregroundStyle(Theme.ink2)
            Text(search.isEmpty
                 ? "Click \u{201C}New\u{201D} above to draw your first region."
                 : "Try a different search or filter.")
                .font(Theme.ui(size: 11))
                .foregroundStyle(Theme.ink4)
            if search.isEmpty {
                LBButton(title: "New", variant: .primary, size: .sm, systemIcon: "plus") {
                    controller.toggleEditor()
                }
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 38)
    }

    // MARK: - Row

    private func regionRow(_ r: NormalizedRegion, index: Int, displayName: String) -> some View {
        let isOn = controller.regionEnabled(id: r.id)
        let hits = controller.captureManager.blocksByRegion[r.id, default: 0]
        let sizeStr = String(format: "%.0f%%\u{00D7}%.0f%%", r.width * 100, r.height * 100)
        return HStack(spacing: 12) {
            // preview — dashed proportional thumbnail of the normalized rect.
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous)
                    .fill(Theme.bg)
                    .overlay(
                        RoundedRectangle(cornerRadius: Theme.Radius.r1, style: .continuous)
                            .strokeBorder(Theme.line, lineWidth: 1)
                    )
                    .frame(width: 36, height: 22)
                RoundedRectangle(cornerRadius: 1)
                    .stroke(isOn ? Theme.accent : Theme.ink4,
                            style: StrokeStyle(lineWidth: 1, dash: [3]))
                    .frame(width: max(4, 36 * r.width), height: max(3, 22 * r.height))
                    .offset(x: 36 * r.x, y: 22 * r.y)
            }
            .frame(width: wPreview, height: 22, alignment: .leading)

            // name + size sub
            VStack(alignment: .leading, spacing: 1) {
                Text(displayName)
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .tracking(-0.07)
                    .foregroundStyle(Theme.ink1)
                    .lineLimit(1)
                Text("region \u{00B7} \(sizeStr)")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Theme.ink4)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            // app · context — these regions are global screen captures.
            HStack(spacing: 8) {
                AppBadge(initial: badgeInitial(displayName),
                         color: isOn ? Theme.accent : Theme.ink5, size: 22)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Screen capture")
                        .font(Theme.ui(size: 13))
                        .foregroundStyle(Theme.ink2)
                        .lineLimit(1)
                    Text(String(format: "x %.0f%% \u{00B7} y %.0f%%", r.x * 100, r.y * 100))
                        .font(Theme.ui(size: 11))
                        .foregroundStyle(Theme.ink4)
                        .lineLimit(1)
                }
            }
            .frame(width: wApp, alignment: .leading)

            // source — user-drawn regions are always manual.
            HStack { LBPill(text: "Manual", tone: .ghost, size: .sm) }
                .frame(width: wSource, alignment: .leading)

            // size
            Text(sizeStr)
                .font(Theme.mono(size: 12))
                .foregroundStyle(Theme.ink3)
                .frame(width: wSize, alignment: .trailing)

            // hits — accent when active.
            Text("\(hits)")
                .font(Theme.mono(size: 14, weight: .semibold))
                .tracking(-0.14)
                .foregroundStyle(isOn ? Theme.accent : Theme.ink4)
                .frame(width: wHits, alignment: .trailing)

            // toggle
            LBToggle(isOn: Binding(
                get: { controller.regionEnabled(id: r.id) },
                set: { controller.setRegionEnabled(id: r.id, on: $0) }
            ), accessibilityName: "Region \(index + 1) enabled",
               accessibilityIdentifier: "region-library-toggle.\(r.id.uuidString)",
               size: .sm)
            .frame(width: wOn, alignment: .leading)
            .help(isOn ? "Disable this region (keeps it in the list)" : "Re-enable this region")

            // more menu — delete.
            Menu {
                Button(role: .destructive) {
                    controller.regionStore.remove(id: r.id)
                    controller.refreshRegionCount()
                } label: {
                    Label("Delete region", systemImage: "trash")
                }
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Theme.ink4)
                    .frame(width: 24, height: 24)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .frame(width: wMenu, alignment: .center)
            .help("More actions")
        }
        .padding(.horizontal, 16)
        .frame(height: 56)
        .background(index == 0 ? Theme.surface2 : Color.clear)
        .overlay(alignment: .top) {
            if index > 0 { Rectangle().fill(Theme.line).frame(height: 1) }
        }
    }

    // MARK: - Derived data

    private var activeCount: Int {
        controller.regionStore.current.filter { controller.regionEnabled(id: $0.id) }.count
    }

    private var totalBlocks: Int {
        controller.regionStore.current.reduce(0) {
            $0 + controller.captureManager.blocksByRegion[$1.id, default: 0]
        }
    }

    private func badgeInitial(_ name: String) -> String {
        String(name.first.map(String.init)?.uppercased() ?? "R")
    }

    /// Generate a human-readable region name from its position + size.
    /// Examples: "Top banner — 28% × 12%", "Bottom-right tile — 18% × 18%".
    private func regionName(_ r: NormalizedRegion) -> String {
        let cx = r.x + r.width / 2.0
        let cy = r.y + r.height / 2.0
        let vertical: String
        switch cy {
        case ..<0.34: vertical = "Top"
        case ..<0.67: vertical = "Middle"
        default: vertical = "Bottom"
        }
        let horizontal: String
        switch cx {
        case ..<0.34: horizontal = "left"
        case ..<0.67: horizontal = "center"
        default: horizontal = "right"
        }
        let location: String
        if vertical == "Middle" && horizontal == "center" { location = "Center" }
        else if horizontal == "center" { location = vertical }
        else if vertical == "Middle" { location = horizontal.capitalized }
        else { location = "\(vertical)-\(horizontal)" }
        let pctW = Int((r.width * 100).rounded())
        let pctH = Int((r.height * 100).rounded())
        return "\(location) \u{2014} \(pctW)% \u{00D7} \(pctH)%"
    }

    private var filteredRegions: [NormalizedRegion] {
        var all = controller.regionStore.current
        // Source filter — user-drawn regions are "Manual"; the detector has no
        // learned regions in the store, so "Learned" yields an empty set.
        switch filter {
        case 2: all = []          // Learned
        default: break            // All / Manual both show every saved region
        }
        guard !search.isEmpty else { return all }
        return all.filter { regionName($0).localizedCaseInsensitiveContains(search) }
    }
}
