import Foundation

/// Automatic removal requires explicit semantic class names from the model.
/// Generic object labels never authorize removal. A detected keep region
/// suppresses the entire overlapping automatic removal box.
enum SponsorClassAllowlist {
    static let autoBlockLabels: Set<String> = ["ad", "sponsor_remove"]

    static let protectedLabels: Set<String> = ["team_keep", "number_keep"]

    /// Unknown and COCO labels never authorize erasure. Even a high-confidence
    /// remove box is suppressed if it overlaps a protected identity mark.
    static func removableDetections(_ detections: [NormalizedDetection]) -> [NormalizedDetection] {
        let protected = detections.filter { protectedLabels.contains($0.label.lowercased()) }
        return detections.filter { candidate in
            allowsAutoBlock(label: candidate.label) && !protected.contains {
                candidate.normRect.intersects($0.normRect)
            }
        }
    }

    /// Case-insensitive match against removable classes.
    static func allowsAutoBlock(label: String) -> Bool {
        guard !autoBlockLabels.isEmpty else { return false }
        return autoBlockLabels.contains(label.lowercased())
    }
}
