import Foundation

/// The single gate that decides whether a *model* detection is allowed to
/// auto-erase content. This is the macOS mirror of the shared-core "SAFE STUB
/// FOR THE NOT-YET-INTEGRATED CLASSIFIER": until a real sponsor/logo model
/// ships, the allowlist is intentionally EMPTY, so the generic COCO detector
/// (person, car, dog, …) can never auto-erase anything.
///
/// Background — the confirmed "class-blind erasure" bug: the capture pipeline
/// used to inpaint EVERY COCO detection above the confidence threshold, which
/// erased people and cars on screen. The model bundled with the app is generic
/// COCO weights, NOT an ad/sponsor detector, so NONE of its classes should
/// drive the eraser. User-drawn regions are unaffected — they never pass
/// through this gate and continue to paint/inpaint exactly as before.
///
/// Only a trained runtime model with a passing validation manifest can enable
/// known removable classes. The bundled generic COCO model remains inert.
enum SponsorClassAllowlist {
    static let keepLabels: Set<String> = ["team_keep", "number_keep"]
    private static let removableLabels: Set<String> = ["ad", "sponsor_remove"]

    static func eligibleLabels(in runtimeDirectory: URL) -> Set<String> {
        let manifest = runtimeDirectory.appendingPathComponent("yolov8n.manifest.json")
        guard let data = try? Data(contentsOf: manifest),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              json["schema_version"] as? Int == 1,
              json["validated"] as? Bool == true,
              let precision = json["precision"] as? Double, precision >= 0.8,
              let map50 = json["map50"] as? Double, map50 >= 0.5,
              let classes = json["classes"] as? [String],
              let permitted = json["auto_block_labels"] as? [String] else {
            return []
        }
        let available = Set(classes)
        let candidates = Set(permitted).intersection(removableLabels).intersection(available)
        if candidates.contains("sponsor_remove") &&
            !keepLabels.isSubset(of: available) {
            return candidates.subtracting(["sponsor_remove"])
        }
        return candidates
    }

    static func allowsAutoBlock(label: String, eligible: Set<String>) -> Bool {
        eligible.contains(label.lowercased())
    }
}
