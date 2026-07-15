import Foundation

private let perAppRulesStorageKey = "perAppExcludedBundleIDs"

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
    private let defaults: UserDefaults
    private let storageKey: String

    @Published var excludedBundleIDs: Set<String> {
        didSet {
            defaults.set(Array(excludedBundleIDs).sorted(), forKey: storageKey)
        }
    }

    init(defaults: UserDefaults = .standard,
         storageKey: String = perAppRulesStorageKey) {
        self.defaults = defaults
        self.storageKey = storageKey
        let stored = defaults.stringArray(forKey: storageKey) ?? []
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
