import Foundation

/// One detector-class row as emitted by the Rust bridge `get_detector_classes`.
/// Field names + types mirror `DetectorClass` in `liveblock-bridge/src/lib.rs`.
struct DetectorClassRule: Decodable, Equatable {
    let enabled: Bool
    let id: UInt32
    let name: String
    let threshold: Float
}

/// Bridges the shared `liveblock-config` detection settings into the Swift
/// detection path.
///
/// Owns a `VocabularyHandle` (from the generated `LiveBlockBridge`) opened
/// against `~/Library/Application Support/LiveBlock/detection-settings.json`,
/// seeded on first run from the bundled `liveblock-vocab.json`. Exposes a
/// per-class effective-threshold lookup keyed on the Vision label identifier,
/// which for the open-vocab model is the vocab class *name* (e.g. "Logo").
final class DetectionVocabulary: @unchecked Sendable {

    private let lock = NSLock()
    private let handle: VocabularyHandle
    private var rulesByName: [String: DetectorClassRule] = [:]

    /// Production init: open the persisted settings store, seed the vocabulary
    /// from the bundled JSON, then cache the per-class rules.
    init(settingsURL: URL, bundledVocabJSON: String?) {
        if let parent = Optional(settingsURL.deletingLastPathComponent()) {
            try? FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        }
        self.handle = vocabulary_open(settingsURL.path)
        if let json = bundledVocabJSON, !json.isEmpty {
            _ = handle.set_vocabulary(json)
        }
        reloadRules()
    }

    /// Test/seam init: build directly from a `get_detector_classes()`-shaped JSON
    /// payload without touching disk. The owned handle is unused here.
    init(detectorClassesJSON: String) {
        self.handle = VocabularyHandle()
        self.rulesByName = Self.parse(detectorClassesJSON)
    }

    /// Re-read the current class rules (enabled flags + effective thresholds)
    /// from the Rust store. Call after the UI mutates enable/threshold.
    func reloadRules() {
        let json = handle.get_detector_classes().toString()
        let parsed = Self.parse(json)
        lock.lock(); rulesByName = parsed; lock.unlock()
    }

    /// Snapshot of the cached rules (for UI / tests).
    var rules: [DetectorClassRule] {
        lock.lock(); defer { lock.unlock() }
        return Array(rulesByName.values).sorted { $0.id < $1.id }
    }

    private static func parse(_ json: String) -> [String: DetectorClassRule] {
        guard !json.isEmpty,
              let data = json.data(using: .utf8),
              let rows = try? JSONDecoder().decode([DetectorClassRule].self, from: data) else {
            return [:]
        }
        return Dictionary(rows.map { ($0.name, $0) }, uniquingKeysWith: { first, _ in first })
    }

    /// Effective score threshold for a Vision label identifier.
    ///
    /// - Returns `nil` when the label maps to a *known, disabled* class — the
    ///   caller must drop the detection.
    /// - Returns the class's effective threshold when the class is enabled.
    /// - Returns `globalFallback` for labels not present in the vocabulary (e.g.
    ///   a transitional COCO model whose names aren't vocab classes), so
    ///   detection keeps working during the model swap.
    func effectiveThreshold(forLabel label: String, globalFallback: Float) -> Float? {
        lock.lock(); defer { lock.unlock() }
        guard let rule = rulesByName[label] else { return globalFallback }
        return rule.enabled ? rule.threshold : nil
    }
}
