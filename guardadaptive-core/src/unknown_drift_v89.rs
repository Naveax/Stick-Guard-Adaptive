//! v0.89: conservative observer for *unknown or changing* stick drift.
//!
//! A quiet nonzero plateau can be drift OR a genuine held direction. Such a
//! plateau is only an *ambiguity alert*, never an automatic neutral label.
//! This experimental module is not connected to a gamepad driver or web output.
use crate::{reacquire::VerifiedCenter, Vec2};

const STABLE_DELTA_MAX: f64 = 0.0012;
const STABLE_RADIUS_MAX: f64 = 0.003;
const ALERT_ABOVE_CENTER: f64 = 0.012;
const STABLE_FRAMES_FOR_ALERT: u32 = 128; // offline fallback without timebase
const TIMED_STABLE_MS: u64 = 800;
const TIMED_MIN_FRAMES: u32 = 24;
const MAX_GAP_MS: u64 = 250;
const MAX_REACQUISITION_CHANGE: f64 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alert {
    Uncalibrated,
    Quiet,
    MotionOrDriftAmbiguous,
    InvalidInput,
    InvalidTimestamp,
}

#[derive(Clone, Copy, Debug)]
pub struct Observation {
    /// Raw passthrough until a reference exists; then direct center subtraction.
    pub output: Vec2,
    pub alert: Alert,
    /// Unauthenticated hypothesis, NEVER applied to live output.
    pub suspected_center: Option<Vec2>,
    pub active_center: Option<Vec2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterError {
    InvalidCenter,
    ShiftAboveLimit,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnknownDriftMetrics {
    pub ambiguous_stable_reports: u64,
    pub reference_updates: u64,
    pub unverified_center_updates: u64,
    pub restored_old_reference: u64,
    pub invalid_samples: u64,
    pub timestamp_anomalies: u64,
}

#[derive(Default)]
pub struct UnknownDriftObserver {
    center: Option<Vec2>,
    rollback_center: Option<Vec2>,
    rollback_available: bool,
    previous_raw: Option<Vec2>,
    plateau_anchor: Option<Vec2>,
    plateau_frames: u32,
    plateau_start_ms: Option<u64>,
    last_timestamp_ms: Option<u64>,
    metrics: UnknownDriftMetrics,
}

impl UnknownDriftObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn center(&self) -> Option<Vec2> {
        self.center
    }

    pub fn metrics(&self) -> UnknownDriftMetrics {
        self.metrics
    }

    fn clear_plateau(&mut self) {
        self.previous_raw = None;
        self.plateau_anchor = None;
        self.plateau_frames = 0;
        self.plateau_start_ms = None;
    }

    /// A reference must originate from an explicit two-release transaction.
    /// The token alone does NOT independently prove the stick was released.
    /// The shift bound prevents an unexpectedly large, silent remap.
    pub fn accept_reacquisition(&mut self, reference: VerifiedCenter) -> Result<(), CenterError> {
        let next = reference.center();
        if !next.finite() || next.x.abs() > 1.0 || next.y.abs() > 1.0 {
            return Err(CenterError::InvalidCenter);
        }
        if self
            .center
            .is_some_and(|current| (next - current).norm() > MAX_REACQUISITION_CHANGE)
        {
            return Err(CenterError::ShiftAboveLimit);
        }
        self.rollback_center = self.center;
        self.rollback_available = true;
        self.center = Some(next);
        self.metrics.reference_updates = self.metrics.reference_updates.saturating_add(1);
        self.clear_plateau();
        Ok(())
    }

    /// Explicit user-requested rollback, never an inferred correction.
    pub fn restore_previous_reference(&mut self) -> bool {
        if !self.rollback_available {
            return false;
        }
        self.center = self.rollback_center.take();
        self.rollback_available = false;
        self.metrics.restored_old_reference = self.metrics.restored_old_reference.saturating_add(1);
        self.clear_plateau();
        true
    }

    /// Forget the reference when user/device/session identity changes.
    pub fn invalidate(&mut self) {
        self.center = None;
        self.rollback_center = None;
        self.rollback_available = false;
        self.clear_plateau();
        self.last_timestamp_ms = None;
    }

    /// Constant-work, allocation-free observation. Never adjusts the active center.
    /// No temporal smoothing, extra gaming frames, or hidden deadzone.
    pub fn observe(&mut self, raw: Vec2) -> Observation {
        self.observe_internal(raw, None)
    }

    /// Time-based detector: candidate requires >=800 ms stable *and* >=24 reports.
    /// A pause, duplicate/reversed clock or network gap resets detection.
    /// Never implies user release; output remains same-frame passthrough/center-only.
    pub fn observe_at(&mut self, raw: Vec2, monotonic_ms: u64) -> Observation {
        self.observe_internal(raw, Some(monotonic_ms))
    }

    fn observe_internal(&mut self, raw: Vec2, now: Option<u64>) -> Observation {
        if let Some(ms) = now {
            if let Some(prev) = self.last_timestamp_ms {
                if ms <= prev {
                    self.clear_plateau();
                    self.metrics.timestamp_anomalies =
                        self.metrics.timestamp_anomalies.saturating_add(1);
                    return Observation {
                        output: match self.center {
                            Some(center) if raw.finite() => (raw.clamp() - center).saturate(),
                            _ if raw.finite() => raw.clamp(),
                            _ => Vec2::ZERO,
                        },
                        alert: Alert::InvalidTimestamp,
                        suspected_center: None,
                        active_center: self.center,
                    };
                }
                if ms - prev > MAX_GAP_MS {
                    self.clear_plateau();
                }
            }
            self.last_timestamp_ms = Some(ms);
        } else {
            self.last_timestamp_ms = None;
            self.plateau_start_ms = None;
        }
        if !raw.finite() || raw.x.abs() > 1.0 || raw.y.abs() > 1.0 {
            self.clear_plateau();
            self.metrics.invalid_samples = self.metrics.invalid_samples.saturating_add(1);
            return Observation {
                output: Vec2::ZERO,
                alert: Alert::InvalidInput,
                suspected_center: None,
                active_center: self.center,
            };
        }

        let stable = self
            .previous_raw
            .is_some_and(|p| (raw - p).norm() <= STABLE_DELTA_MAX)
            && self
                .plateau_anchor
                .is_some_and(|anchor| (raw - anchor).norm() <= STABLE_RADIUS_MAX);

        if stable {
            self.plateau_frames = self.plateau_frames.saturating_add(1);
        } else {
            self.plateau_anchor = Some(raw);
            self.plateau_frames = 1;
            self.plateau_start_ms = now;
        }
        self.previous_raw = Some(raw);

        let reference = self.center.unwrap_or(Vec2::ZERO);
        let measured_deviation = (raw - reference).norm();
        let stable_enough = match now {
            Some(ms) => {
                self.plateau_frames >= TIMED_MIN_FRAMES
                    && self
                        .plateau_start_ms
                        .is_some_and(|since| ms.saturating_sub(since) >= TIMED_STABLE_MS)
            }
            None => self.plateau_frames >= STABLE_FRAMES_FOR_ALERT,
        };
        let ambiguous = stable_enough && measured_deviation >= ALERT_ABOVE_CENTER;
        if ambiguous {
            self.metrics.ambiguous_stable_reports =
                self.metrics.ambiguous_stable_reports.saturating_add(1);
        }
        Observation {
            output: match self.center {
                Some(center) => (raw - center).saturate(),
                None => raw,
            },
            alert: if ambiguous {
                Alert::MotionOrDriftAmbiguous
            } else if self.center.is_none() {
                Alert::Uncalibrated
            } else {
                Alert::Quiet
            },
            suspected_center: if ambiguous { Some(raw) } else { None },
            active_center: self.center,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guided::GuidedWizard;
    use crate::resolution::Resolution;

    fn reference(center: Vec2) -> VerifiedCenter {
        // Synthetic oracle: only the test generator knows neutral is truly zero.
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        for i in 0..128 {
            let d = if i % 2 == 0 { 0.00002 } else { -0.00002 };
            w.sample(Resolution::XInput16.quantize(center + Vec2::new(d, -d)))
                .unwrap();
        }
        w.sample((center + Vec2::new(0.18, 0.0)).clamp()).unwrap();
        w.confirm_movement_finished().unwrap();
        w.confirm_release().unwrap();
        for i in 0..128 {
            let d = if i % 2 == 0 { 0.00002 } else { -0.00002 };
            w.sample(Resolution::XInput16.quantize(center + Vec2::new(d, -d)))
                .unwrap();
        }
        w.commit().unwrap()
    }

    #[test]
    fn unknown_signed_static_grid_corrects_without_a_deadzone_after_reference() {
        let micro = [
            Vec2::new(0.0002, 0.0),
            Vec2::new(-0.0004, 0.0001),
            Vec2::new(0.002, -0.001),
            Vec2::new(-0.008, 0.005),
            Vec2::new(0.01, -0.012),
            Vec2::new(0.03, 0.02),
        ];
        let mut count = 0;
        let mut max_neutral: f64 = 0.0;
        let mut max_micro_error: f64 = 0.0;
        for xx in -10..=10 {
            for yy in -10..=10 {
                let bias = Vec2::new(xx as f64 * 0.01, yy as f64 * 0.01);
                let mut g = UnknownDriftObserver::new();
                let before = g.observe(bias);
                assert_eq!(before.alert, Alert::Uncalibrated);
                assert_eq!(before.output, bias);
                g.accept_reacquisition(reference(bias)).unwrap();
                let neutral = g.observe(bias);
                max_neutral = max_neutral.max(neutral.output.norm());
                for motion in micro {
                    let raw = Resolution::XInput16.quantize((bias + motion).clamp());
                    let result = g.observe(raw);
                    assert!(result.output.norm() > 0.000001, "micro erased");
                    max_micro_error = max_micro_error.max((result.output - motion).norm());
                }
                assert_eq!(g.metrics().unverified_center_updates, 0);
                count += 1;
            }
        }
        println!(
            "V89_STATIC_SYNTHETIC cases={count} max_neutral_pct={:.5} max_micro_error_pct={:.5}",
            max_neutral * 100.0,
            max_micro_error * 100.0
        );
        assert_eq!(count, 441);
        assert!(max_neutral < 0.009);
        assert!(max_micro_error < 0.002);
    }

    #[test]
    fn sudden_unknown_drift_is_flagged_not_learned_or_falsely_fixed() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
        let jump = Vec2::new(0.10, -0.10);
        for i in 0..200 {
            let o = g.observe(jump);
            if i < STABLE_FRAMES_FOR_ALERT as usize - 1 {
                assert_ne!(o.alert, Alert::MotionOrDriftAmbiguous);
            }
        }
        let observed = g.observe(jump);
        assert_eq!(observed.alert, Alert::MotionOrDriftAmbiguous);
        assert_eq!(observed.suspected_center, Some(jump));
        assert!(observed.output.norm() > 0.009); // unverified jump UNSOLVED
        assert!(g.center().unwrap().norm() < 0.0001);
        assert_eq!(g.metrics().unverified_center_updates, 0);
        // A separate, externally acknowledged neutral reacquisition now works.
        g.accept_reacquisition(reference(jump)).unwrap();
        assert!(g.observe(jump).output.norm() < 0.009);
        assert!(g.restore_previous_reference());
        assert!(g.observe(jump).output.norm() > 0.009);
    }

    #[test]
    fn still_intentional_held_motion_may_trigger_alert_but_cannot_be_erased() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
        let held = Vec2::new(0.05, 0.03);
        for _ in 0..1_000 {
            let out = g.observe(held);
            assert!((out.output - held).norm() < 0.0001);
        }
        assert_eq!(g.observe(held).alert, Alert::MotionOrDriftAmbiguous);
        assert_eq!(g.metrics().reference_updates, 1);
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }

    #[test]
    fn small_held_micro_motion_is_never_suppressed_or_trained_away() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::new(0.02, 0.01)))
            .unwrap();
        let intention = Vec2::new(0.003, -0.002);
        for _ in 0..10_000 {
            let out = g.observe(Vec2::new(0.02, 0.01) + intention);
            assert!((out.output - intention).norm() < 0.0001);
            assert_ne!(out.alert, Alert::MotionOrDriftAmbiguous);
        }
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }

    #[test]
    fn reject_large_center_shift_without_erasing_active_mapping() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
        let old = g.center();
        assert_eq!(
            g.accept_reacquisition(reference(Vec2::new(0.20, 0.0))),
            Err(CenterError::ShiftAboveLimit)
        );
        assert_eq!(g.center(), old);
        assert_eq!(g.metrics().reference_updates, 1);
        // Rejected re-acquisition never replaces the saved undo of first calibration.
        assert!(g.restore_previous_reference());
        assert_eq!(g.center(), None);
        assert!(!g.restore_previous_reference());
    }

    #[test]
    fn jitter_and_fast_reversals_do_not_self_calibrate() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::new(-0.05, 0.04)))
            .unwrap();
        for i in 0..10_000 {
            let motion = if (i / 7) % 2 == 0 { 0.08 } else { -0.08 };
            let raw = Vec2::new(-0.05 + motion, 0.04);
            let result = g.observe(raw);
            assert!((result.output.x - motion).abs() < 0.0001);
        }
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }

    #[test]
    fn invalid_axes_fail_closed_and_device_invalidation_falls_back_to_passthrough() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::new(0.02, 0.0)))
            .unwrap();
        let invalid = g.observe(Vec2::new(f64::INFINITY, 0.0));
        assert_eq!(invalid.alert, Alert::InvalidInput);
        assert_eq!(invalid.output, Vec2::ZERO);
        assert_eq!(g.metrics().invalid_samples, 1);
        g.invalidate();
        let raw = Vec2::new(0.015, -0.005);
        let result = g.observe(raw);
        assert_eq!(result.alert, Alert::Uncalibrated);
        assert_eq!(result.output, raw);
    }

    #[test]
    fn timed_detector_flags_ambiguous_plateaus_at_both_60hz_and_250hz() {
        for interval_ms in [16_u64, 4_u64] {
            let mut g = UnknownDriftObserver::new();
            g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
            let held_or_drift = Vec2::new(0.09, -0.03);
            let mut saw_alert = false;
            for i in 0..320_u64 {
                let now = i * interval_ms;
                let observed = g.observe_at(held_or_drift, now);
                assert!((observed.output - held_or_drift).norm() < 0.0000001);
                if observed.alert == Alert::MotionOrDriftAmbiguous {
                    assert!(now >= TIMED_STABLE_MS);
                    saw_alert = true;
                    break;
                }
            }
            assert!(saw_alert, "No alert for {interval_ms} ms intervals");
            assert_eq!(g.metrics().unverified_center_updates, 0);
        }
    }

    #[test]
    fn time_anomalies_and_long_gaps_reset_hypotheses_not_control() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
        let plateau = Vec2::new(0.1, 0.0);
        for i in 0..70_u64 {
            g.observe_at(plateau, i * 16);
        }
        assert_eq!(
            g.observe_at(plateau, 70 * 16).alert,
            Alert::MotionOrDriftAmbiguous
        );
        let clock_error = g.observe_at(plateau, 5);
        assert_eq!(clock_error.alert, Alert::InvalidTimestamp);
        assert!((clock_error.output - plateau).norm() < 0.0000001);
        assert_eq!(g.metrics().timestamp_anomalies, 1);
        let after_large_gap = g.observe_at(plateau, 5000);
        assert_ne!(after_large_gap.alert, Alert::MotionOrDriftAmbiguous);
        for i in 1..70_u64 {
            g.observe_at(plateau, 5000 + i * 16);
        }
        assert_eq!(
            g.observe_at(plateau, 5000 + 70 * 16).alert,
            Alert::MotionOrDriftAmbiguous
        );
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }

    #[test]
    fn gradual_unknown_drift_suspected_only_after_new_plateau() {
        let mut g = UnknownDriftObserver::new();
        g.accept_reacquisition(reference(Vec2::ZERO)).unwrap();
        let mut last = Vec2::ZERO;
        for i in 0..1_001_u64 {
            last = Vec2::new(i as f64 * 0.0001, 0.0);
            let o = g.observe_at(last, i * 5);
            assert_ne!(o.alert, Alert::MotionOrDriftAmbiguous);
            assert!(g.center().unwrap().norm() < 0.0000001);
        }
        let mut observed = false;
        for i in 1..250_u64 {
            let o = g.observe_at(last, 5000 + i * 5);
            if o.alert == Alert::MotionOrDriftAmbiguous {
                observed = true;
                assert!(o.output.norm() >= 0.099);
                break;
            }
        }
        assert!(observed);
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }

    #[test]
    fn first_false_calibration_can_be_undone_to_exact_raw_passthrough() {
        let mut g = UnknownDriftObserver::new();
        let deliberate = Vec2::new(0.03, -0.02);
        g.accept_reacquisition(reference(Vec2::new(0.03, -0.02)))
            .unwrap();
        assert!(g.observe(deliberate).output.norm() < 0.0001);
        assert!(g.restore_previous_reference());
        let restored = g.observe(deliberate);
        assert_eq!(restored.alert, Alert::Uncalibrated);
        assert_eq!(restored.output, deliberate);
        assert_eq!(g.center(), None);
        assert!(!g.restore_previous_reference());
        assert_eq!(g.metrics().restored_old_reference, 1);
    }

    #[test]
    fn falsely_confirmed_neutral_can_still_erase_true_intent() {
        let mut g = UnknownDriftObserver::new();
        // In reality the controller may be genuinely neutral at zero, and
        // the operator held 5% movement while falsely confirming release.
        g.accept_reacquisition(reference(Vec2::new(0.05, 0.0)))
            .unwrap();
        assert!(g.observe(Vec2::ZERO).output.norm() > 0.009);
        assert_eq!(g.metrics().unverified_center_updates, 0);
    }
}
