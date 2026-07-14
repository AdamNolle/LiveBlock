//! Deterministic policy and telemetry for the Windows capture hot path.
//! Kept free of Win32 types so host tests can exercise backpressure and
//! protected-frame behavior.

use crossbeam_channel::{Receiver, Sender, TrySendError};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A sequenced toggle emitted before (or at) panic must never restart capture
/// after panic teardown wins the lifecycle lock.
pub fn action_is_newer_than_panic(action_sequence: u64, last_panic_sequence: u64) -> bool {
    action_sequence > last_panic_sequence
}

pub fn action_is_newer_than_barriers(
    action_sequence: u64,
    last_panic_sequence: u64,
    last_stop_sequence: u64,
    shutting_down: bool,
) -> bool {
    !shutting_down && action_sequence > last_panic_sequence.max(last_stop_sequence)
}

pub const SUSPENSION_POWER: u8 = 1 << 0;
pub const SUSPENSION_SESSION_LOCK: u8 = 1 << 1;
pub const RECOVERY_DELAYS_MS: [u64; 4] = [500, 1_000, 2_000, 4_000];
pub const FIRST_FRAME_TIMEOUT_MS: u64 = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspensionTransition {
    Unchanged,
    BecameSuspended,
    BecameResumable,
}

pub fn update_suspension_reasons(
    current: u8,
    reason: u8,
    active: bool,
) -> (u8, SuspensionTransition) {
    let next = if active {
        current | reason
    } else {
        current & !reason
    };
    let transition = match (current == 0, next == 0) {
        (true, false) => SuspensionTransition::BecameSuspended,
        (false, true) => SuspensionTransition::BecameResumable,
        _ => SuspensionTransition::Unchanged,
    };
    (next, transition)
}

#[derive(Default)]
pub struct CaptureTelemetry {
    captured_frames: AtomicU64,
    processed_frames: AtomicU64,
    dropped_frames: AtomicU64,
    copy_errors: AtomicU64,
    protected_frames: AtomicU64,
    protected_content: AtomicBool,
    last_frame_unix_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTelemetrySnapshot {
    pub captured_frames: u64,
    pub processed_frames: u64,
    pub dropped_frames: u64,
    pub copy_errors: u64,
    pub protected_frames: u64,
    pub protected_content: bool,
    pub last_frame_unix_ms: u64,
}

impl CaptureTelemetry {
    pub fn captured(&self) {
        self.captured_frames.fetch_add(1, Ordering::Relaxed);
        self.last_frame_unix_ms
            .store(unix_millis(), Ordering::Relaxed);
    }
    pub fn processed(&self) {
        self.processed_frames.fetch_add(1, Ordering::Relaxed);
    }
    pub fn dropped(&self) {
        self.dropped_frames.fetch_add(1, Ordering::Relaxed);
    }
    pub fn copy_error(&self) {
        self.copy_errors.fetch_add(1, Ordering::Relaxed);
    }
    pub fn protected_frame(&self) {
        self.protected_frames.fetch_add(1, Ordering::Relaxed);
    }
    pub fn set_protected(&self, value: bool) {
        self.protected_content.store(value, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> CaptureTelemetrySnapshot {
        CaptureTelemetrySnapshot {
            captured_frames: self.captured_frames.load(Ordering::Relaxed),
            processed_frames: self.processed_frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            copy_errors: self.copy_errors.load(Ordering::Relaxed),
            protected_frames: self.protected_frames.load(Ordering::Relaxed),
            protected_content: self.protected_content.load(Ordering::Relaxed),
            last_frame_unix_ms: self.last_frame_unix_ms.load(Ordering::Relaxed),
        }
    }
}

/// A capacity-one queue where a producer replaces stale queued work rather than
/// blocking the Windows.Graphics.Capture callback.
pub fn enqueue_latest<T>(
    sender: &Sender<T>,
    eviction_receiver: &Receiver<T>,
    item: T,
    telemetry: &CaptureTelemetry,
) {
    match sender.try_send(item) {
        Ok(()) => {}
        Err(TrySendError::Full(item)) => {
            if eviction_receiver.try_recv().is_ok() {
                telemetry.dropped();
            }
            if sender.try_send(item).is_err() {
                telemetry.dropped();
            }
        }
        Err(TrySendError::Disconnected(_)) => telemetry.dropped(),
    }
}

#[derive(Debug, Clone)]
pub struct ProtectedFrameDetector {
    dark_streak: u8,
    clear_streak: u8,
    protected: bool,
    trigger_frames: u8,
    clear_frames: u8,
}

impl Default for ProtectedFrameDetector {
    fn default() -> Self {
        Self {
            dark_streak: 0,
            clear_streak: 0,
            protected: false,
            trigger_frames: 8,
            clear_frames: 3,
        }
    }
}

impl ProtectedFrameDetector {
    /// Returns `Some(true/false)` only when protected-content state changes.
    pub fn observe_bgra(&mut self, bytes: &[u8], width: u32, height: u32) -> Option<bool> {
        let dark = looks_like_protected_black(bytes, width, height);
        if dark {
            self.dark_streak = self.dark_streak.saturating_add(1);
            self.clear_streak = 0;
            if !self.protected && self.dark_streak >= self.trigger_frames {
                self.protected = true;
                return Some(true);
            }
        } else {
            self.dark_streak = 0;
            self.clear_streak = self.clear_streak.saturating_add(1);
            if self.protected && self.clear_streak >= self.clear_frames {
                self.protected = false;
                return Some(false);
            }
        }
        None
    }

    pub fn is_protected(&self) -> bool {
        self.protected
    }
}

/// Sample the frame and require near-total opaque black. This is deliberately
/// conservative: merely dark video or a dark desktop must not trigger.
pub fn looks_like_protected_black(bytes: &[u8], width: u32, height: u32) -> bool {
    let expected = width as usize * height as usize * 4;
    if width < 16 || height < 16 || bytes.len() < expected {
        return false;
    }
    let pixels = width as usize * height as usize;
    let step = (pixels / 2048).max(1);
    let mut sampled = 0usize;
    let mut black = 0usize;
    for index in (0..pixels).step_by(step) {
        let offset = index * 4;
        let b = bytes[offset];
        let g = bytes[offset + 1];
        let r = bytes[offset + 2];
        let a = bytes[offset + 3];
        sampled += 1;
        if r <= 3 && g <= 3 && b <= 3 && a >= 250 {
            black += 1;
        }
    }
    sampled >= 64 && black * 1000 >= sampled * 998
}

#[derive(Debug, Default)]
pub struct ConsecutiveFailureGate {
    failures: u8,
    notified: bool,
}

impl ConsecutiveFailureGate {
    pub fn failure(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= 3 && !self.notified {
            self.notified = true;
            return true;
        }
        false
    }
    pub fn success(&mut self) {
        self.failures = 0;
        self.notified = false;
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;

    #[test]
    fn newest_frame_replaces_stale_queued_work() {
        let (sender, receiver) = bounded(1);
        let telemetry = CaptureTelemetry::default();
        enqueue_latest(&sender, &receiver, 1u32, &telemetry);
        enqueue_latest(&sender, &receiver, 2u32, &telemetry);
        assert_eq!(receiver.recv().unwrap(), 2);
        assert_eq!(telemetry.snapshot().dropped_frames, 1);
    }

    #[test]
    fn sustained_opaque_black_triggers_and_clear_frames_reset() {
        let black = vec![0, 0, 0, 255].repeat(64 * 64);
        let mut detector = ProtectedFrameDetector::default();
        for _ in 0..7 {
            assert_eq!(detector.observe_bgra(&black, 64, 64), None);
        }
        assert_eq!(detector.observe_bgra(&black, 64, 64), Some(true));
        assert!(detector.is_protected());

        let mut visible = black.clone();
        for pixel in visible.chunks_exact_mut(4).step_by(2) {
            pixel[2] = 20;
        }
        assert_eq!(detector.observe_bgra(&visible, 64, 64), None);
        assert_eq!(detector.observe_bgra(&visible, 64, 64), None);
        assert_eq!(detector.observe_bgra(&visible, 64, 64), Some(false));
    }

    #[test]
    fn dark_but_nonblack_content_does_not_trigger() {
        let mut dark = vec![0, 0, 0, 255].repeat(64 * 64);
        for (index, pixel) in dark.chunks_exact_mut(4).enumerate() {
            if index % 64 == 0 {
                pixel[1] = 8;
            }
        }
        assert!(!looks_like_protected_black(&dark, 64, 64));
    }

    #[test]
    fn sequenced_panic_and_stop_reject_delayed_actions_but_allow_fresh_intent() {
        assert!(!action_is_newer_than_panic(8, 9));
        assert!(!action_is_newer_than_barriers(9, 9, 8, false));
        assert!(!action_is_newer_than_barriers(10, 8, 10, false));
        assert!(action_is_newer_than_barriers(11, 9, 10, false));
        assert!(!action_is_newer_than_barriers(12, 9, 10, true));
    }

    #[test]
    fn overlapping_power_and_lock_resume_only_after_both_clear() {
        let (power, first) = update_suspension_reasons(0, SUSPENSION_POWER, true);
        assert_eq!(first, SuspensionTransition::BecameSuspended);
        let (both, overlap) = update_suspension_reasons(power, SUSPENSION_SESSION_LOCK, true);
        assert_eq!(overlap, SuspensionTransition::Unchanged);
        let (locked, power_resume) = update_suspension_reasons(both, SUSPENSION_POWER, false);
        assert_eq!(power_resume, SuspensionTransition::Unchanged);
        let (clear, unlock) = update_suspension_reasons(locked, SUSPENSION_SESSION_LOCK, false);
        assert_eq!(unlock, SuspensionTransition::BecameResumable);
        assert_eq!(clear, 0);
        assert_eq!(RECOVERY_DELAYS_MS, [500, 1_000, 2_000, 4_000]);
        assert_eq!(FIRST_FRAME_TIMEOUT_MS, 3_000);
    }

    #[test]
    fn duplicate_suspension_messages_are_idempotent() {
        let (power, _) = update_suspension_reasons(0, SUSPENSION_POWER, true);
        let (same, transition) = update_suspension_reasons(power, SUSPENSION_POWER, true);
        assert_eq!(same, power);
        assert_eq!(transition, SuspensionTransition::Unchanged);
    }

    #[test]
    fn failure_gate_notifies_once_until_success() {
        let mut gate = ConsecutiveFailureGate::default();
        assert!(!gate.failure());
        assert!(!gate.failure());
        assert!(gate.failure());
        assert!(!gate.failure());
        gate.success();
        assert!(!gate.failure());
    }
}
