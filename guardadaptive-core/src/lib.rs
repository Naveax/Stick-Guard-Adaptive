//! GuardAdaptive. The hot path has no allocation and no access to simulation ground truth.
//! Units are normalized stick range [-1, 1]. One step = one input sample.
pub mod acceptance;
pub mod adversarial;
pub mod authorization_bounds_v63;
pub mod backend_counter_v61;
pub mod backend_epoch_v62;
pub mod blind_fence_v73;
pub mod blind_review_v66;
pub mod capture;
pub mod causal_guard;
pub mod coach;
pub mod concurrent_watch_v64;
pub mod confidence_direction;
pub mod conservative_research_v74;
pub mod crash_recovery_v65;
pub mod dataset_split_v67;
pub mod demo;
pub mod diagnostics;
pub mod direction_guard;
pub mod drift_holdout_v40;
pub mod driver_contract_v43;
pub mod evolve;
pub mod guided;
pub mod guided_live;
pub mod host_clock_v65;
pub mod hybrid;
pub mod lifecycle_v46;
pub mod live_monitor;
pub mod measured_lab;
pub mod motion_first;
pub mod neutral_audit;
pub mod neutral_session;
pub mod neutral_trend_v42;
pub mod noise_guard;
pub mod offline_fuzz_v65;
pub mod operator_recenter;
pub mod output_adapter;
pub mod output_supervisor;
pub mod pipeline_v32;
pub mod preview;
pub mod profile;
pub mod profile_archive;
pub mod profile_inventory;
pub mod profile_store;
pub mod provider_audit_v45;
pub mod provider_fault_v54;
pub mod provider_freshness_v59;
pub mod provider_replay_v60;
pub mod qualify;
pub mod reacquire;
pub mod recenter_bridge;
pub mod record_compare_v68;
pub mod recovery_overview_v49;
pub mod reference_gate_v41;
pub mod resolution;
pub mod routing_guard;
pub mod runtime_core;
pub mod safety_preview;
pub mod session_replay;
pub mod shadow;
pub mod sim;
pub mod software_suite;
pub mod stress_v39;
pub mod sweep;
pub mod truth_lab;
pub mod ui;
pub mod ui_neutral;
pub mod v27_all;
pub mod xinput;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}
impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn clamp(self) -> Self {
        Self::new(self.x.clamp(-1., 1.), self.y.clamp(-1., 1.))
    }
    pub fn saturate(self) -> Self {
        let n = self.norm();
        if n > 1. {
            self * (1. / n)
        } else {
            self
        }
    }
}
impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, r: Self) -> Self {
        Self::new(self.x + r.x, self.y + r.y)
    }
}
impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, r: Self) -> Self {
        Self::new(self.x - r.x, self.y - r.y)
    }
}
impl std::ops::Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, r: f64) -> Self {
        Self::new(self.x * r, self.y * r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMode {
    /// External, trusted instruction that the physical stick is untouched.
    /// Only call during a deliberate physical neutral calibration.
    CalibrateNeutral,
    /// In play, no oracle labels, no game state and no knowledge of intended motion.
    Live,
}

/// Precision preserves every finite sub-deadzone analog displacement; the
/// trade-off is that residual center noise also survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPolicy {
    AdaptiveDeadzone,
    PreserveMicro,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub calibration_alpha: f64,
    pub calibration_noise_alpha: f64,
    pub max_deadzone: f64,
    pub base_deadzone: f64,
    pub noise_multiplier: f64,
    pub auto_alpha: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            calibration_alpha: 0.065,
            calibration_noise_alpha: 0.02,
            max_deadzone: 0.075,
            base_deadzone: 0.002,
            noise_multiplier: 3.0,
            auto_alpha: 0.00002,
        }
    }
}
impl Config {
    /// Bound malformed configuration before it enters the real-time path.
    fn sanitized(self) -> Self {
        fn bound(v: f64, low: f64, high: f64, fallback: f64) -> f64 {
            if !v.is_finite() {
                fallback
            } else {
                v.clamp(low, high)
            }
        }
        let mut s = self;
        s.calibration_alpha = bound(s.calibration_alpha, 0.00001, 1.0, 0.065);
        s.calibration_noise_alpha = bound(s.calibration_noise_alpha, 0.00001, 1.0, 0.02);
        s.max_deadzone = bound(s.max_deadzone, 0.001, 0.3, 0.075);
        s.base_deadzone = bound(s.base_deadzone, 0.0, s.max_deadzone, 0.002);
        s.noise_multiplier = bound(s.noise_multiplier, 0.0, 10.0, 3.0);
        s.auto_alpha = bound(s.auto_alpha, 0.0, 0.005, 0.00002);
        s
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Telemetry {
    pub center: Vec2,
    pub deadzone: f64,
    pub noise: f64,
    pub neutral_confidence: f64,
    pub auto_updates: u64,
    pub neutral_calibration_samples: u64,
    pub effective_deadzone: f64,
}

pub struct GuardAdaptive {
    cfg: Config,
    output_policy: OutputPolicy,
    center: Vec2,
    noise: f64,
    calibrated: bool,
    calibration_samples: u64,
    auto_updates: u64,
    last_raw: Vec2,
    last_output: Vec2,
    stability: u32,
    since_motion: u32,
    saw_large_motion: bool,
    confidence: f64,
}
impl GuardAdaptive {
    pub fn new(cfg: Config) -> Self {
        Self::with_policy(cfg, OutputPolicy::AdaptiveDeadzone)
    }
    pub fn with_policy(cfg: Config, output_policy: OutputPolicy) -> Self {
        Self {
            cfg: cfg.sanitized(),
            output_policy,
            center: Vec2::ZERO,
            noise: 0.001,
            calibrated: false,
            calibration_samples: 0,
            auto_updates: 0,
            last_raw: Vec2::ZERO,
            last_output: Vec2::ZERO,
            stability: 0,
            since_motion: 0,
            saw_large_motion: false,
            confidence: 0.0,
        }
    }
    pub fn telemetry(&self) -> Telemetry {
        Telemetry {
            center: self.center,
            deadzone: self.deadzone(),
            noise: self.noise,
            neutral_confidence: self.confidence,
            auto_updates: self.auto_updates,
            neutral_calibration_samples: self.calibration_samples,
            effective_deadzone: match self.output_policy {
                OutputPolicy::AdaptiveDeadzone => self.deadzone(),
                OutputPolicy::PreserveMicro => 0.0,
            },
        }
    }
    pub fn deadzone(&self) -> f64 {
        (self.noise * self.cfg.noise_multiplier)
            .max(self.cfg.base_deadzone)
            .min(self.cfg.max_deadzone)
    }
    /// Reset learning gates but retain the explicitly trained center.
    pub fn begin_live(&mut self) {
        self.stability = 0;
        self.since_motion = 0;
        self.saw_large_motion = false;
        self.confidence = 0.0;
    }
    pub fn process(&mut self, raw: Vec2, mode: InputMode) -> Vec2 {
        // Non-finite samples are unsafe: return neutral but never teach the model.
        if !raw.finite() {
            self.confidence = 0.0;
            return Vec2::ZERO;
        }
        let raw = raw.clamp();
        let delta = (raw - self.last_raw).norm();
        self.last_raw = raw;
        match mode {
            InputMode::CalibrateNeutral => {
                // The first sample initializes instantly; subsequent calibration can
                // track even 90% bias shifts, because trusted neutral was asserted.
                if !self.calibrated {
                    self.center = raw;
                    self.calibrated = true;
                }
                let residual = (raw - self.center).norm();
                self.center = self.center + (raw - self.center) * self.cfg.calibration_alpha;
                // A giant center jump is not sensor noise.
                if residual < 0.05 {
                    self.noise = (1.0 - self.cfg.calibration_noise_alpha) * self.noise
                        + self.cfg.calibration_noise_alpha * residual;
                }
                self.calibration_samples += 1;
                self.confidence = 1.0;
                self.stability = 0;
                self.saw_large_motion = false;
            }
            InputMode::Live => {
                let residual = (raw - self.center).norm();
                // A large excursion is evidence of possible intentional motion,
                // but never proof of its cessation.
                if residual > 0.20 {
                    self.saw_large_motion = true;
                    self.since_motion = 0;
                } else {
                    self.since_motion = self.since_motion.saturating_add(1);
                }
                // Eligibility only after a major excursion followed by a long return
                // very close to calibrated center. Deliberate held micro-movement
                // remains an unsolved ambiguity. Updates are capped to near-zero.
                let near = residual < 0.012;
                if near && delta < 0.0015 {
                    self.stability = self.stability.saturating_add(1);
                } else {
                    self.stability = 0;
                }
                let eligible = self.calibrated
                    && self.saw_large_motion
                    && self.since_motion > 100
                    && self.stability > 250
                    && near;
                if eligible {
                    let correction = (raw - self.center) * self.cfg.auto_alpha;
                    self.center = self.center + correction;
                    self.auto_updates += 1;
                    self.confidence = 0.6;
                } else {
                    self.confidence = if near { 0.2 } else { 0. };
                }
            }
        }
        // No temporal smoothing on intended movement, so no filter-induced lag.
        let relative = raw - self.center;
        let radius = relative.norm();
        let dz = match self.output_policy {
            OutputPolicy::AdaptiveDeadzone => self.deadzone(),
            OutputPolicy::PreserveMicro => 0.0,
        };
        let out = if radius <= dz {
            Vec2::ZERO
        } else {
            // Radial deadzone with continuous response and full-scale normalization.
            // A naive bias subtraction can exceed physical range: clip safely.
            let gain = ((radius - dz) / (1.0 - dz)).min(1.0) / radius;
            (relative * gain).saturate()
        };
        self.last_output = out;
        out
    }
}
pub fn fixed_deadzone(input: Vec2, dz: f64) -> Vec2 {
    let n = input.norm();
    if n <= dz {
        Vec2::ZERO
    } else {
        (input * (((n - dz) / (1. - dz)).min(1.0) / n)).saturate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicitly_trained_large_offset_returns_neutral() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..300 {
            g.process(Vec2::new(0.9, 0.0), InputMode::CalibrateNeutral);
        }
        assert!(g.process(Vec2::new(0.9, 0.0), InputMode::Live).norm() < 1e-10);
    }
    #[test]
    fn held_small_intended_movement_is_not_quickly_learned_as_center() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..300 {
            g.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        g.begin_live();
        for _ in 0..1000 {
            g.process(Vec2::new(0.025, 0.0), InputMode::Live);
        }
        assert!(g.telemetry().center.norm() < 1e-10);
        assert!(g.process(Vec2::new(0.025, 0.), InputMode::Live).x > 0.02);
    }
    #[test]
    fn micro_precision_policy_preserves_001_percent_intended_input_after_trusted_neutral() {
        let mut precision =
            GuardAdaptive::with_policy(Config::default(), OutputPolicy::PreserveMicro);
        let mut deadzone = GuardAdaptive::new(Config::default());
        for _ in 0..400 {
            precision.process(Vec2::ZERO, InputMode::CalibrateNeutral);
            deadzone.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        let tiny = Vec2::new(0.00001, 0.0);
        assert_eq!(deadzone.process(tiny, InputMode::Live), Vec2::ZERO);
        assert!((precision.process(tiny, InputMode::Live).x - tiny.x).abs() < 1e-12);
        assert_eq!(precision.telemetry().effective_deadzone, 0.0);
    }
    #[test]
    fn identically_observed_drift_and_intention_cannot_be_decoded_differently() {
        let mut for_drift =
            GuardAdaptive::with_policy(Config::default(), OutputPolicy::PreserveMicro);
        let mut for_intent =
            GuardAdaptive::with_policy(Config::default(), OutputPolicy::PreserveMicro);
        for _ in 0..400 {
            for_drift.process(Vec2::ZERO, InputMode::CalibrateNeutral);
            for_intent.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        // Physical drift +0.9 while player is neutral, versus physically
        // perfect stick held right +0.9. Both produce the same raw signal.
        for _ in 0..512 {
            let x = Vec2::new(0.9, 0.0);
            assert_eq!(
                for_drift.process(x, InputMode::Live),
                for_intent.process(x, InputMode::Live)
            );
        }
    }
    #[test]
    fn noise_does_not_create_nan() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..100 {
            g.process(Vec2::new(0.003, -0.002), InputMode::CalibrateNeutral);
        }
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(g.process(Vec2::new(v, 0.), InputMode::Live), Vec2::ZERO);
        }
        assert!(g.telemetry().center.finite());
    }
    #[test]
    fn clipping_and_bounds_safe() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..100 {
            g.process(Vec2::new(-0.9, -0.9), InputMode::CalibrateNeutral);
        }
        for x in [-1., -0.3, 0., 0.5, 1.] {
            for y in [-1., 0., 1.] {
                let o = g.process(Vec2::new(x, y), InputMode::Live);
                assert!(o.finite() && o.norm() <= 1.00000001);
            }
        }
    }
    #[test]
    fn calibration_converges_after_step() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..200 {
            g.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        for _ in 0..300 {
            g.process(Vec2::new(0.9, 0.), InputMode::CalibrateNeutral);
        }
        assert!((g.telemetry().center.x - 0.9).abs() < 1e-6);
    }
    #[test]
    fn malformed_config_cannot_produce_nan_or_invalid_deadzone() {
        let config = Config {
            calibration_alpha: f64::INFINITY,
            calibration_noise_alpha: f64::NAN,
            max_deadzone: 1.0,
            base_deadzone: -1.0,
            noise_multiplier: f64::NAN,
            auto_alpha: f64::INFINITY,
        };
        let mut g = GuardAdaptive::new(config);
        for _ in 0..1000 {
            let o = g.process(Vec2::new(0.7, -0.3), InputMode::CalibrateNeutral);
            assert!(o.finite() && o.norm() <= 1.0);
        }
        assert!(g.telemetry().deadzone.is_finite());
        assert!(g.telemetry().deadzone < 1.0);
    }
    #[test]
    fn neutral_calibration_is_not_inferred_during_live_step() {
        let mut g = GuardAdaptive::new(Config::default());
        for _ in 0..300 {
            g.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        g.begin_live();
        for _ in 0..2000 {
            g.process(Vec2::new(0.9, 0.), InputMode::Live);
        }
        assert_eq!(g.telemetry().center, Vec2::ZERO);
    }
}
