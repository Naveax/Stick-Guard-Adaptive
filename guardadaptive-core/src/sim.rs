//! Repeatable adversarial stick drift laboratory. Truth is NEVER passed to GuardAdaptive.
//! All amplitudes are normalized (0.9 = 90% full-axis).
use crate::{fixed_deadzone, Config, GuardAdaptive, InputMode, Vec2};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Instant,
};

#[derive(Clone, Copy, Debug)]
pub enum Scenario {
    Tiny,
    Stable20,
    Stable90,
    Ramp01To90,
    HardSwitch,
    Flip90,
    Brownian,
    Snapback,
    Directional,
    Saturation,
    MicroIntent,
    AllChaos,
    RandomGauntlet,
}
impl Scenario {
    pub const ALL: [Self; 13] = [
        Self::Tiny,
        Self::Stable20,
        Self::Stable90,
        Self::Ramp01To90,
        Self::HardSwitch,
        Self::Flip90,
        Self::Brownian,
        Self::Snapback,
        Self::Directional,
        Self::Saturation,
        Self::MicroIntent,
        Self::AllChaos,
        Self::RandomGauntlet,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Tiny => "tiny_0_1_percent",
            Self::Stable20 => "stable_20_percent",
            Self::Stable90 => "stable_90_percent",
            Self::Ramp01To90 => "ramp_0_1_to_90",
            Self::HardSwitch => "instant_0_1_90",
            Self::Flip90 => "flip_90_percent",
            Self::Brownian => "colored_noise",
            Self::Snapback => "snapback_impulses",
            Self::Directional => "directional_hysteresis",
            Self::Saturation => "adc_saturation",
            Self::MicroIntent => "held_micro_intent",
            Self::AllChaos => "combined_adversarial",
            Self::RandomGauntlet => "random_gauntlet",
        }
    }
}
#[derive(Clone, Copy)]
pub struct Sample {
    pub raw: Vec2,
    pub desired: Vec2,     // test evaluator only; never provided to the guard
    pub actual_bias: Vec2, // oracle only; never provided to the guard
    pub neutral: bool,     // test evaluator only; never provided to the guard in live mode
}
#[derive(Clone)]
struct Rng {
    state: u64,
}
impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9e3779b97f4a7c15 } else { seed },
        }
    }
    fn u64(&mut self) -> u64 {
        let mut s = self.state;
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        self.state = s;
        s.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn unit(&mut self) -> f64 {
        ((self.u64() >> 11) as f64) / ((1_u64 << 53) as f64)
    }
    fn signed(&mut self) -> f64 {
        2.0 * self.unit() - 1.0
    }
}

fn bias(s: Scenario, t: usize, n: usize) -> Vec2 {
    let p = t as f64 / n.max(1) as f64;
    match s {
        Scenario::Tiny => Vec2::new(0.001, -0.0005),
        Scenario::Stable20 => Vec2::new(0.20, -0.08),
        Scenario::Stable90 => Vec2::new(0.90, 0.),
        Scenario::Ramp01To90 => Vec2::new(0.001 + 0.899 * p, 0.07 * p),
        Scenario::HardSwitch => Vec2::new(
            if (t / 1200).is_multiple_of(2) {
                0.001
            } else {
                0.90
            },
            0.,
        ),
        Scenario::Flip90 => Vec2::new(
            if (t / 900).is_multiple_of(2) {
                0.90
            } else {
                -0.90
            },
            0.005,
        ),
        Scenario::Brownian => Vec2::new(0.06 + 0.05 * (p * 34.0).sin(), 0.03 * (p * 55.0).cos()),
        Scenario::Snapback => Vec2::new(0.12, -0.04),
        Scenario::Directional => Vec2::new(0.08, 0.03),
        Scenario::Saturation => Vec2::new(0.90, 0.35),
        Scenario::MicroIntent => Vec2::new(0.005, 0.),
        Scenario::RandomGauntlet => Vec2::new(0.001, 0.),
        Scenario::AllChaos => Vec2::new(
            if (t / 2000).is_multiple_of(2) {
                0.02 + 0.88 * (p * 7.).sin().abs()
            } else {
                -0.82
            },
            0.1 * (p * 120.).sin(),
        ),
    }
}
fn desired_input(s: Scenario, t: usize) -> Vec2 {
    let phase = t % 1500;
    if matches!(s, Scenario::MicroIntent) {
        return if phase < 250 {
            Vec2::ZERO
        } else if phase < 1000 {
            Vec2::new(0.011, 0.0)
        } else {
            Vec2::new(0.62, -0.12)
        };
    }
    match phase {
        0..=399 => Vec2::ZERO,
        400..=699 => {
            let z = (phase - 400) as f64 / 300.;
            Vec2::new(0.012 + 0.028 * (z * 8.).sin(), 0.015 * (z * 3.).cos())
        }
        700..=1149 => {
            let z = (phase - 700) as f64 / 450.;
            Vec2::new(0.65 * (z * 8.).sin(), 0.52 * (z * 6.).cos())
        }
        _ => Vec2::ZERO,
    }
}
fn generate_live(s: Scenario, n: usize, seed: u64, trained_bias: Vec2) -> Vec<Sample> {
    let mut rng = Rng::new(seed ^ 0x4bc54dab8912d5a7);
    let mut correlated = Vec2::ZERO;
    let mut prev_desired = Vec2::ZERO;
    let mut attack_bias = Vec2::new(0.001, 0.0);
    let mut attack_target = attack_bias;
    let mut segment_until = 0_usize;
    let mut attack_rate = 0.0_f64;
    let mut samples = Vec::with_capacity(n);
    for t in 0..n {
        let desired = desired_input(s, t);
        let mut b = bias(s, t, n);
        if matches!(s, Scenario::RandomGauntlet) {
            if t >= segment_until {
                // Seeded red-team bot selects a new step, ramp, oscillation or
                // high-frequency attack. Magnitudes span 0.1% to 90%.
                let mag = (0.001_f64.ln() + rng.unit() * (0.9_f64.ln() - 0.001_f64.ln())).exp();
                let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                let angle = rng.unit() * std::f64::consts::TAU;
                attack_target = Vec2::new(sign * mag * angle.cos(), mag * angle.sin());
                attack_rate = match rng.u64() % 4 {
                    0 => 1.0,
                    1 => 0.3,
                    2 => 0.05,
                    _ => 0.003,
                };
                segment_until = t + 1 + (rng.u64() % 900) as usize;
            }
            attack_bias = attack_bias + (attack_target - attack_bias) * attack_rate;
            b = attack_bias;
        }
        if matches!(s, Scenario::Directional) {
            // Direction-dependent mechanical hysteresis after release.
            b.x += if prev_desired.x >= 0. { 0.025 } else { -0.025 };
            b.y += if prev_desired.y >= 0. { 0.018 } else { -0.018 };
        }
        if matches!(s, Scenario::Snapback | Scenario::AllChaos) {
            let phase = t % 1500;
            if (1150..1190).contains(&phase) {
                let shock = (1.0 - (phase - 1150) as f64 / 40.).powi(2);
                b.x += 0.35 * shock;
                b.y -= 0.22 * shock;
            }
        }
        if matches!(s, Scenario::AllChaos | Scenario::RandomGauntlet) && t % 1100 < 4 {
            b.x -= 0.75;
        }
        let jitter = if matches!(
            s,
            Scenario::Brownian | Scenario::AllChaos | Scenario::RandomGauntlet
        ) {
            0.025
        } else {
            0.0015
        };
        correlated = correlated * 0.92 + Vec2::new(rng.signed(), rng.signed()) * (jitter * 0.08);
        let iid = Vec2::new(rng.signed(), rng.signed()) * jitter;
        let raw = (desired + b + correlated + iid).clamp();
        samples.push(Sample {
            raw,
            desired,
            actual_bias: b,
            neutral: desired.norm() < 1e-12,
        });
        prev_desired = desired;
    }
    // Makes the intent of the API explicit: measured bias is ONLY evaluator metadata.
    let _ = trained_bias;
    samples
}
fn calibration(s: Scenario, seed: u64) -> (GuardAdaptive, Vec2) {
    let mut g = GuardAdaptive::new(Config::default());
    let mut rng = Rng::new(seed ^ 0x91ef029ab144ed12);
    let initial = bias(s, 0, 10000);
    for _ in 0..700 {
        let noise = Vec2::new(rng.signed(), rng.signed()) * 0.001;
        g.process((initial + noise).clamp(), InputMode::CalibrateNeutral);
    }
    g.begin_live();
    (g, initial)
}

#[derive(Default, Clone, Copy)]
struct Accumulator {
    neutral_ss: f64,
    movement_ss: f64,
    neutral_max: f64,
    movement_max: f64,
    neutral_count: usize,
    movement_count: usize,
    wrong_direction: usize,
    neutral_leak_over_1pc: usize,
    neutral_leak_over_10pc: usize,
    movement_magnitude: f64,
    result_magnitude: f64,
}
impl Accumulator {
    fn record(&mut self, output: Vec2, s: Sample) {
        let err = (output - s.desired).norm();
        if s.neutral {
            self.neutral_count += 1;
            self.neutral_ss += err * err;
            self.neutral_max = self.neutral_max.max(err);
            if err > 0.01 {
                self.neutral_leak_over_1pc += 1;
            }
            if err > 0.10 {
                self.neutral_leak_over_10pc += 1;
            }
        } else {
            self.movement_count += 1;
            self.movement_ss += err * err;
            self.movement_max = self.movement_max.max(err);
            self.movement_magnitude += s.desired.norm();
            self.result_magnitude += output.norm();
            if output.x * s.desired.x + output.y * s.desired.y < -0.0001 {
                self.wrong_direction += 1;
            }
        }
    }
    fn neutral_rmse(self) -> f64 {
        (self.neutral_ss / self.neutral_count.max(1) as f64).sqrt()
    }
    fn motion_rmse(self) -> f64 {
        (self.movement_ss / self.movement_count.max(1) as f64).sqrt()
    }
}
#[derive(Clone)]
pub struct ReportRow {
    pub scenario: &'static str,
    pub method: &'static str,
    pub neutral_rmse: f64,
    pub motion_rmse: f64,
    pub neutral_max: f64,
    pub motion_max: f64,
    pub neutral_over_10pc: f64,
    pub wrong_direction: usize,
    pub updates: u64,
}
fn row(s: Scenario, method: &'static str, a: Accumulator, updates: u64) -> ReportRow {
    ReportRow {
        scenario: s.name(),
        method,
        neutral_rmse: a.neutral_rmse(),
        motion_rmse: a.motion_rmse(),
        neutral_max: a.neutral_max,
        motion_max: a.movement_max,
        neutral_over_10pc: a.neutral_leak_over_10pc as f64 / a.neutral_count.max(1) as f64,
        wrong_direction: a.wrong_direction,
        updates,
    }
}
/// Export a replay trace with ground truth used ONLY by the evaluator.
pub fn export_trace(seed: u64, n: usize, scenario_name: &str, out: &Path) -> io::Result<()> {
    let scenario = Scenario::ALL
        .iter()
        .copied()
        .find(|s| s.name() == scenario_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unknown scenario"))?;
    let (mut guard, trained_bias) = calibration(scenario, seed);
    let samples = generate_live(scenario, n.max(1500), seed, trained_bias);
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = fs::File::create(out)?;
    writeln!(f,"t,raw_x,raw_y,intended_x,intended_y,sim_bias_x,sim_bias_y,corrected_x,corrected_y,learned_center_x,learned_center_y,deadzone,confidence,neutral_truth")?;
    for (i, s) in samples.into_iter().enumerate() {
        let output = guard.process(s.raw, InputMode::Live);
        let tm = guard.telemetry();
        writeln!(
            f,
            "{},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.7},{:.5},{}",
            i,
            s.raw.x,
            s.raw.y,
            s.desired.x,
            s.desired.y,
            s.actual_bias.x,
            s.actual_bias.y,
            output.x,
            output.y,
            tm.center.x,
            tm.center.y,
            tm.deadzone,
            tm.neutral_confidence,
            s.neutral
        )?;
    }
    println!(
        "TRACE: scenario={} seed={} samples={} CSV={}",
        scenario.name(),
        seed,
        n.max(1500),
        out.display()
    );
    Ok(())
}
pub fn evaluate(seed: u64, n: usize) -> Vec<ReportRow> {
    let mut rows = Vec::with_capacity(Scenario::ALL.len() * 4);
    for (index, sc) in Scenario::ALL.iter().copied().enumerate() {
        let (mut guard, trained_bias) = calibration(sc, seed + index as u64);
        let mut fixed = Accumulator::default();
        let mut adaptive = Accumulator::default();
        let mut raw_acc = Accumulator::default();
        let mut oracle = Accumulator::default();
        let samples = generate_live(sc, n, seed + index as u64, trained_bias);
        for sample in samples {
            raw_acc.record(sample.raw, sample);
            fixed.record(fixed_deadzone(sample.raw, 0.05), sample);
            adaptive.record(guard.process(sample.raw, InputMode::Live), sample);
            // Perfect knowledge of bias gives an optimistic baseline.
            // Saturation and sensor noise remain irrecoverable.
            oracle.record((sample.raw - sample.actual_bias).saturate(), sample);
        }
        rows.push(row(sc, "raw", raw_acc, 0));
        rows.push(row(sc, "deadzone_5pc", fixed, 0));
        rows.push(row(
            sc,
            "guardadaptive",
            adaptive,
            guard.telemetry().auto_updates,
        ));
        rows.push(row(sc, "oracle_bias_known", oracle, 0));
    }
    rows
}
pub fn run(seed: u64, n: usize, out: &Path) -> io::Result<Vec<ReportRow>> {
    let n = n.max(1500);
    let rows = evaluate(seed, n);
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = fs::File::create(out)?;
    writeln!(f,"scenario,method,neutral_rmse,motion_rmse,neutral_max,motion_max,neutral_over_10pc,wrong_direction,auto_updates")?;
    for r in &rows {
        writeln!(
            f,
            "{},{},{:0.8},{:0.8},{:0.8},{:0.8},{:0.6},{},{}",
            r.scenario,
            r.method,
            r.neutral_rmse,
            r.motion_rmse,
            r.neutral_max,
            r.motion_max,
            r.neutral_over_10pc,
            r.wrong_direction,
            r.updates
        )?;
    }
    println!(
        "seed={seed}, samples_per_scenario={n}, scenario_count={}, CSV={}",
        Scenario::ALL.len(),
        out.display()
    );
    println!(
        "{:<24} {:<18} {:>12} {:>12} {:>12}",
        "SCENARIO", "METHOD", "NEUTRAL_RMS", "MOTION_RMS", "NEUTRAL_MAX"
    );
    for r in &rows {
        if r.method == "guardadaptive" || r.method == "deadzone_5pc" {
            println!(
                "{:<24} {:<18} {:>12.5} {:>12.5} {:>12.5}",
                r.scenario, r.method, r.neutral_rmse, r.motion_rmse, r.neutral_max
            );
        }
    }
    println!(
        "NOTE: 90% abrupt unknown drift is not distinguishable from an intentional stick press."
    );
    println!("NOTE: oracle uses privileged simulated bias ONLY as a diagnostic reference, never as Guard input.");
    Ok(rows)
}
/// Automated seeded adversary: run every scenario across independent seeds,
/// rank the worst blind-correction failures and retain every score as CSV.
/// No reward signal from the evaluator enters the GuardAdaptive live engine.
pub fn attack_search(seed: u64, trials: usize, samples: usize, out: &Path) -> io::Result<()> {
    let trials = trials.clamp(1, 10000);
    let mut scores: Vec<(f64, u64, &'static str, f64, f64)> = Vec::new();
    if let Some(p) = out.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = fs::File::create(out)?;
    writeln!(
        f,
        "seed,scenario,score,neutral_rmse,motion_rmse,raw_neutral_rmse,oracle_motion_rmse"
    )?;
    for run in 0..trials {
        let trial_seed = seed.wrapping_add(run as u64);
        let rows = evaluate(trial_seed, samples.max(1500));
        for scenario in Scenario::ALL {
            let scenario_rows: Vec<_> = rows
                .iter()
                .filter(|r| r.scenario == scenario.name())
                .collect();
            let guard = scenario_rows
                .iter()
                .find(|r| r.method == "guardadaptive")
                .unwrap();
            let raw = scenario_rows.iter().find(|r| r.method == "raw").unwrap();
            let oracle = scenario_rows
                .iter()
                .find(|r| r.method == "oracle_bias_known")
                .unwrap();
            // Combined objective punishes failed neutral and distortion of actual intent.
            let score = guard.neutral_rmse + 0.75 * guard.motion_rmse;
            writeln!(
                f,
                "{},{},{:.8},{:.8},{:.8},{:.8},{:.8}",
                trial_seed,
                scenario.name(),
                score,
                guard.neutral_rmse,
                guard.motion_rmse,
                raw.neutral_rmse,
                oracle.motion_rmse
            )?;
            scores.push((
                score,
                trial_seed,
                scenario.name(),
                guard.neutral_rmse,
                guard.motion_rmse,
            ));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!(
        "RED-TEAM FUZZ: trials={} samples/trial/scenario={} attacks={} CSV={}",
        trials,
        samples.max(1500),
        scores.len(),
        out.display()
    );
    for (rank, (score, seed, scenario, neutral, motion)) in scores.iter().take(15).enumerate() {
        println!(
            "#{:02} seed={} {:<25} score={:.5} neutral_rmse={:.5} motion_rmse={:.5}",
            rank + 1,
            seed,
            scenario,
            score,
            neutral,
            motion
        );
    }
    Ok(())
}
pub fn benchmark(samples: usize) -> f64 {
    let mut g = GuardAdaptive::new(Config::default());
    for _ in 0..500 {
        g.process(Vec2::new(0.05, 0.), InputMode::CalibrateNeutral);
    }
    g.begin_live();
    let now = Instant::now();
    for t in 0..samples {
        let v = Vec2::new(
            (t as f64 * 0.00073).sin() * 0.4,
            (t as f64 * 0.00113).cos() * 0.4,
        );
        std::hint::black_box(g.process(std::hint::black_box(v), InputMode::Live));
    }
    now.elapsed().as_secs_f64() * 1e9 / samples.max(1) as f64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simulator_reproducible() {
        let x = generate_live(Scenario::AllChaos, 2000, 1337, Vec2::ZERO);
        let y = generate_live(Scenario::AllChaos, 2000, 1337, Vec2::ZERO);
        assert_eq!(x[1000].raw, y[1000].raw);
        assert_eq!(x[1999].desired, y[1999].desired);
    }
    #[test]
    fn impossible_shift_is_reported_not_magically_fixed() {
        let rows = evaluate(42, 6000);
        let r = rows
            .iter()
            .find(|r| r.scenario == "instant_0_1_90" && r.method == "guardadaptive")
            .unwrap();
        assert!(r.neutral_rmse > 0.1);
    }
    #[test]
    fn stable_large_drift_benefits_from_explicit_calibration() {
        let rows = evaluate(77, 6000);
        let a = rows
            .iter()
            .find(|r| r.scenario == "stable_90_percent" && r.method == "guardadaptive")
            .unwrap();
        let b = rows
            .iter()
            .find(|r| r.scenario == "stable_90_percent" && r.method == "raw")
            .unwrap();
        assert!(
            a.neutral_rmse < b.neutral_rmse * 0.1,
            "guard={} raw={}",
            a.neutral_rmse,
            b.neutral_rmse
        );
    }
    #[test]
    fn ground_truth_never_passed_as_mode_in_evaluation() {
        let src = include_str!("sim.rs");
        let live_section = src
            .split("pub fn evaluate(")
            .nth(1)
            .unwrap()
            .split("pub fn run(")
            .next()
            .unwrap();
        assert!(live_section.contains("InputMode::Live"));
        assert!(!live_section.contains("InputMode::CalibrateNeutral"));
    }
}
