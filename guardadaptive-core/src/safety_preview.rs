//! Guarded, read-only preview for manually approved profile snapshots.
//! A profile is not a reliable physical device identity. Uncertain calibration
//! must never cause a confident input correction (nor a driver/game write).
use crate::{
    motion_first::{MotionFirst, MotionFirstConfig},
    reacquire::VerifiedCenter,
    resolution::Resolution,
    InputMode, Vec2,
};

/// More stringent than profile storage bounds. These are conservative
/// *engineering policy thresholds*, not statistically proven sensor confidence.
const MAX_AXIS_MAD: f64 = 0.0015;
const MAX_BETWEEN_RELEASE_DISAGREEMENT: f64 = 0.0030;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewMode {
    Calibrated,
    RawPassthrough,
}
impl PreviewMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Calibrated => "calibrated_preview_only",
            Self::RawPassthrough => "uncertain_raw_passthrough",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct QualityReport {
    pub mode: PreviewMode,
    pub max_axis_mad: f64,
    pub release_disagreement: f64,
    /// An estimate, not a posterior probability and not a hardware identity.
    pub uncertainty_bound: f64,
}
impl QualityReport {
    pub fn from_verified(center: VerifiedCenter) -> Self {
        let mad = center.neutral_mad;
        let agreement = center.agreement;
        let max_axis_mad = mad.x.max(mad.y);
        // A simple, conservative coordinate bound. Not a claim about
        // true target intent or future thermal sensor drift.
        let uncertainty_bound = 2.0 * max_axis_mad + agreement;
        let mode = if max_axis_mad.is_finite()
            && agreement.is_finite()
            && max_axis_mad <= MAX_AXIS_MAD
            && agreement <= MAX_BETWEEN_RELEASE_DISAGREEMENT
        {
            PreviewMode::Calibrated
        } else {
            PreviewMode::RawPassthrough
        };
        Self {
            mode,
            max_axis_mad,
            release_disagreement: agreement,
            uncertainty_bound,
        }
    }
}

pub struct SafePreview {
    engine: Option<MotionFirst>,
    quality: QualityReport,
}
impl SafePreview {
    pub fn from_verified(v: VerifiedCenter) -> Self {
        let quality = QualityReport::from_verified(v);
        let engine = if quality.mode == PreviewMode::Calibrated {
            let mut engine = MotionFirst::new(MotionFirstConfig {
                input_lsb: Resolution::XInput16.positive_lsb(),
                softness_lsb: 0.5,
            });
            engine.accept_reacquisition(v);
            Some(engine)
        } else {
            None
        };
        Self { engine, quality }
    }
    pub fn quality(&self) -> QualityReport {
        self.quality
    }
    pub fn process(&mut self, raw: Vec2) -> Vec2 {
        // All untrusted signals remain read-only. No device writes occur.
        match &mut self.engine {
            Some(engine) => engine.process(raw, InputMode::Live),
            None => raw.clamp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reacquire::VerifiedCenter;
    fn checked(x: f64, y: f64, mad: f64, agreement: f64) -> VerifiedCenter {
        VerifiedCenter::restore_checked(Vec2::new(x, y), Vec2::new(mad, mad), agreement)
            .expect("test must create a storage-valid verified profile")
    }
    #[test]
    fn clean_two_release_profile_allows_preview_micro_motion() {
        let v = checked(0.55, -0.2, 0.0001, 0.0005);
        let mut g = SafePreview::from_verified(v);
        assert_eq!(g.quality().mode, PreviewMode::Calibrated);
        let x = g.process(Vec2::new(0.552, -0.2));
        assert!(x.x >= 0.0019 && x.x <= 0.002001);
    }
    #[test]
    fn noisy_storage_valid_profile_can_only_passthrough() {
        let v = checked(0.55, -0.2, 0.005, 0.001);
        let mut g = SafePreview::from_verified(v);
        assert_eq!(g.quality().mode, PreviewMode::RawPassthrough);
        let raw = Vec2::new(0.551, -0.2);
        assert_eq!(g.process(raw), raw);
        assert!(g.quality().uncertainty_bound > 0.01);
    }
    #[test]
    fn two_releases_that_disagree_force_raw_passthrough() {
        let mut g = SafePreview::from_verified(checked(0.8, -0.3, 0.00001, 0.01));
        assert_eq!(g.quality().mode, PreviewMode::RawPassthrough);
        let input = Vec2::new(0.801, -0.299);
        assert_eq!(g.process(input), input);
    }
    #[test]
    fn inclusive_policy_bounds_are_deterministic() {
        let a = checked(0.1, 0.0, MAX_AXIS_MAD, MAX_BETWEEN_RELEASE_DISAGREEMENT);
        let b = checked(
            0.1,
            0.0,
            MAX_AXIS_MAD + 0.00001,
            MAX_BETWEEN_RELEASE_DISAGREEMENT,
        );
        assert_eq!(
            QualityReport::from_verified(a).mode,
            PreviewMode::Calibrated
        );
        assert_eq!(
            QualityReport::from_verified(b).mode,
            PreviewMode::RawPassthrough
        );
    }
    #[test]
    fn raw_passthrough_is_not_relearned_by_repeated_gameplay() {
        let mut g = SafePreview::from_verified(checked(0.4, -0.2, 0.006, 0.009));
        for _ in 0..100_000 {
            assert_eq!(g.process(Vec2::new(0.55, -0.2)), Vec2::new(0.55, -0.2));
        }
        assert_eq!(g.quality().mode, PreviewMode::RawPassthrough);
    }
}
