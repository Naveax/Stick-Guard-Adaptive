//! Research-only conservative hybrid: trusted center subtraction plus
//! a tightly bounded noise-aware attenuation. Never infer user intention,
//! adapt a center from gameplay, or route output to an actual controller.
//!
//! A measurement cannot distinguish stable stick intent from stable drift.
//! The guarantees here apply ONLY to a reported centered vector, not to
//! unknown physical intent when the input has clipped or quantized to zero.
use crate::{
    noise_guard::{NoiseGuard, NoiseGuardConfig},
    reacquire::VerifiedCenter,
    Vec2,
};

#[derive(Clone, Copy, Debug)]
pub struct HybridConfig {
    pub report_lsb: f64,
    /// Maximum amount the hybrid may *attenuate its own centered vector*.
    pub max_attenuation: f64,
    /// Multiplier of noise measured in trusted neutral calibration.
    pub noise_weight: f64,
}
impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            report_lsb: 1.0 / 32767.0,
            max_attenuation: 0.02,
            noise_weight: 1.0,
        }
    }
}
impl HybridConfig {
    fn normalized(self) -> Self {
        Self {
            report_lsb: if self.report_lsb.is_finite() {
                self.report_lsb.clamp(0.0, 0.02)
            } else {
                0.0
            },
            max_attenuation: if self.max_attenuation.is_finite() {
                self.max_attenuation.clamp(0.0, 0.02)
            } else {
                0.0
            },
            noise_weight: if self.noise_weight.is_finite() {
                self.noise_weight.clamp(0.0, 3.0)
            } else {
                0.0
            },
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct HybridTelemetry {
    pub trusted: bool,
    pub center: Vec2,
    pub noise_rms: f64,
    pub last_gain: f64,
    pub noise_attenuation_bypasses: u64,
    pub ambiguous_step_count: u64,
}
pub struct HybridGuard {
    estimator: NoiseGuard,
    cfg: HybridConfig,
    trusted: bool,
    live: bool,
    previous: Option<Vec2>,
    ambiguous_step_count: u64,
    noise_attenuation_bypasses: u64,
    last_gain: f64,
}
impl HybridGuard {
    pub fn new(config: HybridConfig) -> Self {
        let cfg = config.normalized();
        Self {
            estimator: NoiseGuard::new(NoiseGuardConfig {
                input_lsb: cfg.report_lsb,
                noise_weight: cfg.noise_weight,
                min_gain: 0.55,
            }),
            cfg,
            trusted: false,
            live: false,
            previous: None,
            ambiguous_step_count: 0,
            noise_attenuation_bypasses: 0,
            last_gain: 1.0,
        }
    }
    /// Authorized only for externally trusted pre-game neutral observations.
    pub fn calibrate_neutral(&mut self, sample: Vec2) -> bool {
        if self.live {
            return false;
        }
        // Isolated single reports cannot authenticate a neutral center.
        // Begin-live checks the minimum sample count and observed stability.
        self.estimator.calibrate(sample)
    }
    pub fn accept_verified(&mut self, verified: VerifiedCenter) {
        self.estimator.accept_verified(verified);
        self.trusted = true;
        self.live = true;
        self.previous = None;
    }
    pub fn begin_live(&mut self) {
        let s = self.estimator.stats();
        self.trusted = s.samples >= 128 && s.neutral_noise_rms <= 0.03 && s.center.finite();
        self.estimator.begin_live();
        self.live = true;
        self.previous = None;
    }
    pub fn telemetry(&self) -> HybridTelemetry {
        let stats = self.estimator.stats();
        HybridTelemetry {
            trusted: self.trusted,
            center: stats.center,
            noise_rms: stats.neutral_noise_rms,
            last_gain: self.last_gain,
            noise_attenuation_bypasses: self.noise_attenuation_bypasses,
            ambiguous_step_count: self.ambiguous_step_count,
        }
    }
    /// No simulator truth, future frame or "neutral" player label enters this function.
    /// Large steps can be a jump in drift OR an intentional flick. They are
    /// flagged for diagnostics and NEVER authorize learning or correction.
    pub fn process(&mut self, raw: Vec2) -> Vec2 {
        if !raw.finite() {
            return Vec2::ZERO;
        }
        let raw = raw.clamp();
        if let Some(previous) = self.previous {
            if (raw - previous).norm() > 0.25 {
                self.ambiguous_step_count += 1;
            }
        }
        self.previous = Some(raw);
        if !self.trusted {
            self.last_gain = 1.0;
            return raw;
        }
        let centered = raw - self.estimator.stats().center;
        let radius = centered.norm();
        if radius == 0.0 {
            self.last_gain = 1.0;
            return Vec2::ZERO;
        }
        // Sensor noise larger than 0.3% normalized radius makes tiny
        // intent-vs-noise changes unreliable. Suppress only *our own*
        // extra radial attenuation; never guess desired motion, learn from
        // gameplay, or replace the explicitly trusted center.
        // This is conservative centered passthrough, NOT raw passthrough.
        // The threshold is fixed before checking held-out scenario outcomes.
        if self.estimator.stats().neutral_noise_rms > 0.003 {
            self.last_gain = 1.0;
            self.noise_attenuation_bypasses += 1;
            return centered.saturate();
        }
        let noise = (self.estimator.stats().neutral_noise_rms * self.cfg.noise_weight)
            .max(self.cfg.report_lsb * 0.5)
            .min(0.02);
        // Gain in [0.98,1.0]. At large intentional displacement the
        // attenuation smoothly vanishes, preserving direction.
        let gain = 1.0 - self.cfg.max_attenuation * noise / (radius + noise);
        self.last_gain = gain;
        (centered * gain).saturate()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn known() -> HybridGuard {
        let mut g = HybridGuard::new(HybridConfig::default());
        for i in 0..400 {
            let jitter = if i % 2 == 0 { 0.00002 } else { -0.00002 };
            assert!(g.calibrate_neutral(Vec2::new(0.1 + jitter, -0.03)));
        }
        g.begin_live();
        g
    }
    #[test]
    fn unknown_center_is_raw_passthrough() {
        let mut g = HybridGuard::new(HybridConfig::default());
        for v in [Vec2::ZERO, Vec2::new(0.2, -0.3), Vec2::new(-1., 1.)] {
            assert_eq!(g.process(v), v);
        }
        assert!(!g.telemetry().trusted);
    }
    #[test]
    fn trusted_center_never_moves_during_gameplay() {
        let mut g = known();
        let t = g.telemetry();
        for _ in 0..100000 {
            let _ = g.process(Vec2::new(0.95, -0.6));
            assert!(!g.calibrate_neutral(Vec2::ZERO));
        }
        assert_eq!(g.telemetry().center, t.center);
        assert_eq!(g.telemetry().noise_rms, t.noise_rms);
    }
    #[test]
    fn attenuation_is_at_most_two_percent_relative_to_observed_center() {
        let mut g = known();
        let center = g.telemetry().center;
        for length in [0.00001, 0.0001, 0.0005, 0.001, 0.01, 0.1, 0.5] {
            let v = Vec2::new(length, 0.0);
            let out = g.process(center + v);
            let gain = out.x / v.x;
            assert!(
                (0.98 - 1e-8..=1.0 + 1e-8).contains(&gain),
                "gain={gain} input={length}"
            );
            assert!(out.y.abs() < 1e-10);
        }
    }
    #[test]
    fn large_changes_flag_ambiguity_without_modifying_center() {
        let mut g = known();
        let center = g.telemetry().center;
        g.process(Vec2::ZERO);
        g.process(Vec2::new(0.9, -0.8));
        assert_eq!(g.telemetry().ambiguous_step_count, 1);
        assert_eq!(g.telemetry().center, center);
    }
    #[test]
    fn never_invents_sub_lsb_information() {
        let mut g = HybridGuard::new(HybridConfig::default());
        g.calibrate_neutral(Vec2::ZERO);
        g.begin_live();
        assert_eq!(g.process(Vec2::ZERO), Vec2::ZERO);
    }
    #[test]
    fn inadequate_or_unstable_neutral_calibration_reverts_to_raw() {
        let mut too_short = HybridGuard::new(HybridConfig::default());
        for _ in 0..127 {
            too_short.calibrate_neutral(Vec2::new(0.4, 0.0));
        }
        too_short.begin_live();
        assert!(!too_short.telemetry().trusted);
        assert_eq!(
            too_short.process(Vec2::new(0.43, 0.0)),
            Vec2::new(0.43, 0.0)
        );
        let mut noisy = HybridGuard::new(HybridConfig::default());
        for i in 0..400 {
            noisy.calibrate_neutral(Vec2::new(if i % 2 == 0 { 0.7 } else { -0.7 }, 0.0));
        }
        noisy.begin_live();
        assert!(!noisy.telemetry().trusted);
        assert_eq!(noisy.process(Vec2::new(0.1, 0.0)), Vec2::new(0.1, 0.0));
    }
    #[test]
    fn high_trusted_noise_disables_extra_attenuation_but_locks_center() {
        let mut g = HybridGuard::new(HybridConfig::default());
        for i in 0..600 {
            let jitter = if i % 2 == 0 { 0.01 } else { -0.01 };
            g.calibrate_neutral(Vec2::new(0.15 + jitter, -0.05));
        }
        g.begin_live();
        assert!(
            g.telemetry().trusted,
            "noise is high but pre-game neutral quality is not catastrophically bad"
        );
        assert!(g.telemetry().noise_rms > 0.003);
        let center = g.telemetry().center;
        let raw = center + Vec2::new(0.005, 0.0);
        let out = g.process(raw);
        assert!((out.x - 0.005).abs() < 1e-10);
        assert!(out.y.abs() < 1e-10);
        assert_eq!(g.telemetry().noise_attenuation_bypasses, 1);
        assert_eq!(g.telemetry().center, center);
    }
    #[test]
    fn quiet_calibration_keeps_bounded_soft_attenuation() {
        let mut g = known();
        let center = g.telemetry().center;
        let out = g.process(center + Vec2::new(0.0001, 0.0));
        assert!(out.x < 0.0001 && out.x >= 0.000098);
        assert_eq!(g.telemetry().noise_attenuation_bypasses, 0);
    }
    #[test]
    fn invalid_config_is_fail_safe() {
        let mut g = HybridGuard::new(HybridConfig {
            report_lsb: f64::NAN,
            max_attenuation: f64::INFINITY,
            noise_weight: f64::NAN,
        });
        assert!(g.calibrate_neutral(Vec2::ZERO));
        g.begin_live();
        assert_eq!(g.process(Vec2::new(0.0001, 0.)), Vec2::new(0.0001, 0.));
    }
}
