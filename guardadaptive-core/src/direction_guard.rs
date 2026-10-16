//! Read-only research candidate: local temporal voting in an uncertainty cone.
//! This is *not* user-intent inference and never changes calibrated center.
//! A noisy observation has no uniquely recoverable direction.
use crate::{
    hybrid::{HybridConfig, HybridGuard},
    reacquire::VerifiedCenter,
    Vec2,
};
pub struct DirectionGuard {
    base: HybridGuard,
    previous: Option<Vec2>,
    blended_frames: u64,
}
impl DirectionGuard {
    pub fn new(cfg: HybridConfig) -> Self {
        Self {
            base: HybridGuard::new(cfg),
            previous: None,
            blended_frames: 0,
        }
    }
    pub fn calibrate_neutral(&mut self, sample: Vec2) -> bool {
        self.base.calibrate_neutral(sample)
    }
    pub fn accept_verified(&mut self, center: VerifiedCenter) {
        self.base.accept_verified(center);
        self.previous = None;
    }
    pub fn begin_live(&mut self) {
        self.base.begin_live();
        self.previous = None;
    }
    pub fn blended_frames(&self) -> u64 {
        self.blended_frames
    }
    /// Research-only angular smoothing. Both centered observations must be
    /// inside the trusted-noise corridor. Unlike v0.13, the output retains
    /// the EXACT magnitude of the current HybridGuard result (apart from
    /// negligible floating rounding), so the new filter cannot suppress a
    /// resolvable movement by shrinking its reported radius.
    ///
    /// At most one previous unsmoothed report is considered. Reject angular
    /// changes >30 degrees and never invert the current measured direction.
    /// These bounds do NOT reveal physical player intent during drift.
    pub fn process(&mut self, raw: Vec2) -> Vec2 {
        let current = self.base.process(raw);
        let stats = self.base.telemetry();
        // Never reinterpret unverified input: a partially sampled or
        // unreliable neutral window is not permission to smooth gameplay.
        let noise = if stats.trusted { stats.noise_rms } else { 0.0 };
        let radius = current.norm();
        let out = if noise > 0.003 && noise.is_finite() && radius > 0.0 && radius <= 3.0 * noise {
            if let Some(previous) = self.previous {
                if previous.norm() <= 3.0 * noise {
                    let proposed = current * 0.7 + previous * 0.3;
                    let proposal_norm = proposed.norm();
                    // Dot-product gate limits directional lag. Do not use
                    // the score's hidden desired signal to make decisions.
                    const MIN_COS: f64 = 0.866_025_403_784_438_6; // 30 deg
                    if proposal_norm > 0.0
                        && current.x * proposed.x + current.y * proposed.y
                            >= MIN_COS * radius * proposal_norm
                    {
                        self.blended_frames += 1;
                        // Preserve current radial amplitude, only rotate.
                        (proposed * (radius / proposal_norm)).saturate()
                    } else {
                        current
                    }
                } else {
                    current
                }
            } else {
                current
            }
        } else {
            current
        };
        // Crucially, do not stack smoothing across frames: a single 30%
        // interpolation of the last observed result, not a growing lag.
        self.previous = Some(current);
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prepared(noise: f64) -> DirectionGuard {
        let mut g = DirectionGuard::new(HybridConfig::default());
        for i in 0..400 {
            let n = if i % 2 == 0 { noise } else { -noise };
            assert!(g.calibrate_neutral(Vec2::new(n, -n)));
        }
        g.begin_live();
        g
    }
    #[test]
    fn quiet_calibration_is_exactly_hybrid() {
        let mut a = HybridGuard::new(HybridConfig::default());
        let mut b = DirectionGuard::new(HybridConfig::default());
        for i in 0..400 {
            let n = if i % 2 == 0 { 0.00001 } else { -0.00001 };
            let v = Vec2::new(n, -n);
            a.calibrate_neutral(v);
            b.calibrate_neutral(v);
        }
        a.begin_live();
        b.begin_live();
        for i in 0..2000 {
            let raw = Vec2::new(0.05 * (i as f64).sin(), -0.04 * (i as f64).cos());
            assert_eq!(a.process(raw), b.process(raw));
        }
        assert_eq!(b.blended_frames(), 0);
    }
    #[test]
    fn noise_corridor_blending_is_bounded() {
        let mut g = prepared(0.01);
        let first = Vec2::new(0.01, 0.001);
        let second = Vec2::new(0.009, 0.004);
        let _ = g.process(first);
        let direct = g.base.process(second);
        let rotated = g.process(second);
        assert!((direct.norm() - rotated.norm()).abs() < 1e-11);
        assert!(
            (direct - rotated).norm() > 1e-8,
            "angular blending should be active"
        );
        assert_eq!(g.blended_frames(), 1);
    }
    #[test]
    fn deliberate_large_flick_bypasses_temporal_blend() {
        let mut g = prepared(0.01);
        g.process(Vec2::new(0.0001, 0.0));
        let out = g.process(Vec2::new(0.7, -0.6));
        assert!((out - Vec2::new(0.7, -0.6)).norm() < 0.001);
        assert_eq!(g.blended_frames(), 0);
    }
    #[test]
    fn identical_reports_with_opposite_hidden_intentions_cannot_both_be_correct() {
        // Different physical histories can produce the same raw stream:
        // desired +0.01 with zero drift vs desired -0.01 with +0.02 drift.
        // No correction algorithm observing only raw may know which is true.
        let mut a = prepared(0.01);
        let mut b = prepared(0.01);
        let raw = Vec2::new(0.01, 0.0);
        for _ in 0..128 {
            let x = a.process(raw);
            let y = b.process(raw);
            assert_eq!(x, y);
            assert!(
                (x - Vec2::new(0.01, 0.0)).norm() > 0.005
                    || (y - Vec2::new(-0.01, 0.0)).norm() > 0.005
            );
        }
    }
    #[test]
    fn changing_direction_cannot_shrink_observed_radius() {
        let mut plain = HybridGuard::new(HybridConfig::default());
        let mut smooth = DirectionGuard::new(HybridConfig::default());
        for i in 0..400 {
            let s = if i % 2 == 0 { 0.01 } else { -0.01 };
            let v = Vec2::new(s, -s);
            plain.calibrate_neutral(v);
            smooth.calibrate_neutral(v);
        }
        plain.begin_live();
        smooth.begin_live();
        for i in 0..10_000 {
            let t = i as f64;
            let raw = Vec2::new(0.011 * (t * 1.27).sin(), 0.011 * (t * 0.83).cos());
            let baseline = plain.process(raw);
            let changed = smooth.process(raw);
            assert!(
                (baseline.norm() - changed.norm()).abs() < 1e-11,
                "current={baseline:?} proposed={changed:?}"
            );
            if baseline.norm() > 1e-10 && changed.norm() > 1e-10 {
                let cosine = (baseline.x * changed.x + baseline.y * changed.y)
                    / (baseline.norm() * changed.norm());
                assert!(
                    cosine >= 0.8660254 - 1e-9,
                    "angular change exceeds 30 degrees"
                );
            }
        }
        assert!(smooth.blended_frames() > 0, "exercise nontrivial rotation");
    }
    #[test]
    fn opposite_and_zero_directions_cannot_invent_motion() {
        let mut g = prepared(0.01);
        let center = g.base.telemetry().center;
        assert_eq!(g.process(center), Vec2::ZERO);
        let first = center + Vec2::new(0.012, 0.0);
        let second = center + Vec2::new(-0.0002, 0.0);
        let a = g.process(first);
        let b = g.process(second);
        assert!((a.norm() - 0.012).abs() < 1e-10);
        assert!((b.norm() - 0.0002).abs() < 1e-10);
        assert!(
            b.x < 0.0,
            "prior large sample must not invert a real reversal"
        );
        assert_eq!(g.process(center), Vec2::ZERO);
    }
    #[test]
    fn inadequate_calibration_forces_exact_raw_passthrough() {
        let mut g = DirectionGuard::new(HybridConfig::default());
        for i in 0..127 {
            let x = if i % 2 == 0 { 0.01 } else { -0.01 };
            assert!(g.calibrate_neutral(Vec2::new(x, -x)));
        }
        g.begin_live();
        for i in 0..3000 {
            let raw = Vec2::new(0.007 * (i as f64).sin(), 0.01 * (i as f64).cos());
            assert_eq!(g.process(raw), raw);
        }
        assert_eq!(g.blended_frames(), 0);
    }
    #[test]
    fn no_center_learning_from_gameplay_without_permission() {
        let mut g = prepared(0.01);
        for _ in 0..1000 {
            g.process(Vec2::new(0.85, -0.4));
        }
        assert_eq!(g.blended_frames(), 0);
    }
}
