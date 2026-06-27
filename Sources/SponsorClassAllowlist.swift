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
/// When a real fine-tuned sponsor/ad model is wired in (see ASSESSMENT.md
/// Path A / the Rust `decide_verdict` + class-allowlist design), populate
/// `autoBlockLabels` with that model's removable class identifiers (e.g.
/// "sponsor", "ad", "logo"). Keep team emblems / player numbers OUT of this
/// list so they are never auto-erased — that carve-out is enforced in the core
/// via `build_remove_mask_from_tracks`, and mirrored here by simply not
/// allowlisting those classes.
enum SponsorClassAllowlist {

    /// Lower-cased class identifiers that the auto-block pipeline is permitted
    /// to erase. INTENTIONALLY EMPTY until a real sponsor/logo model ships.
    ///
    /// TODO(model): replace with the fine-tuned model's removable classes,
    /// e.g. ["sponsor", "ad", "logo", "banner"]. Until then this MUST stay
    /// empty so no generic COCO class auto-erases user content.
    static let autoBlockLabels: Set<String> = []

    /// Returns `true` only when `label` is an explicitly allowlisted removable
    /// class. With an empty allowlist this is always `false`, so model
    /// detections never reach the eraser. Case-insensitive.
    static func allowsAutoBlock(label: String) -> Bool {
        guard !autoBlockLabels.isEmpty else { return false }
        return autoBlockLabels.contains(label.lowercased())
    }
}
