//! Experiment-only noise-calibrated correction with a *bounded* gain.
//! Stable trusted neutral samples may estimate center and sensor noise.
//! Neither the center nor noise estimate is updated from gameplay input.
//! This cannot infer player intention from an indistinguishable raw signal.
use crate::{reacquire::VerifiedCenter, InputMode, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct NoiseGuardConfig {
    /// Digitized axis quantum: optional when resolution is unknown.
    pub input_lsb: f64,
    /// Multiplier of trusted noise RMS applied as a radial soft knee.
    pub noise_weight: f64,
    /// Minimum fraction of centered observed magnitude that survives.
    /// A lower value improves quiet neutral, but distorts micro motion.
    pub min_gain: f64,
}
impl Default for NoiseGuardConfig {
    fn default() -> Self {
        Self {
            input_lsb: 1.0 / 32767.0,
            noise_weight: 1.0,
            min_gain: 0.55,
        }
    }
}
impl NoiseGuardConfig {
    fn sanitize(mut self) -> Self {
        self.input_lsb = if self.input_lsb.is_finite() {
            self.input_lsb.clamp(0.0, 0.02)
        } else {
            0.0
        };
        self.noise_weight = if self.noise_weight.is_finite() {
            self.noise_weight.clamp(0.0, 3.0)
        } else {
            0.0
        };
        self.min_gain = if self.min_gain.is_finite() {
            self.min_gain.clamp(0.5, 1.0)
        } else {
            1.0
        };
        self
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GuardStats {
    pub center: Vec2,
    pub neutral_noise_rms: f64,
    pub softness: f64,
    pub samples: u64,
    pub live: bool,
}
pub struct NoiseGuard {
    cfg: NoiseGuardConfig,
    center: Vec2,
    noise_sq: f64,
    samples: u64,
    live: bool,
}
impl NoiseGuard {
    pub fn new(cfg: NoiseGuardConfig) -> Self {
        Self {
            cfg: cfg.sanitize(),
            center: Vec2::ZERO,
            noise_sq: 0.0,
            samples: 0,
            live: false,
        }
    }
    /// Trusted operator release or simulator *pre-game* calibration only.
    /// After begin_live no input can alter either estimate.
    pub fn calibrate(&mut self, raw: Vec2) -> bool {
        if self.live || !raw.finite() || raw.x.abs() > 1.0 || raw.y.abs() > 1.0 {
            return false;
        }
        self.samples += 1;
        let alpha = if self.samples == 1 { 1.0 } else { 0.065 };
        self.center = self.center + (raw - self.center) * alpha;
        let residual = raw - self.center;
        let w = if self.samples == 1 { 1.0 } else { 0.025 };
        self.noise_sq =
            (self.noise_sq + (residual.length_squared() - self.noise_sq) * w).clamp(0.0, 1.0);
        true
    }
    pub fn accept_verified(&mut self, verified: VerifiedCenter) {
        self.center = verified.center();
        // MAD measures per-axis robust sigma. Convert to radial RMS.
        let mad = verified.neutral_mad;
        self.noise_sq = (mad.length_squared()).clamp(0.0, 1.0);
        self.samples = self.samples.max(1);
        self.live = true;
    }
    pub fn begin_live(&mut self) {
        self.live = true;
    }
    pub fn stats(&self) -> GuardStats {
        let noise_rms = self.noise_sq.sqrt();
        // No arbitrarily large estimated error may erase intention.
        let softness = (noise_rms * self.cfg.noise_weight)
            .max(self.cfg.input_lsb * 0.5)
            .min(0.015);
        GuardStats {
            center: self.center,
            neutral_noise_rms: noise_rms,
            softness,
            samples: self.samples,
            live: self.live,
        }
    }
    pub fn process(&self, raw: Vec2) -> Vec2 {
        if !raw.finite() {
            return Vec2::ZERO;
        }
        let centered = raw.clamp() - self.center;
        let radius = centered.norm();
        if radius == 0.0 {
            return Vec2::ZERO;
        }
        // Smooth, continuous radial gain; always at least min_gain.
        // This is a *filter policy* guarantee relative to the observable
        // centered vector, not to the unknowable intended movement.
        let softness = self.stats().softness;
        let gain = (radius / (radius + softness)).max(self.cfg.min_gain);
        (centered * gain).saturate()
    }
    pub fn process_mode(&mut self, raw: Vec2, mode: InputMode) -> Vec2 {
        if mode == InputMode::CalibrateNeutral {
            self.calibrate(raw);
        }
        self.process(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locked_model_never_learns_held_gameplay_input() {
        let mut g = NoiseGuard::new(NoiseGuardConfig::default());
        for i in 0..600 {
            let n = if i % 2 == 0 { 0.002 } else { -0.002 };
            assert!(g.calibrate(Vec2::new(0.15 + n, -0.08)));
        }
        g.begin_live();
        let before = g.stats();
        for _ in 0..100_000 {
            let out = g.process_mode(Vec2::new(-0.9, 0.5), InputMode::CalibrateNeutral);
            assert!(out.finite());
        }
        let after = g.stats();
        assert_eq!(before.center, after.center);
        assert_eq!(before.neutral_noise_rms, after.neutral_noise_rms);
        assert_eq!(before.samples, after.samples);
    }
    #[test]
    fn gain_is_bounded_above_half_of_observable_intention() {
        let mut g = NoiseGuard::new(NoiseGuardConfig {
            min_gain: 0.55,
            noise_weight: 3.0,
            input_lsb: 1.0 / 32767.0,
        });
        for i in 0..400 {
            let noisy = if i % 2 == 0 { 0.01 } else { -0.01 };
            g.calibrate(Vec2::new(noisy, -noisy));
        }
        g.begin_live();
        let c = g.stats().center;
        for displacement in [1e-5, 1e-4, 1e-3, 0.01, 0.1, 0.95] {
            let raw = c + Vec2::new(displacement, 0.0);
            let centered = (raw - c).norm();
            let out = g.process(raw);
            assert!(
                out.norm() + 1e-12 >= 0.55 * centered.min(1.0),
                "input {displacement} output={:?} centered={centered}",
                out
            );
        }
    }
    #[test]
    fn noise_estimation_depends_only_on_trusted_samples() {
        let mut stable = NoiseGuard::new(NoiseGuardConfig::default());
        let mut noisy = NoiseGuard::new(NoiseGuardConfig::default());
        for i in 0..1000 {
            stable.calibrate(Vec2::ZERO);
            let n = if i % 2 == 0 { 0.005 } else { -0.005 };
            noisy.calibrate(Vec2::new(n, -n));
        }
        stable.begin_live();
        noisy.begin_live();
        assert!(noisy.stats().softness > stable.stats().softness * 10.0);
        assert!(
            noisy.process(Vec2::new(0.001, 0.0)).norm()
                < stable.process(Vec2::new(0.001, 0.0)).norm()
        );
    }
    #[test]
    fn failed_calibration_and_nan_are_rejected() {
        let mut g = NoiseGuard::new(NoiseGuardConfig::default());
        assert!(!g.calibrate(Vec2::new(f64::NAN, 0.0)));
        assert!(!g.calibrate(Vec2::new(1.1, 0.0)));
        assert_eq!(g.stats().samples, 0);
        assert_eq!(g.process(Vec2::new(f64::INFINITY, 0.0)), Vec2::ZERO);
    }
    #[test]
    fn verified_two_release_profile_restricts_automatic_center_updates() {
        use crate::guided::GuidedWizard;
        let mut wizard = GuidedWizard::new();
        wizard.confirm_release().unwrap();
        for i in 0..128 {
            let noise = if i % 2 == 0 { 0.0004 } else { -0.0004 };
            wizard.sample(Vec2::new(0.4 + noise, -0.12)).unwrap();
        }
        wizard.sample(Vec2::new(0.2, -0.12)).unwrap();
        wizard.confirm_movement_finished().unwrap();
        wizard.confirm_release().unwrap();
        for i in 0..128 {
            let noise = if i % 2 == 0 { 0.0004 } else { -0.0004 };
            wizard.sample(Vec2::new(0.4 + noise, -0.12)).unwrap();
        }
        let verified = wizard.commit().unwrap();
        let mut filter = NoiseGuard::new(NoiseGuardConfig::default());
        filter.accept_verified(verified);
        assert!(filter.stats().live);
        assert!(filter.stats().neutral_noise_rms > 0.0);
        assert!((filter.stats().center - verified.center()).norm() < 1e-10);
        let start = filter.stats().center;
        for _ in 0..1000 {
            filter.process_mode(Vec2::new(0.95, 0.0), InputMode::CalibrateNeutral);
        }
        assert_eq!(filter.stats().center, start);
    }
    #[test]
    fn default_does_not_auto_mutate_model_from_sample() {
        let g = NoiseGuard::new(NoiseGuardConfig::default());
        let before = g.stats().center;
        for i in -500..500 {
            g.process(Vec2::new(i as f64 / 500.0, 0.0));
        }
        assert_eq!(g.stats().center, before);
    }
}
