//! Temporal multi-object tracker for LiveBlock.
//!
//! The detector runs only every Nth frame (`CoordinatorConfig::detect_every`),
//! and today the pipeline reuses the previous frame's boxes verbatim — so masks
//! lag and snap on moving content (race-car liveries, running players, panning
//! cameras). This tracker fixes that: it associates per-detection boxes into
//! persistent tracks with stable ids, learns a per-track velocity to advance
//! masks at full render rate between detections, and carries a **sticky
//! keep/remove verdict** so the (expensive) sponsor-vs-team/number classifier
//! runs ONCE per track instead of every frame.
//!
//! It is a constant-velocity association tracker (SORT-family). All geometry is
//! in normalized `[0..1]` top-left coordinates, matching
//! `liveblock_detection::Detection`, so it is resolution- and
//! platform-independent. A full Kalman covariance / observation-centric
//! recovery (OC-SORT) and global motion compensation for whip-pans are layered
//! refinements that slot on top of this association core.

/// Normalized bounding box, `[0..1]`, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl TrackBox {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Center point `(cx, cy)`.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    fn from_center(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self { x: cx - w * 0.5, y: cy - h * 0.5, width: w, height: h }
    }

    /// Intersection-over-union with another box.
    pub fn iou(&self, other: &TrackBox) -> f32 {
        let ax2 = self.x + self.width;
        let ay2 = self.y + self.height;
        let bx2 = other.x + other.width;
        let by2 = other.y + other.height;
        let ix = (ax2.min(bx2) - self.x.max(other.x)).max(0.0);
        let iy = (ay2.min(by2) - self.y.max(other.y)).max(0.0);
        let inter = ix * iy;
        let union = self.width * self.height + other.width * other.height - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }

    /// Clamp into the unit frame, preserving size where possible.
    fn clamped(self) -> Self {
        let w = self.width.clamp(0.0, 1.0);
        let h = self.height.clamp(0.0, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - h),
            width: w,
            height: h,
        }
    }
}

/// Sticky semantic decision attached to a track. `Unsure` means the classifier
/// has not yet run for this track; the caller classifies only tracks where
/// [`Track::needs_classification`] is true, then [`Tracker::set_verdict`]s the
/// result, which then persists for the track's lifetime (re-verification, e.g.
/// for a rotating LED board, is the caller's decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Preserve these pixels — a team/league mark or a player/car number.
    Keep,
    /// Inpaint/cover these pixels — a commercial sponsor mark or general ad.
    Remove,
    /// Not yet classified.
    Unsure,
}

/// One detection fed into the tracker for a tick.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    pub bbox: TrackBox,
    pub class_id: u32,
}

impl Observation {
    pub fn new(bbox: TrackBox, class_id: u32) -> Self {
        Self { bbox, class_id }
    }
}

pub type TrackId = u64;

/// A persistent track.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: TrackId,
    pub bbox: TrackBox,
    /// Center velocity in normalized units per detection-tick.
    pub velocity: (f32, f32),
    /// Detection-ticks since the track was created.
    pub age: u32,
    /// Total observations associated to this track.
    pub hits: u32,
    /// Ticks since the last association (0 == updated this tick).
    pub time_since_update: u32,
    pub class_id: u32,
    pub verdict: Verdict,
}

impl Track {
    /// True while the classifier still needs to decide keep-vs-remove.
    pub fn needs_classification(&self) -> bool {
        matches!(self.verdict, Verdict::Unsure)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// Minimum IoU between a predicted track box and an observation to associate.
    pub iou_match_threshold: f32,
    /// Ticks a track may coast unmatched before it is retired.
    pub max_age: u32,
    /// Hits before a track is reported as confirmed.
    pub min_hits: u32,
    /// EMA factor `[0..1]` applied to learned velocity (higher = more reactive).
    pub velocity_smoothing: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            iou_match_threshold: 0.3,
            max_age: 30,
            min_hits: 3,
            velocity_smoothing: 0.5,
        }
    }
}

/// Constant-velocity association tracker.
pub struct Tracker {
    config: TrackerConfig,
    tracks: Vec<Track>,
    next_id: TrackId,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub fn new() -> Self {
        Self::with_config(TrackerConfig::default())
    }

    pub fn with_config(config: TrackerConfig) -> Self {
        Self {
            config,
            tracks: Vec::new(),
            next_id: 1,
        }
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Tracks that have accumulated enough hits and were updated this tick.
    pub fn confirmed_tracks(&self) -> impl Iterator<Item = &Track> {
        let min_hits = self.config.min_hits;
        self.tracks
            .iter()
            .filter(move |t| t.hits >= min_hits && t.time_since_update == 0)
    }

    /// Set a track's verdict. The tracker never resets a verdict on `update`, so
    /// once the caller classifies a track the decision sticks. Returns false if
    /// no track has `id`.
    pub fn set_verdict(&mut self, id: TrackId, verdict: Verdict) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
            t.verdict = verdict;
            true
        } else {
            false
        }
    }

    /// Advance one detection tick: predict every track forward by its velocity,
    /// greedily associate observations by IoU, update matched tracks, coast
    /// unmatched ones, retire stale ones, and spawn tracks for unmatched
    /// observations. Returns the ids of tracks that matched an observation (so
    /// the caller can map fresh classifier verdicts back onto them).
    pub fn update(&mut self, observations: &[Observation]) -> Vec<TrackId> {
        // 1. Predict each track's box forward by one tick of velocity.
        let predicted: Vec<TrackBox> = self
            .tracks
            .iter()
            .map(|t| {
                let (cx, cy) = t.bbox.center();
                TrackBox::from_center(
                    cx + t.velocity.0,
                    cy + t.velocity.1,
                    t.bbox.width,
                    t.bbox.height,
                )
            })
            .collect();

        // 2. Greedy IoU association (highest-overlap pairs first).
        let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
        for (ti, pb) in predicted.iter().enumerate() {
            for (oi, obs) in observations.iter().enumerate() {
                let v = pb.iou(&obs.bbox);
                if v >= self.config.iou_match_threshold {
                    pairs.push((ti, oi, v));
                }
            }
        }
        pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut obs_matched = vec![false; observations.len()];
        let mut match_for_track: Vec<Option<usize>> = vec![None; self.tracks.len()];
        for (ti, oi, _) in pairs {
            if match_for_track[ti].is_none() && !obs_matched[oi] {
                match_for_track[ti] = Some(oi);
                obs_matched[oi] = true;
            }
        }

        // 3. Update tracks (matched -> measure; unmatched -> coast).
        let alpha = self.config.velocity_smoothing.clamp(0.0, 1.0);
        let mut matched_ids = Vec::new();
        for (ti, track) in self.tracks.iter_mut().enumerate() {
            track.age += 1;
            if let Some(oi) = match_for_track[ti] {
                let obs = &observations[oi];
                let (old_cx, old_cy) = track.bbox.center();
                let (new_cx, new_cy) = obs.bbox.center();
                let measured = (new_cx - old_cx, new_cy - old_cy);
                track.velocity = (
                    alpha * measured.0 + (1.0 - alpha) * track.velocity.0,
                    alpha * measured.1 + (1.0 - alpha) * track.velocity.1,
                );
                track.bbox = obs.bbox.clamped();
                track.class_id = obs.class_id;
                track.hits += 1;
                track.time_since_update = 0;
                matched_ids.push(track.id);
            } else {
                track.bbox = predicted[ti].clamped();
                track.time_since_update += 1;
            }
        }

        // 4. Retire stale tracks.
        let max_age = self.config.max_age;
        self.tracks.retain(|t| t.time_since_update <= max_age);

        // 5. Spawn new tracks for unmatched observations.
        for (oi, obs) in observations.iter().enumerate() {
            if !obs_matched[oi] {
                let id = self.next_id;
                self.next_id += 1;
                self.tracks.push(Track {
                    id,
                    bbox: obs.bbox.clamped(),
                    velocity: (0.0, 0.0),
                    age: 0,
                    hits: 1,
                    time_since_update: 0,
                    class_id: obs.class_id,
                    verdict: Verdict::Unsure,
                });
            }
        }

        matched_ids
    }

    /// Non-mutating prediction for inter-detection mask propagation. Returns each
    /// track's box advanced `ticks_ahead` detection-ticks into the future (use a
    /// fraction, e.g. `0.25` for the first render frame after a detection when
    /// `detect_every == 4`). This is what keeps masks glued to moving content at
    /// 60 Hz while detection runs at ~15 Hz.
    pub fn predicted_boxes(&self, ticks_ahead: f32) -> Vec<(TrackId, TrackBox)> {
        self.tracks
            .iter()
            .map(|t| {
                let (cx, cy) = t.bbox.center();
                let b = TrackBox::from_center(
                    cx + t.velocity.0 * ticks_ahead,
                    cy + t.velocity.1 * ticks_ahead,
                    t.bbox.width,
                    t.bbox.height,
                )
                .clamped();
                (t.id, b)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(x: f32, y: f32, w: f32, h: f32, cls: u32) -> Observation {
        Observation::new(TrackBox::new(x, y, w, h), cls)
    }

    #[test]
    fn iou_self_is_one_and_partial_overlap_between() {
        let a = TrackBox::new(0.0, 0.0, 0.2, 0.2);
        let b = TrackBox::new(0.1, 0.1, 0.2, 0.2);
        assert!((a.iou(&a) - 1.0).abs() < 1e-6);
        let v = a.iou(&b);
        assert!(v > 0.0 && v < 1.0, "got {v}");
    }

    #[test]
    fn new_observation_creates_unsure_track() {
        let mut t = Tracker::new();
        let matched = t.update(&[obs(0.1, 0.1, 0.2, 0.2, 7)]);
        assert!(matched.is_empty(), "a brand-new track matches no existing track");
        assert_eq!(t.tracks().len(), 1);
        let tr = &t.tracks()[0];
        assert_eq!(tr.verdict, Verdict::Unsure);
        assert!(tr.needs_classification());
        assert_eq!(tr.hits, 1);
        assert_eq!(tr.class_id, 7);
    }

    #[test]
    fn moving_box_keeps_same_id_and_learns_velocity() {
        let mut t = Tracker::new();
        t.update(&[obs(0.10, 0.10, 0.20, 0.20, 1)]);
        let id0 = t.tracks()[0].id;
        let matched = t.update(&[obs(0.15, 0.10, 0.20, 0.20, 1)]);
        assert_eq!(t.tracks().len(), 1, "should associate, not spawn a second track");
        assert_eq!(matched, vec![id0]);
        let tr = &t.tracks()[0];
        assert_eq!(tr.id, id0);
        assert_eq!(tr.hits, 2);
        assert!(tr.velocity.0 > 0.0, "rightward velocity expected, got {:?}", tr.velocity);
    }

    #[test]
    fn disjoint_observation_spawns_a_distinct_track() {
        let mut t = Tracker::new();
        t.update(&[obs(0.05, 0.05, 0.10, 0.10, 1)]);
        t.update(&[obs(0.80, 0.80, 0.10, 0.10, 2)]);
        assert_eq!(t.tracks().len(), 2);
        let ids: Vec<_> = t.tracks().iter().map(|x| x.id).collect();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn stale_track_is_retired_after_max_age() {
        let mut t = Tracker::with_config(TrackerConfig {
            max_age: 3,
            ..TrackerConfig::default()
        });
        t.update(&[obs(0.1, 0.1, 0.2, 0.2, 1)]);
        for _ in 0..3 {
            t.update(&[]); // time_since_update -> 1, 2, 3 (all <= max_age)
        }
        assert_eq!(t.tracks().len(), 1, "alive while tsu <= max_age");
        t.update(&[]); // tsu -> 4 > max_age
        assert!(t.tracks().is_empty(), "retired after exceeding max_age");
    }

    #[test]
    fn verdict_is_sticky_across_updates() {
        let mut t = Tracker::new();
        t.update(&[obs(0.1, 0.1, 0.2, 0.2, 1)]);
        let id = t.tracks()[0].id;
        assert!(t.set_verdict(id, Verdict::Remove));
        // Re-observe: the verdict must persist so the classifier isn't re-run.
        t.update(&[obs(0.12, 0.10, 0.2, 0.2, 1)]);
        assert_eq!(t.tracks()[0].verdict, Verdict::Remove);
        assert!(!t.tracks()[0].needs_classification());
    }

    #[test]
    fn predicted_boxes_extrapolate_along_velocity() {
        let mut t = Tracker::new();
        t.update(&[obs(0.10, 0.10, 0.20, 0.20, 1)]); // center 0.20
        t.update(&[obs(0.15, 0.10, 0.20, 0.20, 1)]); // center 0.25, learns +vx
        let preds = t.predicted_boxes(1.0);
        assert_eq!(preds.len(), 1);
        let (_, b) = preds[0];
        assert!(b.center().0 > 0.25, "predicted center x should advance, got {}", b.center().0);
    }

    #[test]
    fn confirmed_only_after_min_hits() {
        let mut t = Tracker::with_config(TrackerConfig {
            min_hits: 3,
            iou_match_threshold: 0.1,
            ..TrackerConfig::default()
        });
        t.update(&[obs(0.10, 0.10, 0.30, 0.30, 1)]);
        assert_eq!(t.confirmed_tracks().count(), 0, "1 hit < min_hits");
        t.update(&[obs(0.11, 0.10, 0.30, 0.30, 1)]);
        assert_eq!(t.confirmed_tracks().count(), 0, "2 hits < min_hits");
        t.update(&[obs(0.12, 0.10, 0.30, 0.30, 1)]);
        assert_eq!(t.confirmed_tracks().count(), 1, "3 hits == min_hits -> confirmed");
    }
}
