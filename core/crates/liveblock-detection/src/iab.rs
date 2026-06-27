//! Geometric IAB ad-slot proposer.
//!
//! Standard display-ad units have fixed pixel dimensions, so a detection box
//! whose **aspect ratio** matches a known IAB slot is a strong geometric prior
//! that it's an ad — independent of any ML class. [`iab_slot_match`] returns the
//! canonical name of the closest standard slot within tolerance, or `None`.
//!
//! This is a *proposer*, not a verdict: a positive match raises ad probability
//! (it can populate `ClassifySignals::sponsor_prob` / a slot-prior), but the
//! keep-vs-remove decision still runs through the policy + protect gates so a
//! team emblem that happens to be 300x250 is never erased on geometry alone.
//!
//! Pure compute: matches on aspect ratio (scale-invariant), so it works whether
//! the box is given at native slot size or scaled by display DPI.

/// One standard IAB display slot: a human name and its nominal pixel size.
struct Slot {
    name: &'static str,
    w: f32,
    h: f32,
}

/// The standard slots we recognize (the set named in the task). Aspect ratio is
/// derived from `w/h` at match time.
const SLOTS: &[Slot] = &[
    Slot { name: "medium_rectangle", w: 300.0, h: 250.0 }, // 300x250  (1.20)
    Slot { name: "leaderboard", w: 728.0, h: 90.0 },       // 728x90   (8.09)
    Slot { name: "wide_skyscraper", w: 160.0, h: 600.0 },  // 160x600  (0.267)
    Slot { name: "billboard", w: 970.0, h: 250.0 },        // 970x250  (3.88)
    Slot { name: "half_page", w: 300.0, h: 600.0 },        // 300x600  (0.50)
    Slot { name: "mobile_leaderboard", w: 320.0, h: 50.0 }, // 320x50  (6.40)
];

/// Default relative tolerance on the aspect ratio (±8%). Chosen tight enough to
/// keep the distinct slots from colliding (the closest pair, 728x90 ≈ 8.09 and
/// 320x50 = 6.40, are ~21% apart) yet loose enough to absorb sub-pixel cropping.
pub const DEFAULT_ASPECT_TOLERANCE: f32 = 0.08;

/// Match a detection box (in pixels) to a standard IAB slot by aspect ratio,
/// using [`DEFAULT_ASPECT_TOLERANCE`]. Returns the canonical slot name of the
/// single closest match within tolerance, or `None`.
pub fn iab_slot_match(box_w_px: f32, box_h_px: f32) -> Option<&'static str> {
    iab_slot_match_tol(box_w_px, box_h_px, DEFAULT_ASPECT_TOLERANCE)
}

/// Like [`iab_slot_match`] with an explicit relative aspect tolerance.
///
/// A slot matches when the relative difference between the box aspect and the
/// slot aspect is within `tol` (i.e. `|box_aspect - slot_aspect| / slot_aspect
/// <= tol`). When several slots qualify, the closest (smallest relative error)
/// wins.
pub fn iab_slot_match_tol(box_w_px: f32, box_h_px: f32, tol: f32) -> Option<&'static str> {
    if !box_w_px.is_finite() || !box_h_px.is_finite() || box_w_px <= 0.0 || box_h_px <= 0.0 {
        return None;
    }
    let box_aspect = box_w_px / box_h_px;

    let mut best: Option<(&'static str, f32)> = None;
    for slot in SLOTS {
        let slot_aspect = slot.w / slot.h;
        let rel_err = (box_aspect - slot_aspect).abs() / slot_aspect;
        if rel_err <= tol {
            match best {
                Some((_, e)) if rel_err >= e => {}
                _ => best = Some((slot.name, rel_err)),
            }
        }
    }
    best.map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_slot_sizes() {
        assert_eq!(iab_slot_match(300.0, 250.0), Some("medium_rectangle"));
        assert_eq!(iab_slot_match(728.0, 90.0), Some("leaderboard"));
        assert_eq!(iab_slot_match(160.0, 600.0), Some("wide_skyscraper"));
        assert_eq!(iab_slot_match(970.0, 250.0), Some("billboard"));
        assert_eq!(iab_slot_match(300.0, 600.0), Some("half_page"));
        assert_eq!(iab_slot_match(320.0, 50.0), Some("mobile_leaderboard"));
    }

    #[test]
    fn matches_scaled_slots_aspect_is_scale_invariant() {
        // 2x DPI scaling preserves aspect -> still matches.
        assert_eq!(iab_slot_match(600.0, 500.0), Some("medium_rectangle"));
        assert_eq!(iab_slot_match(1456.0, 180.0), Some("leaderboard"));
        // Fractional scale.
        assert_eq!(iab_slot_match(150.0, 125.0), Some("medium_rectangle"));
    }

    #[test]
    fn matches_within_tolerance() {
        // 300x250 -> 1.20. A box 306x250 -> 1.224, ~2% off, within 8%.
        assert_eq!(iab_slot_match(306.0, 250.0), Some("medium_rectangle"));
    }

    #[test]
    fn rejects_outside_tolerance() {
        // A near-square box matches no slot (closest is 300x250 at 1.20).
        assert_eq!(iab_slot_match(250.0, 250.0), None);
        // A 16:9-ish video frame.
        assert_eq!(iab_slot_match(1920.0, 1080.0), None);
    }

    #[test]
    fn distinct_slots_do_not_collide() {
        // Each exact slot resolves to itself, never a neighbor.
        let cases = [
            (300.0, 250.0, "medium_rectangle"),
            (728.0, 90.0, "leaderboard"),
            (160.0, 600.0, "wide_skyscraper"),
            (970.0, 250.0, "billboard"),
            (300.0, 600.0, "half_page"),
            (320.0, 50.0, "mobile_leaderboard"),
        ];
        for (w, h, name) in cases {
            assert_eq!(iab_slot_match(w, h), Some(name), "slot {name}");
        }
    }

    #[test]
    fn picks_closest_when_two_in_tolerance() {
        // With a deliberately huge tolerance both the leaderboard (8.09) and the
        // mobile leaderboard (6.40) qualify for an aspect ~7.0; the closer one
        // (mobile leaderboard, |7.0-6.4|/6.4 = 0.094 vs |7.0-8.09|/8.09 = 0.135)
        // must win.
        assert_eq!(iab_slot_match_tol(700.0, 100.0, 0.5), Some("mobile_leaderboard"));
    }

    #[test]
    fn rejects_bad_geometry() {
        assert_eq!(iab_slot_match(0.0, 250.0), None);
        assert_eq!(iab_slot_match(300.0, 0.0), None);
        assert_eq!(iab_slot_match(-300.0, 250.0), None);
        assert_eq!(iab_slot_match(f32::NAN, 250.0), None);
        assert_eq!(iab_slot_match(300.0, f32::INFINITY), None);
    }
}
