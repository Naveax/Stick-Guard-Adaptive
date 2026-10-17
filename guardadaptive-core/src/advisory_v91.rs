//! v0.91: presentation-only rate limiting for ambiguous joystick observations.
//!
//! This module NEVER edits raw stick samples, centers, outputs or game devices.
//! An off-center stationary stick could be drift OR intentional held movement.
//! Therefore every notice deliberately says "ambiguous", not "drift detected".
use crate::unknown_drift_v89::{Alert, Observation};

/// Minimum spacing between displayed ambiguity notices in a single session.
pub const NOTICE_COOLDOWN_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    None,
    AmbiguousInput,
    Coalesced,
    InvalidSample,
    ClockFault,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoticeStats {
    pub displayed: u64,
    pub coalesced: u64,
    pub invalid_samples: u64,
    pub clock_faults: u64,
    pub explicit_resets: u64,
}

/// A read-only, allocation-free observer of *existing* drift ambiguity alerts.
/// Must receive the same monotonic timebase as UnknownDriftObserver::observe_at.
/// On a clock fault, only an explicit session reset may re-enable notices.
#[derive(Default)]
pub struct AdvisoryGate {
    last_seen_ms: Option<u64>,
    last_display_ms: Option<u64>,
    clock_faulted: bool,
    stats: NoticeStats,
}

impl AdvisoryGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> NoticeStats {
        self.stats
    }

    /// Call explicitly on controller/session replacement or a newly authorized
    /// calibration epoch. Never automatically call when drift looks different.
    pub fn reset_session(&mut self) {
        self.last_seen_ms = None;
        self.last_display_ms = None;
        self.clock_faulted = false;
        self.stats.explicit_resets = self.stats.explicit_resets.saturating_add(1);
    }

    /// Returns a presentation decision only. Does not return or alter stick data.
    pub fn review(&mut self, observation: &Observation, monotonic_ms: u64) -> Notice {
        if observation.alert == Alert::InvalidInput {
            self.stats.invalid_samples = self.stats.invalid_samples.saturating_add(1);
            return Notice::InvalidSample;
        }

        if observation.alert == Alert::InvalidTimestamp {
            self.clock_faulted = true;
            self.stats.clock_faults = self.stats.clock_faults.saturating_add(1);
            return Notice::ClockFault;
        }

        if self.clock_faulted {
            return Notice::ClockFault;
        }
        if self.last_seen_ms.is_some_and(|prev| monotonic_ms <= prev) {
            self.clock_faulted = true;
            self.stats.clock_faults = self.stats.clock_faults.saturating_add(1);
            return Notice::ClockFault;
        }
        self.last_seen_ms = Some(monotonic_ms);

        // No trusted reference means no calibrated drift comparison.
        // Conflicting or repeated flags cannot forge additional display events.
        if observation.active_center.is_none()
            || observation.alert != Alert::MotionOrDriftAmbiguous
            || !observation.ambiguity_started
        {
            return Notice::None;
        }

        if self
            .last_display_ms
            .is_some_and(|prev| monotonic_ms.saturating_sub(prev) < NOTICE_COOLDOWN_MS)
        {
            self.stats.coalesced = self.stats.coalesced.saturating_add(1);
            return Notice::Coalesced;
        }

        self.last_display_ms = Some(monotonic_ms);
        self.stats.displayed = self.stats.displayed.saturating_add(1);
        Notice::AmbiguousInput
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guided::GuidedWizard;
    use crate::resolution::Resolution;
    use crate::unknown_drift_v89::{Alert as ObserverAlert, UnknownDriftObserver};
    use crate::Vec2;

    fn frame(alert: Alert, started: bool, center: Option<Vec2>) -> Observation {
        Observation {
            output: Vec2::new(0.08, -0.02),
            alert,
            ambiguity_started: started,
            suspected_center: if started {
                Some(Vec2::new(0.08, -0.02))
            } else {
                None
            },
            active_center: center,
        }
    }

    fn calibrate_zero(core: &mut UnknownDriftObserver) {
        let mut wizard = GuidedWizard::new();
        wizard.confirm_release().unwrap();
        for i in 0_u32..128 {
            let n = if i.is_multiple_of(2) {
                0.00002
            } else {
                -0.00002
            };
            wizard
                .sample(Resolution::XInput16.quantize(Vec2::new(n, -n)))
                .unwrap();
        }
        wizard.sample(Vec2::new(0.18, 0.0)).unwrap();
        wizard.confirm_movement_finished().unwrap();
        wizard.confirm_release().unwrap();
        for i in 0_u32..128 {
            let n = if i.is_multiple_of(2) {
                0.00002
            } else {
                -0.00002
            };
            wizard
                .sample(Resolution::XInput16.quantize(Vec2::new(n, -n)))
                .unwrap();
        }
        core.accept_reacquisition(wizard.commit().unwrap()).unwrap();
    }

    #[test]
    fn cooldown_is_inclusive_at_ten_seconds_and_does_not_reset_on_coalesced_events() {
        let mut presenter = AdvisoryGate::new();
        let event = frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO));
        assert_eq!(presenter.review(&event, 0), Notice::AmbiguousInput);
        assert_eq!(presenter.review(&event, 9_999), Notice::Coalesced);
        assert_eq!(presenter.review(&event, 10_000), Notice::AmbiguousInput);
        assert_eq!(presenter.stats().displayed, 2);
        assert_eq!(presenter.stats().coalesced, 1);
    }

    #[test]
    fn repeated_plateau_frames_never_repeat_a_notice() {
        let mut p = AdvisoryGate::new();
        let first = frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO));
        assert_eq!(p.review(&first, 1_000), Notice::AmbiguousInput);
        for i in 1_u64..50_000 {
            let next = frame(Alert::MotionOrDriftAmbiguous, false, Some(Vec2::ZERO));
            assert_eq!(p.review(&next, 1_000 + i), Notice::None);
        }
        assert_eq!(p.stats().displayed, 1);
        assert_eq!(p.stats().coalesced, 0);
    }

    #[test]
    fn no_center_and_conflicting_flags_do_not_create_false_notice() {
        let mut p = AdvisoryGate::new();
        assert_eq!(
            p.review(&frame(Alert::MotionOrDriftAmbiguous, true, None), 100),
            Notice::None
        );
        assert_eq!(
            p.review(&frame(Alert::Quiet, true, Some(Vec2::ZERO)), 101),
            Notice::None
        );
        assert_eq!(
            p.review(
                &frame(Alert::MotionOrDriftAmbiguous, false, Some(Vec2::ZERO)),
                102
            ),
            Notice::None
        );
        assert_eq!(p.stats().displayed, 0);
    }

    #[test]
    fn malformed_samples_cannot_bypass_cooldown() {
        let mut p = AdvisoryGate::new();
        let event = frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO));
        assert_eq!(p.review(&event, 500), Notice::AmbiguousInput);
        assert_eq!(
            p.review(&frame(Alert::InvalidInput, false, Some(Vec2::ZERO)), 500),
            Notice::InvalidSample
        );
        assert_eq!(p.review(&event, 5_000), Notice::Coalesced);
        assert_eq!(p.stats().invalid_samples, 1);
        assert_eq!(p.stats().coalesced, 1);
        assert_eq!(p.stats().displayed, 1);
    }

    #[test]
    fn clock_replay_fails_closed_until_explicit_reset() {
        let mut p = AdvisoryGate::new();
        let event = frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO));
        assert_eq!(p.review(&event, 1_000), Notice::AmbiguousInput);
        assert_eq!(p.review(&event, 1_000), Notice::ClockFault);
        assert_eq!(p.review(&event, 20_000), Notice::ClockFault);
        assert_eq!(p.stats().displayed, 1);
        assert_eq!(p.stats().clock_faults, 1);
        p.reset_session();
        assert_eq!(p.review(&event, 0), Notice::AmbiguousInput);
        assert_eq!(p.stats().displayed, 2);
        assert_eq!(p.stats().explicit_resets, 1);
    }

    #[test]
    fn observation_clock_fault_also_fails_closed() {
        let mut p = AdvisoryGate::new();
        assert_eq!(
            p.review(&frame(Alert::InvalidTimestamp, false, Some(Vec2::ZERO)), 77),
            Notice::ClockFault
        );
        assert_eq!(
            p.review(
                &frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO)),
                900
            ),
            Notice::ClockFault
        );
        assert_eq!(p.stats().clock_faults, 1);
    }

    #[test]
    fn a_changed_reference_alone_does_not_bypass_cooldown() {
        let mut p = AdvisoryGate::new();
        assert_eq!(
            p.review(
                &frame(Alert::MotionOrDriftAmbiguous, true, Some(Vec2::ZERO)),
                1_000
            ),
            Notice::AmbiguousInput
        );
        assert_eq!(
            p.review(
                &frame(
                    Alert::MotionOrDriftAmbiguous,
                    true,
                    Some(Vec2::new(0.02, 0.0))
                ),
                1_500
            ),
            Notice::Coalesced
        );
        // The caller can explicitly start a fresh authorized epoch.
        p.reset_session();
        assert_eq!(
            p.review(
                &frame(
                    Alert::MotionOrDriftAmbiguous,
                    true,
                    Some(Vec2::new(0.02, 0.0))
                ),
                0
            ),
            Notice::AmbiguousInput
        );
    }

    #[test]
    fn same_stick_signal_yields_same_notice_for_held_intent_or_drift() {
        let mut decisions = Vec::new();
        for _meaning in [false, true] {
            let mut core = UnknownDriftObserver::new();
            calibrate_zero(&mut core);
            let mut p = AdvisoryGate::new();
            let mut first_alert = None;
            for step in 0_u64..500 {
                let raw = Vec2::new(0.10, 0.0); // identical for both possible truths
                let observed = core.observe_at(raw, step * 4);
                let result = p.review(&observed, step * 4);
                assert!((observed.output - raw).norm() < 0.0001);
                if result == Notice::AmbiguousInput {
                    first_alert = Some(step * 4);
                }
                assert_ne!(observed.alert, ObserverAlert::InvalidInput);
            }
            assert_eq!(p.stats().displayed, 1);
            assert_eq!(core.metrics().unverified_center_updates, 0);
            decisions.push(first_alert);
        }
        assert_eq!(decisions[0], decisions[1]);
        assert!(decisions[0].unwrap() >= 800);
    }
}
