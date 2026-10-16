//! v0.36 operator-confirmed neutral audit for the read-only localhost UI.
//! Human "released" confirmation is a declared premise, NOT sensor proof.
//! This module does not infer intent, learn a new center, or write to games.
use crate::{
    profile_store::Stick,
    recenter_bridge::{Audit, AUDIT_SAMPLES, MAX_AUDIT_MAD, MIN_CENTER_SHIFT},
    xinput::Report,
    Vec2,
};
use std::time::{Duration, Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Collecting,
    Stable,
    CenterDisagreement,
    Unstable,
    InvalidSample,
    StreamInterrupted,
    SessionChanged,
}
impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Self::Collecting => "collecting_operator_declared_neutral",
            Self::Stable => "stable_declared_neutral_no_center_change",
            Self::CenterDisagreement => "declared_neutral_center_disagreement_recalibrate",
            Self::Unstable => "declared_neutral_unstable_recalibrate",
            Self::InvalidSample => "invalid_report_preview_revoked",
            Self::StreamInterrupted => "read_gap_or_packet_reset_preview_revoked",
            Self::SessionChanged => "session_changed_preview_revoked",
        }
    }
}
pub struct Session {
    audit: Audit,
    reference: Vec2,
    slot: u32,
    stick: Stick,
    nonce: u64,
    previous: Option<(Instant, u32)>,
}
impl Session {
    pub fn new(
        reference: Vec2,
        slot: u32,
        stick: Stick,
        nonce: u64,
        confirmed_release: bool,
    ) -> Option<Self> {
        if !confirmed_release
            || slot > 3
            || !reference.finite()
            || reference.x.abs() > 1.0
            || reference.y.abs() > 1.0
        {
            return None;
        }
        Some(Self {
            audit: Audit::new(),
            reference,
            slot,
            stick,
            nonce,
            previous: None,
        })
    }
    pub fn remaining(&self) -> usize {
        AUDIT_SAMPLES - self.audit.count()
    }
    /// Read-only input; returns a terminal finding once 128 fresh poll samples
    /// have been observed. Identical XInput packet numbers are allowed because
    /// a stationary gamepad legitimately returns identical reports.
    pub fn sample(&mut self, report: Report, slot: u32, nonce: u64, when: Instant) -> Verdict {
        if slot != self.slot || nonce != self.nonce {
            return Verdict::SessionChanged;
        }
        if let Some((last, packet)) = self.previous {
            if when < last
                || when.duration_since(last) > Duration::from_millis(50)
                || report.packet < packet
            {
                return Verdict::StreamInterrupted;
            }
        }
        let raw = report.stick(self.stick);
        if [report.left.x, report.left.y, report.right.x, report.right.y]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1.0)
        {
            return Verdict::InvalidSample;
        }
        self.previous = Some((when, report.packet));
        if let Some((median, mad)) = self.audit.push(raw) {
            if mad.x > MAX_AUDIT_MAD || mad.y > MAX_AUDIT_MAD {
                Verdict::Unstable
            } else if (median - self.reference).norm() > MIN_CENTER_SHIFT {
                Verdict::CenterDisagreement
            } else {
                Verdict::Stable
            }
        } else {
            Verdict::Collecting
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn report(packet: u32, x: f64) -> Report {
        Report {
            packet,
            left: Vec2::new(x, -0.18),
            right: Vec2::ZERO,
            buttons: 0xA535,
            left_trigger: 65,
            right_trigger: 32,
        }
    }
    fn exercise(values: impl Fn(usize) -> f64) -> Verdict {
        let mut s = Session::new(Vec2::new(0.24, -0.18), 0, Stick::Left, 19, true).unwrap();
        let clock = Instant::now();
        let mut verdict = Verdict::Collecting;
        for i in 0..128 {
            verdict = s.sample(
                report((i / 2) as u32, values(i)),
                0,
                19,
                clock + Duration::from_millis(i as u64 * 8),
            );
            if i < 127 {
                assert_eq!(verdict, Verdict::Collecting);
            }
        }
        assert_eq!(s.remaining(), 0);
        verdict
    }
    #[test]
    fn stable_declared_release_preserves_center() {
        assert_eq!(
            exercise(|i| 0.24 + if i % 2 == 0 { 0.00003 } else { -0.00003 }),
            Verdict::Stable
        );
    }
    #[test]
    fn large_consistent_disagreement_suggests_recalibration() {
        assert_eq!(exercise(|_| 0.35), Verdict::CenterDisagreement);
    }
    #[test]
    fn unstable_or_falsely_declared_release_is_rejected() {
        assert_eq!(
            exercise(|i| if i % 2 == 0 { 0.20 } else { 0.36 }),
            Verdict::Unstable
        );
    }
    #[test]
    fn consent_required_and_missing_session_rejected() {
        assert!(Session::new(Vec2::ZERO, 0, Stick::Left, 1, false).is_none());
        assert!(Session::new(Vec2::new(f64::NAN, 0.0), 0, Stick::Left, 1, true).is_none());
        assert!(Session::new(Vec2::ZERO, 4, Stick::Left, 1, true).is_none());
    }
    #[test]
    fn connection_switch_and_poll_gap_invalidate_session() {
        let clock = Instant::now();
        let mut s = Session::new(Vec2::new(0.24, -0.18), 0, Stick::Left, 1, true).unwrap();
        assert_eq!(s.sample(report(10, 0.24), 0, 1, clock), Verdict::Collecting);
        assert_eq!(
            s.sample(report(9, 0.24), 0, 1, clock + Duration::from_millis(8)),
            Verdict::StreamInterrupted
        );
        assert_eq!(
            s.sample(report(11, 0.24), 0, 2, clock + Duration::from_millis(16)),
            Verdict::SessionChanged
        );
        let mut s = Session::new(Vec2::new(0.24, -0.18), 0, Stick::Left, 1, true).unwrap();
        s.sample(report(10, 0.24), 0, 1, clock);
        assert_eq!(
            s.sample(report(11, 0.24), 0, 1, clock + Duration::from_millis(51)),
            Verdict::StreamInterrupted
        );
    }
    #[test]
    fn other_stick_invalid_axes_also_revoke_audit() {
        let mut s = Session::new(Vec2::new(0.24, -0.18), 0, Stick::Left, 1, true).unwrap();
        let mut f = report(1, 0.24);
        f.right = Vec2::new(f64::INFINITY, 0.0);
        assert_eq!(s.sample(f, 0, 1, Instant::now()), Verdict::InvalidSample);
    }
    #[test]
    fn right_stick_can_be_audited_without_using_left_axis() {
        let mut s = Session::new(Vec2::new(0.10, -0.20), 0, Stick::Right, 1, true).unwrap();
        let clock = Instant::now();
        let mut verdict = Verdict::Collecting;
        for i in 0..128 {
            let f = Report {
                packet: (i / 2) as u32,
                left: Vec2::new(0.7, 0.6),
                right: Vec2::new(0.10, -0.20),
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
            };
            verdict = s.sample(f, 0, 1, clock + Duration::from_millis(i as u64 * 8));
        }
        assert_eq!(verdict, Verdict::Stable);
    }
    #[test]
    fn invalid_axes_are_not_used_for_center_estimation() {
        let mut s = Session::new(Vec2::new(0.24, -0.18), 0, Stick::Left, 1, true).unwrap();
        assert_eq!(
            s.sample(report(1, f64::NAN), 0, 1, Instant::now()),
            Verdict::InvalidSample
        );
        assert_eq!(s.remaining(), 128);
    }
}
