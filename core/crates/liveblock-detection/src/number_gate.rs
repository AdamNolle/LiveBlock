//! Number-protect Stage-2b gate.
//!
//! A jersey / car number must never be erased. After OCR extracts text from a
//! detection box, [`is_protected_number`] decides whether that text-plus-geometry
//! looks like a bold numeral worth protecting. The result feeds
//! `ClassifySignals::is_protected_number`, which forces a Keep in
//! `liveblock_core::decide_verdict`.
//!
//! Pure compute: no OCR, no model — just the string + aspect-ratio rules.

/// True when `text` reads as a protected numeral and the box geometry is
/// plausible for a bold, prominent number.
///
/// Rules (all must hold):
///  - After trimming surrounding whitespace, the text is **non-empty** and made
///    up **only of ASCII digits** (`0`–`9`). No letters, punctuation, or signs:
///    a jersey number is "23", never "23A" or "#23".
///  - **At most 3 digits** (`len <= 3`): real player/car numbers are 1–3 digits;
///    longer digit runs are scoreboards, timers, prices, phone numbers — not a
///    protected number.
///  - The box **aspect ratio is plausible** for a rendered numeral or short
///    number group. A single bold digit is taller than wide; a 2–3 digit group
///    trends toward square / slightly wide. We accept a generous band
///    (`0.15 ..= 4.0` width/height) so we never *fail* to protect a real number
///    on tight cropping, while still rejecting absurdly thin slivers or extreme
///    letterbox strips that can't be a numeral.
///
/// `box_w` / `box_h` are the detection box dimensions; any consistent unit works
/// (pixels or normalized) since only their ratio is used. Non-positive or
/// non-finite dimensions are rejected.
pub fn is_protected_number(text: &str, box_w: f32, box_h: f32) -> bool {
    let trimmed = text.trim();

    // Digits-only, non-empty, length <= 3.
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() > 3 {
        return false;
    }
    if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }

    // Geometry must be finite and positive.
    if !box_w.is_finite() || !box_h.is_finite() || box_w <= 0.0 || box_h <= 0.0 {
        return false;
    }

    // Plausible aspect for a bold numeral / short number group.
    let aspect = box_w / box_h;
    const MIN_ASPECT: f32 = 0.15;
    const MAX_ASPECT: f32 = 4.0;
    (MIN_ASPECT..=MAX_ASPECT).contains(&aspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_single_digit_tall_box() {
        // A bold "7", taller than wide.
        assert!(is_protected_number("7", 30.0, 50.0));
    }

    #[test]
    fn accepts_two_digit_squareish_box() {
        assert!(is_protected_number("23", 60.0, 50.0));
    }

    #[test]
    fn accepts_three_digit_wide_box() {
        assert!(is_protected_number("100", 120.0, 50.0));
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert!(is_protected_number("  44 ", 50.0, 50.0));
    }

    #[test]
    fn rejects_four_plus_digits() {
        // Scoreboard / timer / price — not a jersey number.
        assert!(!is_protected_number("1234", 80.0, 50.0));
        assert!(!is_protected_number("88888", 100.0, 50.0));
    }

    #[test]
    fn rejects_non_digits() {
        assert!(!is_protected_number("23A", 60.0, 50.0));
        assert!(!is_protected_number("#23", 60.0, 50.0));
        assert!(!is_protected_number("-7", 30.0, 50.0));
        assert!(!is_protected_number("ad", 60.0, 50.0));
        assert!(!is_protected_number("1.5", 60.0, 50.0));
    }

    #[test]
    fn rejects_empty_or_whitespace() {
        assert!(!is_protected_number("", 30.0, 50.0));
        assert!(!is_protected_number("   ", 30.0, 50.0));
    }

    #[test]
    fn rejects_extreme_aspect_ratios() {
        // Ultra-wide strip (a banner, not a number).
        assert!(!is_protected_number("7", 500.0, 50.0)); // aspect 10
        // Ultra-thin vertical sliver.
        assert!(!is_protected_number("7", 5.0, 500.0)); // aspect 0.01
    }

    #[test]
    fn accepts_aspect_band_edges() {
        // Right at the wide bound (aspect == 4.0).
        assert!(is_protected_number("23", 200.0, 50.0));
        // Right at the tall bound (aspect == 0.15).
        assert!(is_protected_number("1", 15.0, 100.0));
    }

    #[test]
    fn rejects_bad_geometry() {
        assert!(!is_protected_number("7", 0.0, 50.0));
        assert!(!is_protected_number("7", 50.0, 0.0));
        assert!(!is_protected_number("7", -10.0, 50.0));
        assert!(!is_protected_number("7", f32::NAN, 50.0));
        assert!(!is_protected_number("7", 50.0, f32::INFINITY));
    }
}
