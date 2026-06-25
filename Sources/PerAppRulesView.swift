import SwiftUI
import AppKit

/// Settings → Per-app rules pane — reskinned to the v4 dark dashboard
/// (design/project/screens-app.jsx · "Apps & sites" app-list style). A
/// `.lbCard` list of per-app rules: each row is an app icon/AppBadge + name +
/// bundle id + a small `LBToggle` (block on/off), separated by hairline
/// dividers. An `LBSectionHeader` tops the screen and a manual `LBField` +
/// add `LBButton` lets users add apps that aren't running yet.
///
/// Lists running, dock-visible apps with a toggle that adds / removes the
/// app's bundle identifier from `PerAppRulesStore.excludedBundleIDs`. When
/// the frontmost app is excluded, the capture pipeline pauses (see
/// `AppController.handleFrontmostAppChange` and the `frontmostIsExcluded`
/// gate in `ScreenCaptureManager`). All store bindings are preserved.
struct PerAppRulesPane: View {
    @ObservedObject var rules: PerAppRulesStore
    let currentBundleID: String?
    let currentName: String?

    @State private var apps: [NSRunningApplication] = []
    @State private var refreshTimer: Timer?
    @State private var manualBundleID: String = ""

    // Deterministic AppBadge palette for apps without an icon.
    private static let badgePalette: [Color] = [
        Theme.accent, Theme.info, Theme.ml, Theme.success, Theme.warn,
        Color(hex: 0x00C7BE), Color(hex: 0xEC407A), Color(hex: 0xFF9500),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            LBSectionHeader(
                title: "Apps & sites",
                subtitle: "Pause LiveBlock when these apps are frontmost. Useful for password managers, banking apps, or when you specifically want ads visible (e.g. previewing a campaign)."
            )

            frontmostBand

            appListCard

            addAppRow

            footerRow
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.bg)
        .preferredColorScheme(.dark)
        .onAppear {
            refreshApps()
            // Cheap poll: the list re-renders when an app launches or quits.
            refreshTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { _ in
                Task { @MainActor in refreshApps() }
            }
        }
        .onDisappear {
            refreshTimer?.invalidate()
            refreshTimer = nil
        }
    }

    // MARK: - Frontmost indicator

    /// Live indicator: which app is frontmost right now, and whether we're
    /// paused for it. Helps the user verify the rule works.
    private var frontmostBand: some View {
        HStack(spacing: 10) {
            if let name = currentName {
                let paused = currentBundleID.map { rules.isExcluded($0) } ?? false
                StatusDot(color: paused ? Theme.warn : Theme.success, size: 7, pulse: !paused)
                Text("Frontmost")
                    .font(Theme.ui(size: 12, weight: .medium))
                    .foregroundStyle(Theme.ink3)
                Text(name)
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink1)
                if paused {
                    LBPill(text: "Paused", tone: .warn, size: .sm)
                }
            } else {
                StatusDot(color: Theme.ink4, size: 7)
                Text("No frontmost app detected")
                    .font(Theme.ui(size: 12, weight: .medium))
                    .foregroundStyle(Theme.ink3)
            }
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .lbCard(Theme.surface2, radius: Theme.Radius.r3, stroke: Theme.line)
    }

    // MARK: - App list

    private var appListCard: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Where it's working")
                    .font(Theme.ui(size: 12, weight: .semibold))
                    .foregroundStyle(Theme.ink2)
                Rectangle().fill(Theme.line).frame(height: 1)
                Text("\(apps.count) apps")
                    .font(Theme.mono(size: 11))
                    .foregroundStyle(Theme.ink4)
            }
            .padding(.horizontal, 14)
            .padding(.top, 12)
            .padding(.bottom, 4)

            ForEach(Array(apps.enumerated()), id: \.element.processIdentifier) { idx, app in
                appRow(app)
                if idx < apps.count - 1 {
                    Rectangle()
                        .fill(Theme.line)
                        .frame(height: 1)
                        .padding(.horizontal, 14)
                }
            }
        }
        .padding(.bottom, 6)
        .frame(maxWidth: .infinity)
        .lbCard()
    }

    private func appRow(_ app: NSRunningApplication) -> some View {
        let bundleID = app.bundleIdentifier ?? ""
        let name = app.localizedName ?? bundleID
        let excluded = rules.isExcluded(bundleID)

        return HStack(spacing: 11) {
            appIcon(app, name: name)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(Theme.ui(size: 13, weight: .semibold))
                    .tracking(-0.06)
                    .foregroundStyle(Theme.ink1)
                Text(bundleID)
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Theme.ink3)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer()
            if excluded {
                LBPill(text: "Paused", tone: .warn, size: .sm)
            }
            LBToggle(
                isOn: Binding(
                    get: { excluded },
                    set: { rules.setExcluded(bundleID, excluded: $0) }
                ),
                size: .sm
            )
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    @ViewBuilder
    private func appIcon(_ app: NSRunningApplication, name: String) -> some View {
        if let icon = app.icon {
            Image(nsImage: icon)
                .resizable()
                .frame(width: 26, height: 26)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.r2, style: .continuous))
        } else {
            let initial = String(name.prefix(1)).uppercased()
            let color = Self.badgePalette[abs(name.hashValue) % Self.badgePalette.count]
            AppBadge(initial: initial.isEmpty ? "?" : initial, color: color, size: 26)
        }
    }

    // MARK: - Manual add

    /// Manual entry — for apps that aren't running yet, or background helpers
    /// that don't appear in the dock-visible list above.
    private var addAppRow: some View {
        HStack(spacing: 8) {
            LBField(
                text: $manualBundleID,
                placeholder: "com.example.app  (bundle identifier)",
                systemIcon: "plus.app"
            )
            LBButton(
                title: "Add app",
                variant: .secondary,
                systemIcon: "plus",
                action: addManualExclusion
            )
            .opacity(manualBundleID.trimmingCharacters(in: .whitespaces).isEmpty ? 0.5 : 1)
            .disabled(manualBundleID.trimmingCharacters(in: .whitespaces).isEmpty)
        }
    }

    // MARK: - Footer

    private var footerRow: some View {
        HStack(spacing: 8) {
            if !rules.excludedBundleIDs.isEmpty {
                LBButton(title: "Clear exclusions", variant: .ghost, size: .sm) {
                    rules.excludedBundleIDs = []
                }
            }
            Spacer()
            Caption("\(rules.excludedBundleIDs.count) excluded")
        }
    }

    // MARK: - Behaviour (unchanged)

    private func addManualExclusion() {
        let id = manualBundleID.trimmingCharacters(in: .whitespaces)
        guard !id.isEmpty else { return }
        rules.setExcluded(id, excluded: true)
        manualBundleID = ""
    }

    private func refreshApps() {
        let visible = NSWorkspace.shared.runningApplications.filter {
            $0.activationPolicy == .regular && $0.bundleIdentifier != nil
        }
        // Stable sort: pinned-excluded first, then alphabetical by name.
        apps = visible.sorted { a, b in
            let aExcluded = rules.isExcluded(a.bundleIdentifier ?? "")
            let bExcluded = rules.isExcluded(b.bundleIdentifier ?? "")
            if aExcluded != bExcluded { return aExcluded && !bExcluded }
            let an = a.localizedName ?? ""
            let bn = b.localizedName ?? ""
            return an.localizedCaseInsensitiveCompare(bn) == .orderedAscending
        }
    }
}
