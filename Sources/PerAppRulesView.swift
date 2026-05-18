import SwiftUI
import AppKit

/// Settings → Per-app rules pane.
///
/// Lists running, dock-visible apps with a toggle that adds / removes the
/// app's bundle identifier from `PerAppRulesStore.excludedBundleIDs`. When
/// the frontmost app is excluded, the capture pipeline pauses (see
/// `AppController.handleFrontmostAppChange` and the `frontmostIsExcluded`
/// gate in `ScreenCaptureManager`).
struct PerAppRulesPane: View {
    @ObservedObject var rules: PerAppRulesStore
    let currentBundleID: String?
    let currentName: String?

    @State private var apps: [NSRunningApplication] = []
    @State private var refreshTimer: Timer?
    @State private var manualBundleID: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Per-app rules")
                    .font(Theme.display(size: 28, weight: .bold))
                    .foregroundStyle(Color.primary)
                Text("Pause LiveBlock when these apps are frontmost. Useful for password managers, banking apps, or when you specifically want ads visible (e.g. previewing a campaign).")
                    .font(Theme.ui(size: 13))
                    .foregroundStyle(Color.secondary)
            }

            // Live indicator: which app is frontmost right now, and whether
            // we're paused for it. Helps the user verify the rule works.
            HStack(spacing: 10) {
                Image(systemName: "dot.radiowaves.left.and.right")
                    .foregroundStyle(Color.secondary)
                if let name = currentName {
                    Text("Frontmost: \(name)")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Color.primary)
                    if let bundleID = currentBundleID, rules.isExcluded(bundleID) {
                        Text("• paused")
                            .font(Theme.ui(size: 12, weight: .semibold))
                            .foregroundStyle(Theme.warn)
                    }
                } else {
                    Text("No frontmost app detected")
                        .font(Theme.ui(size: 12))
                        .foregroundStyle(Color.secondary)
                }
                Spacer()
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))

            // The list of running, user-facing apps.
            VStack(spacing: 0) {
                ForEach(apps, id: \.processIdentifier) { app in
                    appRow(app)
                    Divider().background(Color.black.opacity(0.05))
                }
            }
            .padding(6)
            .glassEffect(in: RoundedRectangle(cornerRadius: 20))
            .frame(maxWidth: .infinity)

            // Manual entry — for apps that aren't running yet, or background
            // helpers that don't appear in the dock-visible list above.
            HStack(spacing: 8) {
                Image(systemName: "plus.app")
                    .foregroundStyle(Color.secondary)
                TextField("com.example.app  (bundle identifier)", text: $manualBundleID)
                    .textFieldStyle(.plain)
                    .font(Theme.mono(size: 12))
                    .onSubmit { addManualExclusion() }
                Button("Add") { addManualExclusion() }
                    .buttonStyle(.glass)
                    .controlSize(.small)
                    .disabled(manualBundleID.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))

            HStack(spacing: 8) {
                if !rules.excludedBundleIDs.isEmpty {
                    Button("Clear exclusions") {
                        rules.excludedBundleIDs = []
                    }
                    .buttonStyle(.borderless)
                }
                Spacer()
                Text("\(rules.excludedBundleIDs.count) excluded")
                    .font(Theme.ui(size: 11))
                    .foregroundStyle(Color.secondary)
            }
        }
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

    private func appRow(_ app: NSRunningApplication) -> some View {
        let bundleID = app.bundleIdentifier ?? ""
        let name = app.localizedName ?? bundleID
        let excluded = rules.isExcluded(bundleID)

        return HStack(spacing: 12) {
            if let icon = app.icon {
                Image(nsImage: icon)
                    .resizable()
                    .frame(width: 28, height: 28)
            } else {
                RoundedRectangle(cornerRadius: 6).fill(Color.secondary.opacity(0.2))
                    .frame(width: 28, height: 28)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(name)
                    .font(Theme.ui(size: 13, weight: .medium))
                    .foregroundStyle(Color.primary)
                Text(bundleID)
                    .font(Theme.mono(size: 10))
                    .foregroundStyle(Color.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer()
            Toggle("", isOn: Binding(
                get: { excluded },
                set: { rules.setExcluded(bundleID, excluded: $0) }
            ))
            .labelsHidden()
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }
}
