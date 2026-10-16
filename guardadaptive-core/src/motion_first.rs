//! Motion-first candidate: calibrated affine center subtraction plus an optional
//! sub-LSB soft neutral shrink. A research candidate, never a default driver.
//!
//! The model sees raw reports and trusted neutral calibration, NOT simulator
//! truth, 'neutral' gameplay labels, or future inputs.
use crate::{reacquire::VerifiedCenter, Config, GuardAdaptive, InputMode, OutputPolicy, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct MotionFirstConfig {
    /// Effective report quantum in normalized axis units. Zero is allowed
    /// when precision/transport resolution is unknown.
    pub input_lsb: f64,
    /// Shrinkage scale, in fractions of input_lsb.
    /// Nonzero values trade real micro movement for more neutral stability.
    pub softness_lsb: f64,
}
impl MotionFirstConfig {
    fn normalized(self) -> Self {
        Self {
            input_lsb: if self.input_lsb.is_finite() {
                self.input_lsb.clamp(0.0, 0.02)
            } else {
                0.0
            },
            softness_lsb: if self.softness_lsb.is_finite() {
                self.softness_lsb.clamp(0.0, 8.0)
            } else {
                0.0
            },
        }
    }
}
impl Default for MotionFirstConfig {
    fn default() -> Self {
        Self {
            input_lsb: 0.0,
            softness_lsb: 0.5,
        }
    }
}
pub struct MotionFirst {
    observer: GuardAdaptive,
    cfg: MotionFirstConfig,
    verified_center: Option<Vec2>,
}
impl MotionFirst {
    pub fn new(cfg: MotionFirstConfig) -> Self {
        // No learning during unknown gameplay. Prevent intentional held input
        // from ever teaching the center. Recalibrate only on trusted release.
        let observer_cfg = Config {
            auto_alpha: 0.0,
            ..Config::default()
        };
        Self {
            observer: GuardAdaptive::with_policy(observer_cfg, OutputPolicy::PreserveMicro),
            cfg: cfg.normalized(),
            verified_center: None,
        }
    }
    pub fn center(&self) -> Vec2 {
        self.verified_center
            .unwrap_or_else(|| self.observer.telemetry().center)
    }
    /// Only called after the explicit two-release calibration transaction has passed.
    pub fn accept_reacquisition(&mut self, verified: VerifiedCenter) {
        self.verified_center = Some(verified.center());
    }
    pub fn trained_samples(&self) -> u64 {
        self.observer.telemetry().neutral_calibration_samples
    }
    pub fn process(&mut self, raw: Vec2, mode: InputMode) -> Vec2 {
        // Do not allow NaNs to poison calibration state or output.
        if !raw.finite() {
            return Vec2::ZERO;
        }
        let raw = raw.clamp();
        // Once a verified profile is active, stray calibration-mode calls
        // must not wipe or relearn it. Replacement requires a completed
        // external two-release transaction via accept_reacquisition().
        let effective_mode = if self.verified_center.is_some() {
            InputMode::Live
        } else {
            mode
        };
        let _ = self.observer.process(raw, effective_mode);
        let centered = raw - self.center();
        let radius = centered.norm();
        if radius == 0.0 {
            return Vec2::ZERO;
        }
        let lambda = self.cfg.input_lsb * self.cfg.softness_lsb;
        // Continuous response; unlike a hard deadzone, doesn't set any
        // nonzero representable input to zero.
        let gain = if lambda == 0.0 {
            1.0
        } else {
            radius / (radius + lambda)
        };
        (centered * gain).saturate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn trained(lambda: f64) -> MotionFirst {
        let mut g = MotionFirst::new(MotionFirstConfig {
            input_lsb: 1.0 / 32767.0,
            softness_lsb: lambda,
        });
        for _ in 0..512 {
            g.process(Vec2::new(0.1, -0.03), InputMode::CalibrateNeutral);
        }
        g
    }
    #[test]
    fn preserves_small_intention_better_than_hard_deadzone() {
        let mut g = trained(0.5);
        let o = g.process(Vec2::new(0.1001, -0.03), InputMode::Live);
        assert!(o.x > 0.00008, "{:?}", o);
        assert!(o.y.abs() < 1e-8);
    }
    #[test]
    fn never_relearns_constant_held_input_during_live() {
        let mut g = trained(0.5);
        let center = g.center();
        for _ in 0..10000 {
            g.process(Vec2::new(0.4, -0.03), InputMode::Live);
        }
        assert_eq!(center, g.center());
    }
    #[test]
    fn smooth_at_zero_and_odd_symmetric() {
        let mut g = trained(0.5);
        for step in [0.0, 0.00001, 0.00003, 0.0001, 0.001, 0.1] {
            let right = g.process(Vec2::new(0.1 + step, -0.03), InputMode::Live);
            let left = g.process(Vec2::new(0.1 - step, -0.03), InputMode::Live);
            assert!((right.x + left.x).abs() < 1e-10);
            assert!(right.x >= 0.0 && left.x <= 0.0);
        }
    }
    #[test]
    fn trusted_calibration_updates_center_only_with_permission() {
        let mut g = trained(0.5);
        for _ in 0..500 {
            g.process(Vec2::new(-0.8, 0.2), InputMode::CalibrateNeutral);
        }
        assert!((g.center().x + 0.8).abs() < 1e-10);
        assert!(g.process(Vec2::new(-0.8, 0.2), InputMode::Live).norm() < 1e-10);
    }
    #[test]
    fn verified_center_survives_accidental_calibration_mode_input() {
        use crate::guided::GuidedWizard;
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(Vec2::new(0.55, -0.2)).unwrap();
        }
        w.sample(Vec2::new(0.3, -0.2)).unwrap();
        w.confirm_movement_finished().unwrap();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(Vec2::new(0.55, -0.2)).unwrap();
        }
        let verified = w.commit().unwrap();
        let mut g = MotionFirst::new(MotionFirstConfig::default());
        g.accept_reacquisition(verified);
        let center = g.center();
        for _ in 0..1000 {
            g.process(Vec2::new(-0.8, 0.2), InputMode::CalibrateNeutral);
        }
        assert_eq!(g.center(), center);
        assert!(g.process(center, InputMode::Live).norm() < 1e-12);
    }
    #[test]
    fn invalid_config_is_bounded() {
        let mut g = MotionFirst::new(MotionFirstConfig {
            input_lsb: f64::NAN,
            softness_lsb: f64::INFINITY,
        });
        for _ in 0..10 {
            g.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        assert!(g.process(Vec2::new(0.001, 0.), InputMode::Live).finite());
    }
}
