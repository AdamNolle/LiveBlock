import Foundation

/// Persists the user's per-app exclusion list. When the frontmost app's
/// bundle identifier appears in `excludedBundleIDs`, the capture pipeline
/// pauses (no detection, no inpaint, no patches drawn). Default is empty
/// — block everywhere.
///
/// Stored as a `[String]` in `UserDefaults` under `perAppExcludedBundleIDs`.
/// Small dataset, no need for a JSON file; survives relaunch like the rest
/// of `@AppStorage`-backed settings.
@MainActor
final class PerAppRulesStore: ObservableObject {
    private static let key = "perAppExcludedBundleIDs"

    @Published var excludedBundleIDs: Set<String> {
        didSet {
            UserDefaults.standard.set(Array(excludedBundleIDs).sorted(),
                                      forKey: Self.key)
        }
    }

    init() {
        let stored = UserDefaults.standard.stringArray(forKey: Self.key) ?? []
        self.excludedBundleIDs = Set(stored)
    }

    func isExcluded(_ bundleID: String) -> Bool {
        guard !bundleID.isEmpty else { return false }
        return excludedBundleIDs.contains(bundleID)
    }

    func setExcluded(_ bundleID: String, excluded: Bool) {
        guard !bundleID.isEmpty else { return }
        if excluded {
            excludedBundleIDs.insert(bundleID)
        } else {
            excludedBundleIDs.remove(bundleID)
        }
    }

    func toggle(_ bundleID: String) {
        setExcluded(bundleID, excluded: !isExcluded(bundleID))
    }
}
