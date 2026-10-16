//! Streaming multidimensional adversarial sweep. Oracle truth is scoring-only.
use crate::{
    confidence_direction::ConfidenceDirection,
    direction_guard::DirectionGuard,
    fixed_deadzone,
    hybrid::{HybridConfig, HybridGuard},
    motion_first::{MotionFirst, MotionFirstConfig},
    noise_guard::{NoiseGuard, NoiseGuardConfig},
    resolution::Resolution,
    Config, GuardAdaptive, InputMode, OutputPolicy, Vec2,
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Instant,
};

const AMPLITUDES: [f64; 15] = [
    0.00001, 0.00002, 0.00005, 0.0001, 0.0002, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1,
    0.5, 1.0,
]; // 0.001% to 100% inclusive
const DIRECTIONS: usize = 16;
const TRANSITIONS: usize = 8;
const INTENTS: usize = 9;
const NOISES: [f64; 4] = [0.0, 0.00001, 0.0005, 0.01];
pub const GRID_CASES: u64 = AMPLITUDES.len() as u64
    * DIRECTIONS as u64
    * TRANSITIONS as u64
    * INTENTS as u64
    * NOISES.len() as u64;

#[derive(Clone, Copy)]
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn fraction(&mut self) -> f64 {
        ((self.next() >> 11) as f64) / (1_u64 << 53) as f64
    }
    fn signed(&mut self) -> f64 {
        self.fraction() * 2.0 - 1.0
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub amplitude: f64,
    pub direction: f64,
    pub transition: u8,
    pub intent: u8,
    pub noise: f64,
    pub seed: u64,
    pub case_id: u64,
}
impl Spec {
    /// Systematic grid with unique mixed-radix coordinates (no missing endpoints).
    pub fn grid(id: u64, seed: u64) -> Self {
        let mut v = id % GRID_CASES;
        let noise = NOISES[(v % NOISES.len() as u64) as usize];
        v /= NOISES.len() as u64;
        let intent = (v % INTENTS as u64) as u8;
        v /= INTENTS as u64;
        let transition = (v % TRANSITIONS as u64) as u8;
        v /= TRANSITIONS as u64;
        let direction = (v % DIRECTIONS as u64) as f64 * std::f64::consts::TAU / DIRECTIONS as f64;
        v /= DIRECTIONS as u64;
        let amplitude = AMPLITUDES[v as usize];
        Self {
            amplitude,
            direction,
            transition,
            intent,
            noise,
            seed,
            case_id: id,
        }
    }
    /// Continuous log-uniform exploration in addition to finite grid anchors.
    pub fn randomized(id: u64, seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ id.wrapping_mul(0x9e3779b97f4a7c15));
        let amplitude =
            (0.00001_f64.ln() + rng.fraction() * (1.0_f64.ln() - 0.00001_f64.ln())).exp();
        Self {
            amplitude,
            direction: rng.fraction() * std::f64::consts::TAU,
            transition: (rng.next() % TRANSITIONS as u64) as u8,
            intent: (rng.next() % INTENTS as u64) as u8,
            noise: NOISES[(rng.next() % NOISES.len() as u64) as usize],
            seed,
            case_id: id,
        }
    }
}
fn motion(style: u8, t: usize, frames: usize, angle: f64) -> Vec2 {
    let z = t as f64 / frames.max(1) as f64;
    let theta = angle + std::f64::consts::FRAC_PI_3;
    let direction = Vec2::new(theta.cos(), theta.sin());
    let (a, phi) = match style {
        0 => (0.0, 0.0),     // neutral only
        1 => (0.00001, 0.0), // smallest user movement
        2 => (0.0001, 0.0),
        3 => (0.001, 0.0),
        4 => (0.01, 0.0),
        5 => (0.1, 0.0),
        6 => (0.75, 0.0),
        7 => (0.7, (z * 50.0).sin()),   // large fast trajectory
        _ => (0.03, (z * 120.0).sin()), // slow precision tracking + reversals
    };
    if t < frames / 5 || t > frames * 4 / 5 {
        return Vec2::ZERO;
    }
    if style == 7 {
        Vec2::new((z * 50.0).sin() * a, (z * 42.0).cos() * a)
    } else if style == 8 {
        direction * (a * phi)
    } else {
        direction * a
    }
}
fn corrupt(spec: Spec, t: usize, frames: usize, rng: &mut Rng) -> Vec2 {
    let z = t as f64 / frames.max(1) as f64;
    let dir = Vec2::new(spec.direction.cos(), spec.direction.sin());
    let amp = spec.amplitude;
    let shock = match spec.transition {
        0 => 1.0,        // fixed after trusted calibration (known initial bias is 0)
        1 => z.min(1.0), // linear
        2 => z.powi(3),  // accelerating ramp
        3 => {
            if t < frames / 2 {
                0.0
            } else {
                1.0
            }
        } // instant unlearned jump
        4 => {
            if (t / 43).is_multiple_of(2) {
                0.0
            } else {
                1.0
            }
        } // rapid telegraph
        5 => {
            if (z * 120.0).sin() > 0.0 {
                1.0
            } else {
                -1.0
            }
        } // sign-flipping
        6 => (z * 53.0).sin(), // oscillating
        _ => {
            if rng.fraction() < 0.2 {
                rng.signed()
            } else {
                (z * 4.0).sin()
            }
        } // random pulses
    };
    dir * (amp * shock)
}
#[derive(Default, Clone, Copy, Debug)]
pub struct Score {
    pub n_neutral: u64,
    pub n_motion: u64,
    pub neutral_sq: f64,
    pub motion_sq: f64,
    pub neutral_max: f64,
    pub motion_max: f64,
    pub micro_lost: u64,
    /// Intentional movements passing the standalone input report resolution,
    /// but still attenuated below 50% of the desired magnitude.
    pub micro_lost_above_resolution: u64,
    /// Intentional movement smaller than report resolution in isolation.
    pub micro_lost_below_resolution: u64,
    pub wrong_direction: u64,
    /// Sign-only direction reversal; magnitude independent, except exact zero.
    pub sign_opposite: u64,
    pub false_positive: u64,
    pub high_error: u64,
}
impl Score {
    #[cfg(test)]
    fn update(&mut self, desired: Vec2, output: Vec2) {
        self.update_with_resolution(desired, output, true)
    }
    fn update_with_resolution(&mut self, desired: Vec2, output: Vec2, input_resolvable: bool) {
        let err = (output - desired).norm();
        if desired.norm() < 1e-12 {
            self.n_neutral += 1;
            self.neutral_sq += err * err;
            self.neutral_max = self.neutral_max.max(err);
            if output.norm() > 0.01 {
                self.false_positive += 1;
            }
        } else {
            self.n_motion += 1;
            self.motion_sq += err * err;
            self.motion_max = self.motion_max.max(err);
            if desired.norm() >= 1e-5 && output.norm() < desired.norm() * 0.5 {
                self.micro_lost += 1;
                if input_resolvable {
                    self.micro_lost_above_resolution += 1;
                } else {
                    self.micro_lost_below_resolution += 1;
                }
            }
            let dot = desired.x * output.x + desired.y * output.y;
            if dot < -0.000001 {
                self.wrong_direction += 1;
            }
            if dot < 0.0 && output.norm() > 1e-12 {
                self.sign_opposite += 1;
            }
        }
        if err > 0.1 {
            self.high_error += 1;
        }
    }
    pub fn neutral_rmse(self) -> f64 {
        (self.neutral_sq / self.n_neutral.max(1) as f64).sqrt()
    }
    pub fn motion_rmse(self) -> f64 {
        (self.motion_sq / self.n_motion.max(1) as f64).sqrt()
    }
    pub fn objective(self) -> f64 {
        self.neutral_rmse()
            + 2.0 * self.motion_rmse()
            + self.micro_lost as f64 / self.n_motion.max(1) as f64 * 0.1
    }
}
pub struct ResultCase {
    pub spec: Spec,
    pub guard: Score,
    pub precision: Score,
    pub motion_first: Score,
    pub noise_guard: Score,
    pub hybrid: Score,
    pub direction_guard: Score,
    pub confidence_direction: Score,
    pub raw: Score,
    pub fixed: Score,
    pub oracle: Score,
    pub auto_updates: u64,
    /// Count of intended nonzero axis-magnitude samples under the isolated
    /// quantizer's zero code when evaluated with zero sensor noise/offset.
    /// This does NOT prove a particular physical sensor would emit zero.
    pub below_report_resolution: u64,
    pub quantization_changed: u64,
}
pub fn evaluate(spec: Spec, frames: usize) -> ResultCase {
    evaluate_with_resolution(spec, frames, Resolution::Ideal)
}
pub fn evaluate_with_resolution(spec: Spec, frames: usize, resolution: Resolution) -> ResultCase {
    let frames = frames.max(256);
    let mut guard = GuardAdaptive::new(Config::default());
    let mut precision = GuardAdaptive::with_policy(Config::default(), OutputPolicy::PreserveMicro);
    let mut motion_first = MotionFirst::new(MotionFirstConfig {
        input_lsb: resolution.positive_lsb(),
        softness_lsb: 0.5,
    });
    let mut noise_guard = NoiseGuard::new(NoiseGuardConfig {
        input_lsb: resolution.positive_lsb(),
        ..NoiseGuardConfig::default()
    });
    let mut hybrid = HybridGuard::new(HybridConfig {
        report_lsb: resolution.positive_lsb(),
        ..HybridConfig::default()
    });
    let mut confidence_direction = ConfidenceDirection::new(HybridConfig {
        report_lsb: resolution.positive_lsb(),
        ..HybridConfig::default()
    });
    let mut direction_guard = DirectionGuard::new(HybridConfig {
        report_lsb: resolution.positive_lsb(),
        ..HybridConfig::default()
    });
    let mut rng = Rng::new(spec.seed ^ spec.case_id.wrapping_mul(0xa0761d6478bd642f));
    // Explicit neutral calibration at zero bias, never updated from truth in play.
    for _ in 0..400 {
        let v = Vec2::new(rng.signed(), rng.signed()) * spec.noise;
        let captured = resolution.quantize(v);
        guard.process(captured, InputMode::CalibrateNeutral);
        precision.process(captured, InputMode::CalibrateNeutral);
        motion_first.process(captured, InputMode::CalibrateNeutral);
        noise_guard.calibrate(captured);
        hybrid.calibrate_neutral(captured);
        direction_guard.calibrate_neutral(captured);
        confidence_direction.calibrate_neutral(captured);
    }
    guard.begin_live();
    precision.begin_live();
    noise_guard.begin_live();
    hybrid.begin_live();
    direction_guard.begin_live();
    confidence_direction.begin_live();
    let mut a = Score::default();
    let mut precision_score = Score::default();
    let mut motion_first_score = Score::default();
    let mut noise_guard_score = Score::default();
    let mut hybrid_score = Score::default();
    let mut direction_score = Score::default();
    let mut confidence_score = Score::default();
    let mut raw = Score::default();
    let mut fixed = Score::default();
    let mut oracle = Score::default();
    let mut filtered = Vec2::ZERO;
    let mut previous_motion = Vec2::ZERO;
    let mut below_report_resolution = 0_u64;
    let mut quantization_changed = 0_u64;
    for t in 0..frames {
        let desired = motion(spec.intent, t, frames, spec.direction);
        let bias = corrupt(spec, t, frames, &mut rng);
        filtered = filtered * 0.92 + Vec2::new(rng.signed(), rng.signed()) * (spec.noise * 0.08);
        // Release snapback / directional hysteresis added on top of each combination.
        let release = previous_motion.norm() > 0.015 && desired.norm() < 0.000001;
        let extra = if release {
            Vec2::new(0.15, -0.09) * (spec.amplitude.min(1.0))
        } else {
            Vec2::ZERO
        };
        let noise = Vec2::new(rng.signed(), rng.signed()) * spec.noise + filtered;
        let ideal_observed = (desired + bias + noise + extra).clamp();
        let observed = resolution.quantize(ideal_observed);
        if observed != ideal_observed {
            quantization_changed += 1;
        }
        // Diagnostic: quantization alone, with no drift and no sensor noise.
        // Never leaked to the correction engine.
        if desired.norm() > 0.0 && resolution.quantize(desired) == Vec2::ZERO {
            below_report_resolution += 1;
        }
        let corrected = guard.process(observed, InputMode::Live);
        let isolated_resolvable = resolution.quantize(desired) != Vec2::ZERO;
        a.update_with_resolution(desired, corrected, isolated_resolvable);
        precision_score.update_with_resolution(
            desired,
            precision.process(observed, InputMode::Live),
            isolated_resolvable,
        );
        motion_first_score.update_with_resolution(
            desired,
            motion_first.process(observed, InputMode::Live),
            isolated_resolvable,
        );
        noise_guard_score.update_with_resolution(
            desired,
            noise_guard.process(observed),
            isolated_resolvable,
        );
        hybrid_score.update_with_resolution(desired, hybrid.process(observed), isolated_resolvable);
        direction_score.update_with_resolution(
            desired,
            direction_guard.process(observed),
            isolated_resolvable,
        );
        confidence_score.update_with_resolution(
            desired,
            confidence_direction.process(observed),
            isolated_resolvable,
        );
        raw.update_with_resolution(desired, observed, isolated_resolvable);
        fixed.update_with_resolution(desired, fixed_deadzone(observed, 0.05), isolated_resolvable);
        // Simulated-bias oracle demonstrates information lost to clipping.
        oracle.update_with_resolution(
            desired,
            (observed - bias - extra).clamp(),
            isolated_resolvable,
        );
        previous_motion = desired;
    }
    ResultCase {
        spec,
        guard: a,
        precision: precision_score,
        motion_first: motion_first_score,
        noise_guard: noise_guard_score,
        hybrid: hybrid_score,
        direction_guard: direction_score,
        confidence_direction: confidence_score,
        raw,
        fixed,
        oracle,
        auto_updates: guard.telemetry().auto_updates,
        below_report_resolution,
        quantization_changed,
    }
}
pub fn run(seed: u64, cases: u64, frames: usize, out: &Path) -> io::Result<()> {
    run_with_resolution(seed, cases, frames, out, Resolution::Ideal)
}
pub fn run_with_resolution(
    seed: u64,
    cases: u64,
    frames: usize,
    out: &Path,
    resolution: Resolution,
) -> io::Result<()> {
    if cases == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cases must be positive",
        ));
    }
    let frames = frames.clamp(256, 65536);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(out)?;
    let mut f = io::BufWriter::new(file);
    writeln!(f,"case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score,precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction,resolution,report_lsb,below_report_resolution,quantization_changed,guard_micro_lost_above_resolution,guard_micro_lost_below_resolution,precision_micro_lost_above_resolution,precision_micro_lost_below_resolution,motionfirst_neutral_rmse,motionfirst_motion_rmse,motionfirst_micro_lost,motionfirst_wrong_direction,motionfirst_micro_lost_above_resolution,motionfirst_micro_lost_below_resolution,noiseguard_neutral_rmse,noiseguard_motion_rmse,noiseguard_micro_lost,noiseguard_wrong_direction,noiseguard_micro_lost_above_resolution,noiseguard_micro_lost_below_resolution,hybrid_neutral_rmse,hybrid_motion_rmse,hybrid_micro_lost,hybrid_wrong_direction,hybrid_micro_lost_above_resolution,hybrid_micro_lost_below_resolution,direction_neutral_rmse,direction_motion_rmse,direction_micro_lost,direction_wrong_direction,direction_micro_lost_above_resolution,direction_micro_lost_below_resolution,hybrid_sign_opposite,direction_sign_opposite,confidence_neutral_rmse,confidence_motion_rmse,confidence_micro_lost,confidence_wrong_direction,confidence_micro_lost_above_resolution,confidence_micro_lost_below_resolution,confidence_sign_opposite")?;
    let begin = Instant::now();
    let mut worst: Vec<(f64, u64, u64, bool)> = Vec::new();
    let mut finished = 0u64;
    let mut better_neutral = 0u64;
    let mut better_motion = 0u64;
    let mut worse_than_fixed = 0u64;
    let mut precision_better_micro = 0u64;
    let mut precision_worse_neutral = 0u64;
    let mut motionfirst_better_motion = 0u64;
    let mut motionfirst_worse_neutral = 0u64;
    let mut noise_better_neutral_vs_motionfirst = 0u64;
    let mut noise_worse_motion_vs_motionfirst = 0u64;
    let mut hybrid_better_neutral_vs_motionfirst = 0_u64;
    let mut hybrid_better_motion_vs_motionfirst = 0_u64;
    let mut hybrid_more_micro_lost_than_motionfirst = 0_u64;
    let mut direction_better_wrong_cases = 0_u64;
    let mut direction_worse_micro_cases = 0_u64;
    for i in 0..cases {
        // First finite factorial grid, then continuous log-uniform attack fuzzing.
        let is_random = i >= GRID_CASES;
        let spec = if is_random {
            Spec::randomized(i - GRID_CASES, seed)
        } else {
            Spec::grid(i, seed)
        };
        let result = evaluate_with_resolution(spec, frames, resolution);
        let score = result.guard.objective();
        if result.guard.neutral_rmse() < result.fixed.neutral_rmse() {
            better_neutral += 1;
        }
        if result.guard.motion_rmse() < result.fixed.motion_rmse() {
            better_motion += 1;
        }
        if score > result.fixed.objective() {
            worse_than_fixed += 1;
        }
        if result.precision.micro_lost < result.guard.micro_lost {
            precision_better_micro += 1;
        }
        if result.precision.neutral_rmse() > result.guard.neutral_rmse() + 1e-9 {
            precision_worse_neutral += 1;
        }
        if result.motion_first.motion_rmse() + 1e-9 < result.guard.motion_rmse() {
            motionfirst_better_motion += 1;
        }
        if result.motion_first.neutral_rmse() > result.guard.neutral_rmse() + 1e-9 {
            motionfirst_worse_neutral += 1;
        }
        if result.noise_guard.neutral_rmse() + 1e-9 < result.motion_first.neutral_rmse() {
            noise_better_neutral_vs_motionfirst += 1;
        }
        if result.noise_guard.motion_rmse() > result.motion_first.motion_rmse() + 1e-9 {
            noise_worse_motion_vs_motionfirst += 1;
        }
        if result.hybrid.neutral_rmse() + 1e-9 < result.motion_first.neutral_rmse() {
            hybrid_better_neutral_vs_motionfirst += 1;
        }
        if result.hybrid.motion_rmse() + 1e-9 < result.motion_first.motion_rmse() {
            hybrid_better_motion_vs_motionfirst += 1;
        }
        if result.hybrid.micro_lost > result.motion_first.micro_lost {
            hybrid_more_micro_lost_than_motionfirst += 1;
        }
        if result.direction_guard.wrong_direction < result.hybrid.wrong_direction {
            direction_better_wrong_cases += 1;
        }
        if result.direction_guard.micro_lost > result.hybrid.micro_lost {
            direction_worse_micro_cases += 1;
        }
        writeln!(f,"{},{},{},{:.8},{:.3},{},{},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{},{},{},{},{},{:.8},{:.8},{:.8},{},{},{},{:.10},{},{},{},{},{},{},{:.8},{:.8},{},{},{},{},{:.8},{:.8},{},{},{},{},{:.8},{:.8},{},{},{},{},{:.8},{:.8},{},{},{},{},{},{},{:.8},{:.8},{},{},{},{},{}",
            i,seed,if is_random{"continuous"}else{"grid"},spec.amplitude*100.0,
            spec.direction.to_degrees(),spec.transition,spec.intent,spec.noise*100.0,
            result.guard.neutral_rmse(),result.guard.motion_rmse(),
            result.raw.neutral_rmse(),result.raw.motion_rmse(),
            result.fixed.neutral_rmse(),result.fixed.motion_rmse(),
            result.oracle.motion_rmse(),result.guard.micro_lost,
            result.guard.wrong_direction,result.guard.false_positive,
            result.guard.high_error,result.auto_updates,score,
            result.precision.neutral_rmse(),result.precision.motion_rmse(),
            result.precision.micro_lost,result.precision.wrong_direction,
            resolution.name(),resolution.positive_lsb(),result.below_report_resolution,result.quantization_changed,
            result.guard.micro_lost_above_resolution,result.guard.micro_lost_below_resolution,
            result.precision.micro_lost_above_resolution,result.precision.micro_lost_below_resolution,
            result.motion_first.neutral_rmse(),result.motion_first.motion_rmse(),result.motion_first.micro_lost,
            result.motion_first.wrong_direction,result.motion_first.micro_lost_above_resolution,result.motion_first.micro_lost_below_resolution,
            result.noise_guard.neutral_rmse(),result.noise_guard.motion_rmse(),result.noise_guard.micro_lost,
            result.noise_guard.wrong_direction,result.noise_guard.micro_lost_above_resolution,result.noise_guard.micro_lost_below_resolution,
            result.hybrid.neutral_rmse(),result.hybrid.motion_rmse(),result.hybrid.micro_lost,
            result.hybrid.wrong_direction,result.hybrid.micro_lost_above_resolution,result.hybrid.micro_lost_below_resolution,
            result.direction_guard.neutral_rmse(),result.direction_guard.motion_rmse(),result.direction_guard.micro_lost,
            result.direction_guard.wrong_direction,result.direction_guard.micro_lost_above_resolution,result.direction_guard.micro_lost_below_resolution,
            result.hybrid.sign_opposite,result.direction_guard.sign_opposite,
            result.confidence_direction.neutral_rmse(),result.confidence_direction.motion_rmse(),result.confidence_direction.micro_lost,
            result.confidence_direction.wrong_direction,result.confidence_direction.micro_lost_above_resolution,
            result.confidence_direction.micro_lost_below_resolution,result.confidence_direction.sign_opposite)?;
        worst.push((score, spec.case_id, i, is_random));
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        worst.truncate(12);
        finished += 1;
    }
    f.flush()?;
    println!("SWEEP COMPLETE: cases={finished}, frames/case={frames}, total_samples={} elapsed_ms={} CSV={}",
        finished*frames as u64,begin.elapsed().as_millis(),out.display());
    println!("Grid={} combinations (15 amplitudes x 16 directions x 8 transitions x 9 intentions x 4 noise levels)",GRID_CASES);
    println!(
        "Better than 5% deadzone on neutral={} motion={}; total worse-on-combined-score={}",
        better_neutral, better_motion, worse_than_fixed
    );
    println!(
        "INPUT REPORT MODEL={} LSB={:.10} (normalized axis)",
        resolution.name(),
        resolution.positive_lsb()
    );
    println!("Precision preserves more micro samples in {} cases but has worse neutral RMS in {} cases; not proof of real hardware gains.", precision_better_micro, precision_worse_neutral);
    println!("MotionFirst candidate better-motion cases={} worse-neutral cases={} (vs legacy adaptive, paired synthetic truth)",motionfirst_better_motion,motionfirst_worse_neutral);
    println!("NoiseGuard exploratory vs MotionFirst: better-neutral cases={} worse-motion cases={} (paired, no privileged desired input)",noise_better_neutral_vs_motionfirst,noise_worse_motion_vs_motionfirst);
    println!("Hybrid bounded-gain candidate vs MotionFirst: better-neutral={} better-motion={} more-micro-loss-cases={}",hybrid_better_neutral_vs_motionfirst,hybrid_better_motion_vs_motionfirst,hybrid_more_micro_lost_than_motionfirst);
    println!("Direction smoothing research candidate: better-wrong-direction-cases={} worse-micro-loss-cases={} vs HybridGuard",direction_better_wrong_cases,direction_worse_micro_cases);
    for (index, (score, case_id, idx, is_random)) in worst.iter().enumerate() {
        println!("WORST #{:02} score={:.6} case_id={} index={} kind={} replay: sweep --case {} --seed {} --frames {}",
            index+1,score,case_id,idx,if *is_random{"continuous"}else{"grid"},idx,seed,frames);
    }
    Ok(())
}
pub fn replay(seed: u64, index: u64, frames: usize) {
    replay_with_resolution(seed, index, frames, Resolution::Ideal)
}
pub fn replay_with_resolution(seed: u64, index: u64, frames: usize, resolution: Resolution) {
    let spec = if index >= GRID_CASES {
        Spec::randomized(index - GRID_CASES, seed)
    } else {
        Spec::grid(index, seed)
    };
    let r = evaluate_with_resolution(spec, frames, resolution);
    println!(
        "REPORT MODEL={} LSB={:.10} below_report_resolution={} quantization_changed={}",
        resolution.name(),
        resolution.positive_lsb(),
        r.below_report_resolution,
        r.quantization_changed
    );
    println!(
        "REPLAY case={} amp={:.7}% direction={:.3} transition={} intent={} noise={:.6}%",
        index,
        spec.amplitude * 100.0,
        spec.direction.to_degrees(),
        spec.transition,
        spec.intent,
        spec.noise * 100.0
    );
    println!(
        "GUARD neutral={:.8} motion={:.8} micro_lost={} wrong_direction={} high_error={}",
        r.guard.neutral_rmse(),
        r.guard.motion_rmse(),
        r.guard.micro_lost,
        r.guard.wrong_direction,
        r.guard.high_error
    );
    println!(
        "PRECISION neutral={:.8} motion={:.8} micro_lost={} wrong_direction={}",
        r.precision.neutral_rmse(),
        r.precision.motion_rmse(),
        r.precision.micro_lost,
        r.precision.wrong_direction
    );
    println!(
        "NOISEGUARD neutral={:.8} motion={:.8} micro_lost={} reversed={} (experiment only)",
        r.noise_guard.neutral_rmse(),
        r.noise_guard.motion_rmse(),
        r.noise_guard.micro_lost,
        r.noise_guard.wrong_direction
    );
    println!(
        "RAW neutral={:.8} motion={:.8}; FIXED neutral={:.8} motion={:.8}; ORACLE motion={:.8}",
        r.raw.neutral_rmse(),
        r.raw.motion_rmse(),
        r.fixed.neutral_rmse(),
        r.fixed.motion_rmse(),
        r.oracle.motion_rmse()
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_and_grid_unique() {
        assert_eq!(GRID_CASES, 69120);
        assert_eq!(Spec::grid(0, 42).amplitude, 0.00001);
        assert_eq!(Spec::grid(GRID_CASES - 1, 42).amplitude, 1.0);
        assert_eq!(Spec::grid(GRID_CASES - 1, 42).intent, 8);
        assert_eq!(Spec::grid(GRID_CASES - 1, 42).transition, 7);
    }
    #[test]
    fn partition_micro_loss_by_isolated_report_resolution() {
        let mut score = Score::default();
        score.update_with_resolution(Vec2::new(0.00001, 0.), Vec2::ZERO, false);
        score.update_with_resolution(Vec2::new(0.002, 0.), Vec2::ZERO, true);
        assert_eq!(score.micro_lost, 2);
        assert_eq!(score.micro_lost_above_resolution, 1);
        assert_eq!(score.micro_lost_below_resolution, 1);
    }
    #[test]
    fn sub_report_micro_motion_disappears_before_filter() {
        let spec = Spec {
            amplitude: 0.00001,
            direction: 0.0,
            transition: 0,
            intent: 1,
            noise: 0.0,
            seed: 42,
            case_id: 7,
        };
        let ideal = evaluate_with_resolution(spec, 1024, Resolution::Ideal);
        let digitized = evaluate_with_resolution(spec, 1024, Resolution::XInput16);
        assert_eq!(ideal.below_report_resolution, 0);
        assert!(digitized.below_report_resolution > 600);
        assert!(digitized.quantization_changed > 0);
        // The diagnostic oracle cannot reconstruct the lost sub-LSB motion
        // without privileged knowledge of the player's desired movement.
        assert!(digitized.oracle.motion_rmse() > ideal.oracle.motion_rmse());
    }
    #[test]
    fn sign_only_direction_metric_exposes_amplitude_suppression() {
        let desired = Vec2::new(0.01, 0.0);
        let mut unattenuated = Score::default();
        let mut attenuated = Score::default();
        unattenuated.update(desired, Vec2::new(-0.002, 0.0));
        attenuated.update(desired, Vec2::new(-0.00002, 0.0));
        assert_eq!(unattenuated.sign_opposite, 1);
        assert_eq!(
            attenuated.sign_opposite, 1,
            "shrinking output must not hide opposite physical direction"
        );
        assert_eq!(unattenuated.wrong_direction, 1);
        assert_eq!(
            attenuated.wrong_direction, 0,
            "older magnitude-dependent counter can be gamed by attenuation"
        );
    }
    #[test]
    fn same_seed_same_metrics() {
        let s = Spec::randomized(312, 824);
        let a = evaluate(s, 1600);
        let b = evaluate(s, 1600);
        assert_eq!(a.guard.neutral_sq, b.guard.neutral_sq);
        assert_eq!(a.guard.motion_sq, b.guard.motion_sq);
    }
    #[test]
    fn a_constant_zero_output_is_penalized_for_real_motion() {
        let mut s = Score::default();
        for _ in 0..100 {
            s.update(Vec2::new(0.9, 0.0), Vec2::ZERO);
        }
        assert!(s.motion_rmse() > 0.89);
        assert_eq!(s.micro_lost, 100);
        assert_eq!(s.sign_opposite, 0);
    }
    #[test]
    fn perfect_calibration_cannot_reconstruct_clipped_signal() {
        let r = evaluate(
            Spec {
                amplitude: 1.0,
                direction: 0.0,
                transition: 0,
                intent: 6,
                noise: 0.0,
                seed: 1,
                case_id: 1,
            },
            1800,
        );
        assert!(r.oracle.motion_rmse() > 0.1);
    }
}
