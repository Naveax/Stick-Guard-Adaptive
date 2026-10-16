//! Independent, causal read-only candidate. The prior DirectionGuard is kept
//! unchanged for paired evaluation. Neither candidate infers player intent.
use crate::{
    hybrid::{HybridConfig, HybridGuard},
    reacquire::VerifiedCenter,
    Vec2,
};

const SAME_HEMISPHERE_COS: f64 = 0.5; // +/- 60 degrees
const MAX_OUTPUT_ANGLE_COS: f64 = 0.866_025_403_784_438_6; // +/- 30 degrees

pub struct ConfidenceDirection {
    base: HybridGuard,
    previous: Option<Vec2>,
    rotations: u64,
    bypasses: u64,
}
impl ConfidenceDirection {
    pub fn new(config: HybridConfig) -> Self {
        Self {
            base: HybridGuard::new(config),
            previous: None,
            rotations: 0,
            bypasses: 0,
        }
    }
    pub fn calibrate_neutral(&mut self, v: Vec2) -> bool {
        self.base.calibrate_neutral(v)
    }
    pub fn begin_live(&mut self) {
        self.base.begin_live();
        self.previous = None;
    }
    pub fn accept_verified(&mut self, v: VerifiedCenter) {
        self.base.accept_verified(v);
        self.previous = None;
    }
    pub fn rotations(&self) -> u64 {
        self.rotations
    }
    pub fn bypasses(&self) -> u64 {
        self.bypasses
    }
    pub fn process(&mut self, raw: Vec2) -> Vec2 {
        let current = self.base.process(raw);
        let telem = self.base.telemetry();
        let radius = current.norm();
        let noise = if telem.trusted { telem.noise_rms } else { 0.0 };
        let mut output = current;
        if noise > 0.003 && noise.is_finite() && radius > 0.0 && radius <= 3.0 * noise {
            if let Some(previous) = self.previous {
                let prev_radius = previous.norm();
                if prev_radius > 0.0 && prev_radius <= 3.0 * noise {
                    let cos =
                        (current.x * previous.x + current.y * previous.y) / (radius * prev_radius);
                    // Never use the previous sample if there is a genuine
                    // strong observed change in direction. Reversal may be
                    // intentional and its ground truth is unavailable.
                    if cos >= SAME_HEMISPHERE_COS {
                        let proposed = current * 0.5 + previous * 0.5;
                        let proposed_radius = proposed.norm();
                        if proposed_radius > 0.0
                            && (current.x * proposed.x + current.y * proposed.y)
                                >= MAX_OUTPUT_ANGLE_COS * radius * proposed_radius
                        {
                            output = (proposed * (radius / proposed_radius)).saturate();
                            self.rotations += 1;
                        } else {
                            self.bypasses += 1;
                        }
                    } else {
                        self.bypasses += 1;
                    }
                } else {
                    self.bypasses += 1;
                }
            }
        }
        // Do not compound a lagged output across frames.
        self.previous = Some(current);
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prepare(noise: f64) -> ConfidenceDirection {
        let mut g = ConfidenceDirection::new(HybridConfig::default());
        for i in 0..400 {
            let a = if i % 2 == 0 { noise } else { -noise };
            assert!(g.calibrate_neutral(Vec2::new(a, -a)));
        }
        g.begin_live();
        g
    }
    #[test]
    fn preserve_radius_and_cap_turn_across_10000_reports() {
        let mut base = HybridGuard::new(HybridConfig::default());
        let mut candidate = ConfidenceDirection::new(HybridConfig::default());
        for i in 0..400 {
            let n = if i % 2 == 0 { 0.01 } else { -0.01 };
            base.calibrate_neutral(Vec2::new(n, -n));
            candidate.calibrate_neutral(Vec2::new(n, -n));
        }
        base.begin_live();
        candidate.begin_live();
        for i in 0..10000 {
            let t = i as f64;
            let raw = Vec2::new(0.012 * (t * 0.201).cos(), 0.012 * (t * 0.203).sin());
            let a = base.process(raw);
            let b = candidate.process(raw);
            assert!((a.norm() - b.norm()).abs() < 1e-11);
            if a.norm() > 1e-9 {
                assert!(
                    (a.x * b.x + a.y * b.y) >= MAX_OUTPUT_ANGLE_COS * a.norm() * b.norm() - 1e-12
                );
            }
        }
        assert!(candidate.rotations() > 0);
    }
    #[test]
    fn physical_reversal_does_not_use_previous_sample() {
        let mut g = prepare(0.01);
        let center = g.base.telemetry().center;
        g.process(center + Vec2::new(0.01, 0.0));
        let out = g.process(center + Vec2::new(-0.0002, 0.0));
        assert!((out.x + 0.0002).abs() < 1e-9);
        assert_eq!(g.rotations(), 0);
    }
    #[test]
    fn insufficient_neutral_window_returns_raw() {
        let mut g = ConfidenceDirection::new(HybridConfig::default());
        for i in 0..127 {
            let x = if i % 2 == 0 { 0.01 } else { -0.01 };
            assert!(g.calibrate_neutral(Vec2::new(x, 0.0)));
        }
        g.begin_live();
        for i in 0..500 {
            let raw = Vec2::new(0.01 * (i as f64).cos(), 0.01 * (i as f64).sin());
            assert_eq!(g.process(raw), raw);
        }
        assert_eq!(g.rotations(), 0);
    }
    #[test]
    fn zero_and_flick_passthrough() {
        let mut g = prepare(0.01);
        let center = g.base.telemetry().center;
        assert_eq!(g.process(center), Vec2::ZERO);
        let fast = center + Vec2::new(0.75, -0.6);
        assert_eq!(g.process(fast), (fast - center).saturate());
        assert_eq!(g.rotations(), 0);
    }
}
