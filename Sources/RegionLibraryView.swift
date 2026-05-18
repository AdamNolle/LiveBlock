import SwiftUI

/// Region library — port of `screens.jsx::ScreenRegionLibrary`.
/// Lists every saved region in the store with sort/filter chrome.
struct RegionLibraryView: View {
    @ObservedObject var controller: AppController
    @State private var search: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Region library")
                        .font(Theme.display(size: 26, weight: .bold))
                        .foregroundStyle(Color.primary)
                    Text("\(controller.regionCount) saved \u{00B7} all apps")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Color.secondary)
                }
                Spacer()
                HStack(spacing: 8) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(Color.secondary.opacity(0.6))
                    TextField("Search by name or position\u{2026}", text: $search)
                        .textFieldStyle(.plain)
                        .font(Theme.ui(size: 12))
                }
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(.thinMaterial, in: RoundedRectangle(cornerRadius: Theme.Radius.pill))
                .frame(width: 240)
                Button("Draw region", systemImage: "plus") {
                    controller.toggleEditor()
                }
                .buttonStyle(.glassProminent).tint(Theme.block)
                .help("Open the region editor and draw a new rectangle")
            }

            // Table head
            HStack(spacing: 12) {
                Text("").frame(width: 20)
                Text("REGION").frame(maxWidth: .infinity, alignment: .leading)
                Text("APP / CONTEXT").frame(width: 160, alignment: .leading)
                Text("SIZE").frame(width: 90, alignment: .leading)
                Text("BLOCKS").frame(width: 70, alignment: .leading)
                Text("ACTIVE").frame(width: 70, alignment: .leading)
                Text("").frame(width: 40)
            }
            .font(Theme.ui(size: 10, weight: .semibold))
            .tracking(0.6)
            .foregroundStyle(Color.secondary)
            .padding(.horizontal, 16)

            VStack(spacing: 4) {
                let regions = filteredRegions
                if regions.isEmpty {
                    VStack(spacing: 10) {
                        Image(systemName: "rectangle.dashed")
                            .font(.system(size: 32))
                            .foregroundStyle(Color.secondary.opacity(0.6))
                        Text("No regions yet")
                            .font(Theme.ui(size: 13, weight: .semibold))
                            .foregroundStyle(Color.secondary)
                        Text("Click \u{201C}Draw region\u{201D} above to create your first one.")
                            .font(Theme.ui(size: 11))
                            .foregroundStyle(Color.secondary.opacity(0.6))
                        Button("Draw region", systemImage: "plus") {
                            controller.toggleEditor()
                        }
                        .buttonStyle(.glassProminent).tint(Theme.block)
                        .controlSize(.small)
                        .padding(.top, 4)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 30)
                } else {
                    ForEach(regions, id: \.id) { r in
                        regionRow(r, displayName: regionName(r))
                    }
                }
            }
            .padding(6)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))
            .frame(maxWidth: .infinity, alignment: .top)
        }
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
        let all = controller.regionStore.current
        guard !search.isEmpty else { return all }
        return all.filter { regionName($0).localizedCaseInsensitiveContains(search) }
    }

    private func regionRow(_ r: NormalizedRegion, displayName: String) -> some View {
        let isOn = controller.regionEnabled(id: r.id)
        return HStack(spacing: 12) {
            Image(systemName: "line.3.horizontal").foregroundStyle(Color.secondary.opacity(0.6)).frame(width: 20)
            // preview swatch — proportional thumbnail of the region's normalized rect.
            HStack(spacing: 10) {
                ZStack(alignment: .topLeading) {
                    RoundedRectangle(cornerRadius: 6).fill(Color(red: 0.10, green: 0.07, blue: 0.19))
                        .frame(width: 36, height: 24)
                    RoundedRectangle(cornerRadius: 2)
                        .stroke(Theme.block, style: StrokeStyle(lineWidth: 1, dash: [3]))
                        .frame(width: max(4, 36 * r.width), height: max(3, 24 * r.height))
                        .offset(x: 36 * r.x, y: 24 * r.y)
                }
                .frame(width: 36, height: 24)
                Text(displayName)
                    .font(Theme.ui(size: 12, weight: .medium))
                    .foregroundStyle(Color.primary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 8) {
                Circle()
                    .fill(isOn ? Theme.success : Color.secondary.opacity(0.4))
                    .frame(width: 10, height: 10)
                Text(isOn ? "Active" : "Disabled")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Color.primary)
            }.frame(width: 160, alignment: .leading)

            Text(String(format: "%.0f%%\u{00D7}%.0f%%", r.width * 100, r.height * 100))
                .font(Theme.mono(size: 11))
                .foregroundStyle(Color.secondary)
                .frame(width: 90, alignment: .leading)

            Text("\(controller.captureManager.blocksByRegion[r.id, default: 0])")
                .font(Theme.mono(size: 12, weight: .semibold))
                .foregroundStyle(Theme.block)
                .frame(width: 70, alignment: .leading)

            Toggle("", isOn: Binding(
                get: { controller.regionEnabled(id: r.id) },
                set: { controller.setRegionEnabled(id: r.id, on: $0) }
            ))
            .labelsHidden()
            .frame(width: 70, alignment: .leading)
            .help(isOn ? "Disable this region (keeps it in the list)" : "Re-enable this region")

            Button {
                controller.regionStore.remove(id: r.id)
                controller.refreshRegionCount()
            } label: {
                Image(systemName: "trash")
                    .font(.system(size: 12))
                    .foregroundStyle(Color.secondary.opacity(0.6))
                    .frame(width: 26, height: 26)
            }
            .buttonStyle(.plain)
            .frame(width: 40, alignment: .trailing)
            .help("Delete this region permanently")
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
    }
}
