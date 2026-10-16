//! v0.28 experimental bounded causal noise stabilizer.
//! Correctness caveat: slow intentional movement and slow sensor drift can be
//! observationally identical; no algorithm using ONLY these reports can
//! distinguish them. This research candidate is NEVER auto-enabled.
//!
//! Effects are bounded relative to the same-frame centered passthrough.
//! No automatic zero estimation, no online center learning, no game writes.
use crate::{reacquire::VerifiedCenter, Vec2};
#[derive(Clone, Copy, Debug)]
pub struct GuardConfig {
    /// Maximum normalized change permitting one-sample smoothing.
    pub max_delta: f64,
    /// Maximum distance this candidate may deviate from centered passthrough.
    pub max_correction: f64,
    /// Temporal mixing only, 0 means pure raw centered.
    pub blend: f64,
}
impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_delta: 0.012,
            max_correction: 0.003,
            blend: 0.35,
        }
    }
}
impl GuardConfig {
    fn valid(self) -> bool {
        self.max_delta.is_finite()
            && self.max_correction.is_finite()
            && self.blend.is_finite()
            && (0.0..=0.02).contains(&self.max_delta)
            && (0.0..=0.005).contains(&self.max_correction)
            && (0.0..=0.5).contains(&self.blend)
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Metrics {
    pub bypassed_steps: u64,
    pub smoothed_steps: u64,
    pub maximum_change: f64,
}
pub struct CausalGuard {
    cfg: GuardConfig,
    center: Option<Vec2>,
    previous: Option<Vec2>,
    pub metrics: Metrics,
}
impl CausalGuard {
    pub fn new(cfg: GuardConfig) -> Self {
        Self {
            cfg,
            center: None,
            previous: None,
            metrics: Metrics::default(),
        }
    }
    /// Accepts a center acquired through the existing explicit two-release
    /// consent protocol, never through live gameplay.
    pub fn accept_verified(&mut self, v: VerifiedCenter) {
        if !self.cfg.valid() {
            self.center = None;
        } else {
            self.center = Some(v.center());
        }
        self.previous = None;
        self.metrics = Metrics::default();
    }
    pub fn invalidate(&mut self) {
        self.center = None;
        self.previous = None;
    }
    pub fn process(&mut self, raw: Vec2) -> Vec2 {
        if !raw.finite() {
            self.previous = None;
            return Vec2::ZERO;
        }
        let raw = raw.clamp();
        let Some(center) = self.center else {
            return raw;
        };
        let centered = (raw - center).saturate();
        let result = if let Some(prev) = self.previous {
            let step = (centered - prev).norm();
            // Flick and reversal passthrough. Do NOT infer user intent by
            // clamping or pushing the stick toward its last known location.
            if step > self.cfg.max_delta || self.cfg.blend == 0.0 {
                self.metrics.bypassed_steps += 1;
                centered
            } else {
                let proposed = prev * self.cfg.blend + centered * (1.0 - self.cfg.blend);
                let deviation = proposed - centered;
                let norm = deviation.norm();
                let shift = if norm > self.cfg.max_correction && norm > 0.0 {
                    deviation * (self.cfg.max_correction / norm)
                } else {
                    deviation
                };
                self.metrics.smoothed_steps += 1;
                self.metrics.maximum_change = self.metrics.maximum_change.max(shift.norm());
                (centered + shift).saturate()
            }
        } else {
            centered
        };
        // Store only observed input, not the previous output to avoid
        // hidden multi-step filtering delays or drift tracking.
        self.previous = Some(centered);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn calibrated() -> CausalGuard {
        let v = VerifiedCenter::restore_checked(
            Vec2::new(0.15, -0.2),
            Vec2::new(0.00001, 0.00001),
            0.00001,
        )
        .unwrap();
        let mut g = CausalGuard::new(GuardConfig::default());
        g.accept_verified(v);
        g
    }
    #[test]
    fn never_learns_center_from_gameplay() {
        let mut g = CausalGuard::new(GuardConfig::default());
        for _ in 0..5000 {
            assert_eq!(g.process(Vec2::new(0.15, -0.2)), Vec2::new(0.15, -0.2));
        }
        assert_eq!(g.metrics.smoothed_steps, 0);
    }
    #[test]
    fn correction_is_strictly_bounded() {
        let mut g = calibrated();
        for i in 0..3000 {
            let noise = if i % 2 == 0 { 0.002 } else { -0.002 };
            let raw = Vec2::new(0.15 + noise, -0.2 - noise);
            let v = g.process(raw);
            let center_only = (raw - Vec2::new(0.15, -0.2)).saturate();
            assert!((v - center_only).norm() <= 0.0030000001);
        }
        assert!(g.metrics.smoothed_steps > 0);
        assert!(g.metrics.maximum_change <= 0.0030000001);
    }
    #[test]
    fn large_flick_is_exact_current_frame_passthrough() {
        let mut g = calibrated();
        let start = Vec2::new(0.15, -0.2);
        let _ = g.process(start);
        assert!((g.process(Vec2::new(-0.35, 0.4)) - Vec2::new(-0.5, 0.6)).norm() < 1e-12);
        assert!((g.process(Vec2::new(0.35, -0.4)) - Vec2::new(0.2, -0.2)).norm() < 1e-12);
        assert!(g.metrics.bypassed_steps >= 2);
    }
    #[test]
    fn calibration_invalidation_forces_raw() {
        let mut g = calibrated();
        g.process(Vec2::new(0.15, -0.2));
        g.invalidate();
        assert_eq!(g.process(Vec2::new(0.2, -0.1)), Vec2::new(0.2, -0.1));
    }
    #[test]
    fn malformed_configuration_cannot_cause_correction() {
        let v = VerifiedCenter::restore_checked(Vec2::ZERO, Vec2::ZERO, 0.0).unwrap();
        let mut g = CausalGuard::new(GuardConfig {
            max_delta: f64::NAN,
            max_correction: 0.002,
            blend: 0.5,
        });
        g.accept_verified(v);
        assert_eq!(g.process(Vec2::new(0.2, 0.0)), Vec2::new(0.2, 0.0));
    }
}
