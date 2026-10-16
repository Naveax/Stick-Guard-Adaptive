//! Labeled, deterministic, synthetic-only desktop UI demonstration.
//! The "desired" signal is secret from MotionFirst::process and exists solely
//! to score output; neither real XInput nor profile_store is referenced here.
use crate::{
    confidence_direction::ConfidenceDirection,
    direction_guard::DirectionGuard,
    hybrid::{HybridConfig, HybridGuard},
    motion_first::{MotionFirst, MotionFirstConfig},
    noise_guard::{NoiseGuard, NoiseGuardConfig},
    resolution::Resolution,
    InputMode, Vec2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Stable,
    Flipping,
    Creeping,
    Noisy,
}
impl Scenario {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "stable" => Some(Self::Stable),
            "flipping" => Some(Self::Flipping),
            "creeping" => Some(Self::Creeping),
            "noisy" => Some(Self::Noisy),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Flipping => "flipping",
            Self::Creeping => "creeping",
            Self::Noisy => "noisy",
        }
    }
    fn initial_bias(self) -> Vec2 {
        match self {
            Self::Stable => Vec2::new(0.18, -0.09),
            Self::Flipping => Vec2::new(0.85, 0.0),
            Self::Creeping => Vec2::ZERO,
            Self::Noisy => Vec2::new(0.025, -0.01),
        }
    }
    fn bias(self, n: u64) -> Vec2 {
        match self {
            Self::Stable => self.initial_bias(),
            Self::Flipping => {
                if (n / 250).is_multiple_of(2) {
                    self.initial_bias()
                } else {
                    self.initial_bias() * -1.0
                }
            }
            Self::Creeping => Vec2::new(
                (n as f64 / 4800.0).min(1.0) * 0.35,
                -(n as f64 / 4800.0).min(1.0) * 0.15,
            ),
            Self::Noisy => self.initial_bias(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct DemoFrame {
    pub left: Vec2,
    pub right: Vec2,
    pub desired: Vec2,
    pub corrected: Vec2,
    pub noise_corrected: Vec2,
    pub hybrid_corrected: Vec2,
    pub direction_corrected: Vec2,
    pub confidence_corrected: Vec2,
    pub count: u64,
    pub raw_rmse: f64,
    pub corrected_rmse: f64,
    pub noise_corrected_rmse: f64,
    pub hybrid_corrected_rmse: f64,
    pub direction_corrected_rmse: f64,
    pub confidence_corrected_rmse: f64,
    pub confidence_micro_lost: u64,
    pub confidence_reversed: u64,
    pub confidence_rotations: u64,
    pub direction_micro_lost: u64,
    pub direction_reversed: u64,
    pub direction_blended: u64,
    pub direction_max_radius_delta: f64,
    pub hybrid_micro_lost: u64,
    pub hybrid_reversed: u64,
    pub hybrid_noise_bypass: u64,
    pub hybrid_calibrated_noise: f64,
    pub noise_micro_lost: u64,
    pub noise_reversed: u64,
    pub micro_lost: u64,
    pub reversed: u64,
}
pub struct DemoEngine {
    scenario: Scenario,
    engine: MotionFirst,
    noise_engine: NoiseGuard,
    hybrid_engine: HybridGuard,
    direction_engine: DirectionGuard,
    confidence_engine: ConfidenceDirection,
    count: u64,
    raw_squared: f64,
    corrected_squared: f64,
    noise_corrected_squared: f64,
    hybrid_corrected_squared: f64,
    direction_corrected_squared: f64,
    confidence_corrected_squared: f64,
    confidence_micro_lost: u64,
    confidence_reversed: u64,
    direction_micro_lost: u64,
    direction_reversed: u64,
    direction_max_radius_delta: f64,
    hybrid_micro_lost: u64,
    hybrid_reversed: u64,
    noise_micro_lost: u64,
    noise_reversed: u64,
    micro_lost: u64,
    reversed: u64,
    latest: Option<DemoFrame>,
}
impl DemoEngine {
    pub fn new(scenario: Scenario) -> Self {
        let mut noise_engine = NoiseGuard::new(NoiseGuardConfig::default());
        let mut hybrid_engine = HybridGuard::new(HybridConfig::default());
        let mut direction_engine = DirectionGuard::new(HybridConfig::default());
        let mut confidence_engine = ConfidenceDirection::new(HybridConfig::default());
        let mut engine = MotionFirst::new(MotionFirstConfig {
            input_lsb: Resolution::XInput16.positive_lsb(),
            softness_lsb: 0.5,
        });
        // These neutral samples are declared trusted by the offline simulator,
        // NEVER derived from a live, non-consenting player's raw input.
        for i in 0..512 {
            let jitter = 1e-6 * (i as f64 * 0.79).sin();
            let raw =
                Resolution::XInput16.quantize(scenario.initial_bias() + Vec2::new(jitter, -jitter));
            engine.process(raw, InputMode::CalibrateNeutral);
            noise_engine.calibrate(raw);
            hybrid_engine.calibrate_neutral(raw);
            direction_engine.calibrate_neutral(raw);
            confidence_engine.calibrate_neutral(raw);
        }
        noise_engine.begin_live();
        hybrid_engine.begin_live();
        direction_engine.begin_live();
        confidence_engine.begin_live();
        Self {
            scenario,
            engine,
            noise_engine,
            hybrid_engine,
            direction_engine,
            confidence_engine,
            count: 0,
            raw_squared: 0.0,
            corrected_squared: 0.0,
            noise_corrected_squared: 0.0,
            hybrid_corrected_squared: 0.0,
            direction_corrected_squared: 0.0,
            confidence_corrected_squared: 0.0,
            confidence_micro_lost: 0,
            confidence_reversed: 0,
            direction_micro_lost: 0,
            direction_reversed: 0,
            direction_max_radius_delta: 0.0,
            hybrid_micro_lost: 0,
            hybrid_reversed: 0,
            noise_micro_lost: 0,
            noise_reversed: 0,
            micro_lost: 0,
            reversed: 0,
            latest: None,
        }
    }
    pub fn scenario(&self) -> Scenario {
        self.scenario
    }
    pub fn latest(&self) -> Option<DemoFrame> {
        self.latest
    }
    pub fn step(&mut self) -> DemoFrame {
        let n = self.count;
        let phase = n % 500;
        let desired = match phase {
            0..=89 => Vec2::ZERO,
            90..=169 => Vec2::new(0.002, 0.0),
            170..=229 => Vec2::new(-0.0001, 0.0001),
            230..=309 => Vec2::new(0.25, -0.12),
            310..=439 => {
                let f = (phase - 310) as f64 / 129.0;
                Vec2::new(-0.75 + 1.5 * f, 0.3 * (f * std::f64::consts::TAU).sin())
            }
            _ => Vec2::new(-0.3, 0.45),
        };
        let noise_scale = if self.scenario == Scenario::Noisy {
            0.006
        } else {
            0.000025
        };
        let noise = Vec2::new(
            noise_scale * (n as f64 * 1.147).sin(),
            noise_scale * (n as f64 * 0.763).cos(),
        );
        let observed =
            Resolution::XInput16.quantize((desired + self.scenario.bias(n) + noise).clamp());
        let corrected = self.engine.process(observed, InputMode::Live);
        let noise_corrected = self.noise_engine.process(observed);
        let hybrid_corrected = self.hybrid_engine.process(observed);
        let direction_corrected = self.direction_engine.process(observed);
        let confidence_corrected = self.confidence_engine.process(observed);
        self.direction_max_radius_delta = self
            .direction_max_radius_delta
            .max((direction_corrected.norm() - hybrid_corrected.norm()).abs());
        self.count += 1;
        self.raw_squared += (observed - desired).length_squared();
        self.corrected_squared += (corrected - desired).length_squared();
        self.noise_corrected_squared += (noise_corrected - desired).length_squared();
        self.hybrid_corrected_squared += (hybrid_corrected - desired).length_squared();
        self.direction_corrected_squared += (direction_corrected - desired).length_squared();
        self.confidence_corrected_squared += (confidence_corrected - desired).length_squared();
        if desired.norm() > 0.00001 && confidence_corrected.norm() < 0.5 * desired.norm() {
            self.confidence_micro_lost += 1;
        }
        if desired.norm() > 0.00001
            && confidence_corrected.norm() > 0.00001
            && desired.x * confidence_corrected.x + desired.y * confidence_corrected.y < 0.0
        {
            self.confidence_reversed += 1;
        }
        if desired.norm() > 0.00001 && direction_corrected.norm() < 0.5 * desired.norm() {
            self.direction_micro_lost += 1;
        }
        if desired.norm() > 0.00001
            && direction_corrected.norm() > 0.00001
            && desired.x * direction_corrected.x + desired.y * direction_corrected.y < 0.0
        {
            self.direction_reversed += 1;
        }
        if desired.norm() > 0.00001 && hybrid_corrected.norm() < 0.5 * desired.norm() {
            self.hybrid_micro_lost += 1;
        }
        if desired.norm() > 0.00001
            && hybrid_corrected.norm() > 0.00001
            && desired.x * hybrid_corrected.x + desired.y * hybrid_corrected.y < 0.0
        {
            self.hybrid_reversed += 1;
        }
        if desired.norm() > 0.00001 && noise_corrected.norm() < 0.5 * desired.norm() {
            self.noise_micro_lost += 1;
        }
        if desired.norm() > 0.00001
            && noise_corrected.norm() > 0.00001
            && desired.x * noise_corrected.x + desired.y * noise_corrected.y < 0.0
        {
            self.noise_reversed += 1;
        }
        if desired.norm() > 0.00001 && corrected.norm() < 0.5 * desired.norm() {
            self.micro_lost += 1;
        }
        if desired.norm() > 0.00001
            && corrected.norm() > 0.00001
            && desired.x * corrected.x + desired.y * corrected.y < 0.0
        {
            self.reversed += 1;
        }
        let snapshot = DemoFrame {
            left: observed,
            // Only the LEFT analog is simulated and scored; do not depict
            // decorative right-stick motion as a validated correction.
            right: Vec2::ZERO,
            desired,
            corrected,
            noise_corrected,
            hybrid_corrected,
            direction_corrected,
            confidence_corrected,
            count: self.count,
            raw_rmse: (self.raw_squared / self.count as f64).sqrt(),
            corrected_rmse: (self.corrected_squared / self.count as f64).sqrt(),
            noise_corrected_rmse: (self.noise_corrected_squared / self.count as f64).sqrt(),
            hybrid_corrected_rmse: (self.hybrid_corrected_squared / self.count as f64).sqrt(),
            direction_corrected_rmse: (self.direction_corrected_squared / self.count as f64).sqrt(),
            confidence_corrected_rmse: (self.confidence_corrected_squared / self.count as f64)
                .sqrt(),
            confidence_micro_lost: self.confidence_micro_lost,
            confidence_reversed: self.confidence_reversed,
            confidence_rotations: self.confidence_engine.rotations(),
            direction_micro_lost: self.direction_micro_lost,
            direction_reversed: self.direction_reversed,
            direction_blended: self.direction_engine.blended_frames(),
            direction_max_radius_delta: self.direction_max_radius_delta,
            hybrid_micro_lost: self.hybrid_micro_lost,
            hybrid_reversed: self.hybrid_reversed,
            hybrid_noise_bypass: self.hybrid_engine.telemetry().noise_attenuation_bypasses,
            hybrid_calibrated_noise: self.hybrid_engine.telemetry().noise_rms,
            noise_micro_lost: self.noise_micro_lost,
            noise_reversed: self.noise_reversed,
            micro_lost: self.micro_lost,
            reversed: self.reversed,
        };
        self.latest = Some(snapshot);
        snapshot
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_and_bounded_for_all_scenarios() {
        for scenario in [
            Scenario::Stable,
            Scenario::Flipping,
            Scenario::Creeping,
            Scenario::Noisy,
        ] {
            let mut a = DemoEngine::new(scenario);
            let mut b = DemoEngine::new(scenario);
            for _ in 0..2000 {
                let aa = a.step();
                let bb = b.step();
                assert_eq!(aa.left, bb.left);
                assert_eq!(aa.corrected, bb.corrected);
                assert_eq!(aa.noise_corrected, bb.noise_corrected);
                assert_eq!(aa.hybrid_corrected, bb.hybrid_corrected);
                assert_eq!(aa.direction_corrected, bb.direction_corrected);
                assert_eq!(aa.confidence_corrected, bb.confidence_corrected);
                assert!(aa.noise_corrected.finite());
                assert!(aa.corrected.finite() && aa.left.finite());
                assert!(aa.corrected.norm() <= 1.0 + 1e-9);
            }
        }
    }
    #[test]
    fn stable_known_center_reduces_error_but_not_proof_of_live_use() {
        let mut e = DemoEngine::new(Scenario::Stable);
        let mut r = e.step();
        for _ in 0..1999 {
            r = e.step();
        }
        assert!(
            r.corrected_rmse < 0.05 * r.raw_rmse,
            "corrected={} raw={}",
            r.corrected_rmse,
            r.raw_rmse
        );
    }
    #[test]
    fn severe_flipping_drift_still_has_large_unrecoverable_error() {
        let mut e = DemoEngine::new(Scenario::Flipping);
        let mut r = e.step();
        for _ in 0..999 {
            r = e.step();
        }
        assert!(r.noise_corrected_rmse > 0.5);
        assert!(r.hybrid_corrected_rmse > 0.5);
        assert!(
            r.corrected_rmse > 0.5,
            "impossible case must remain obvious"
        );
    }
    #[test]
    fn invalid_scenario_never_defaults_silently() {
        assert_eq!(Scenario::parse("Stable"), None);
        assert_eq!(Scenario::parse("hardware"), None);
        assert_eq!(Scenario::parse("stable"), Some(Scenario::Stable));
    }
}
