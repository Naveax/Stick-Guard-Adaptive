//! v0.24: deterministic, *synthetic-only* known-intent evaluation.
//! The oracle is visible exclusively to scoring. Models see only raw
//! XInput-like observations and a separately declared neutral calibration.
use crate::{
    causal_guard::{CausalGuard, GuardConfig},
    confidence_direction::ConfidenceDirection,
    direction_guard::DirectionGuard,
    hybrid::{HybridConfig, HybridGuard},
    motion_first::{MotionFirst, MotionFirstConfig},
    reacquire::VerifiedCenter,
    resolution::Resolution,
    InputMode, Vec2,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static TEMP: AtomicU64 = AtomicU64::new(0);
const LSB: f64 = 1.0 / 32767.0;
const NAMES: [&str; 6] = [
    "center_only",
    "motion_first",
    "hybrid",
    "direction_guard",
    "confidence_direction_rejected_v015",
    "causal_bounded_v028_research_only",
];
#[derive(Clone, Copy)]
struct Frame {
    desired: Vec2,
    observed: Vec2,
    is_micro: bool,
    is_reversal: bool,
}
#[derive(Clone, Copy)]
struct Rng {
    state: u64,
}
impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0xA076_1D64_78BD_642F,
        }
    }
    fn next(&mut self) -> f64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let n = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((n >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn noise(&mut self) -> f64 {
        self.next() * 2.0 - 1.0
    }
}
enum Candidate {
    Center,
    Motion(MotionFirst),
    Hybrid(HybridGuard),
    Direction(DirectionGuard),
    Confidence(ConfidenceDirection),
    Causal(CausalGuard),
}
impl Candidate {
    fn process(&mut self, input: Vec2, center: Vec2) -> Vec2 {
        match self {
            Self::Center => (input - center).saturate(),
            Self::Motion(g) => g.process(input, InputMode::Live),
            Self::Hybrid(g) => g.process(input),
            Self::Direction(g) => g.process(input),
            Self::Confidence(g) => g.process(input),
            Self::Causal(g) => g.process(input),
        }
    }
}
fn build(center: Vec2) -> [Candidate; 6] {
    let trusted = VerifiedCenter::restore_checked(center, Vec2::new(0.00002, 0.00002), 0.00002)
        .expect("bounded lab-only calibration");
    let mut motion = MotionFirst::new(MotionFirstConfig {
        input_lsb: LSB,
        softness_lsb: 0.5,
    });
    motion.accept_reacquisition(trusted);
    let cfg = HybridConfig::default();
    let mut hybrid = HybridGuard::new(cfg);
    hybrid.accept_verified(trusted);
    let mut direction = DirectionGuard::new(cfg);
    direction.accept_verified(trusted);
    let mut confidence = ConfidenceDirection::new(cfg);
    confidence.accept_verified(trusted);
    let mut causal = CausalGuard::new(GuardConfig::default());
    causal.accept_verified(trusted);
    [
        Candidate::Center,
        Candidate::Motion(motion),
        Candidate::Hybrid(hybrid),
        Candidate::Direction(direction),
        Candidate::Confidence(confidence),
        Candidate::Causal(causal),
    ]
}
fn sequence(i: usize, rng: &mut Rng, center: Vec2) -> Frame {
    let phase = i % 600;
    let episode = (i / 600) % 6;
    let desired = match episode {
        0 => Vec2::ZERO,
        1 => Vec2::new(0.0001 * if phase < 300 { 1.0 } else { -1.0 }, 0.0),
        2 => Vec2::new(0.2 * if phase < 300 { 1.0 } else { -1.0 }, 0.0),
        3 => Vec2::new(0.7 * (phase as f64 / 599.0), -0.5 * (phase as f64 / 599.0)),
        4 => Vec2::new(
            if phase < 300 { 0.0 } else { 0.65 },
            if phase < 300 { 0.0 } else { -0.4 },
        ),
        _ => Vec2::new(
            0.005 * (phase as f64 / 40.0).sin(),
            0.005 * (phase as f64 / 53.0).cos(),
        ),
    };
    let noise_level = match (i / 3600) % 4 {
        0 => 0.00002,
        1 => 0.0005,
        2 => 0.005,
        _ => 0.03,
    };
    let jitter = Vec2::new(rng.noise() * noise_level, rng.noise() * noise_level);
    // Varying offset is a synthetic drift, NEVER ground truth provided
    // to a filter; the engine gets only the fixed consented center.
    let drift = match (i / 2400) % 3 {
        0 => Vec2::ZERO,
        1 => Vec2::new(0.03, 0.0),
        _ => Vec2::new(-0.10, 0.055),
    };
    let measured = Resolution::XInput16.quantize((center + desired + drift + jitter).clamp());
    Frame {
        desired,
        observed: measured,
        is_micro: desired.norm() > LSB && desired.norm() <= 0.01,
        is_reversal: (episode == 1 || episode == 2) && phase == 300,
    }
}
#[derive(Default, Clone, Copy)]
pub struct Score {
    pub total: u64,
    pub squared: f64,
    pub neutral_squared: f64,
    pub neutral: u64,
    pub micro_total: u64,
    pub micro_lost: u64,
    pub micro_wrong: u64,
    pub wrong_direction: u64,
    pub reversal_total: u64,
    pub reversal_wrong: u64,
    pub core_ns: u128,
}
impl Score {
    fn observe(&mut self, f: Frame, out: Vec2, elapsed: u128) {
        self.total += 1;
        self.squared += (out - f.desired).length_squared();
        self.core_ns += elapsed;
        if f.desired == Vec2::ZERO {
            self.neutral += 1;
            self.neutral_squared += out.length_squared();
        }
        if f.is_micro {
            self.micro_total += 1;
            if out.norm() < 0.5 * f.desired.norm() {
                self.micro_lost += 1;
            }
            if out.x * f.desired.x + out.y * f.desired.y < 0.0 {
                self.micro_wrong += 1;
            }
        }
        if f.desired.norm() > LSB
            && out.norm() > LSB
            && out.x * f.desired.x + out.y * f.desired.y < 0.0
        {
            self.wrong_direction += 1;
        }
        if f.is_reversal {
            self.reversal_total += 1;
            if out.x * f.desired.x + out.y * f.desired.y <= 0.0 {
                self.reversal_wrong += 1;
            }
        }
    }
    fn json(&self) -> String {
        format!(
            concat!(
                "{{\"samples\":{},\"rmse_vs_synthetic_truth\":{:.10},",
                "\"neutral_rmse\":{:.10},\"micro_total\":{},\"micro_lost\":{},",
                "\"micro_wrong\":{},\"wrong_direction\":{},",
                "\"reversal_total\":{},\"reversal_wrong\":{},",
                "\"core_compute_ns\":{}}}"
            ),
            self.total,
            (self.squared / self.total as f64).sqrt(),
            if self.neutral > 0 {
                (self.neutral_squared / self.neutral as f64).sqrt()
            } else {
                0.0
            },
            self.micro_total,
            self.micro_lost,
            self.micro_wrong,
            self.wrong_direction,
            self.reversal_total,
            self.reversal_wrong,
            self.core_ns
        )
    }
}
pub struct ResultSet {
    pub seed: u64,
    pub frames: usize,
    pub scores: [Score; 6],
}
pub fn evaluate(seed: u64, count: usize) -> io::Result<ResultSet> {
    if !(3600..=1_000_000).contains(&count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be 3600..1000000",
        ));
    }
    let center = Vec2::new(0.095, -0.04);
    let mut rng = Rng::new(seed);
    let mut candidates = build(center);
    let mut scores = [Score::default(); 6];
    for i in 0..count {
        let f = sequence(i, &mut rng, center);
        for (index, engine) in candidates.iter_mut().enumerate() {
            let tick = Instant::now();
            let out = engine.process(f.observed, center);
            scores[index].observe(f, out, tick.elapsed().as_nanos());
        }
    }
    Ok(ResultSet {
        seed,
        frames: count,
        scores,
    })
}
impl ResultSet {
    pub fn json(&self) -> String {
        let parts = NAMES
            .iter()
            .zip(self.scores.iter())
            .map(|(name, score)| format!("\"{name}\":{}", score.json()))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"deterministic_synthetic_oracle\",",
                "\"seed\":{},\"samples\":{},\"resolution\":\"xinput16\",",
                "\"models\":{{{}}},",
                "\"true_intent_known_only_in_simulation\":true,",
                "\"real_controller_tested\":false,",
                "\"game_latency_measured\":false,\"production_qualified\":false}}\n"
            ),
            self.seed, self.frames, parts
        )
    }
}
/// v0.28: paired per-regime diagnosis. Nothing below is an algorithm
/// training signal, and simulator truth is available ONLY to metrics.
const REGIMES: [&str; 6] = [
    "true_neutral",
    "micro_reversal",
    "large_reversal",
    "analog_ramp",
    "step_flick",
    "small_wave",
];
pub struct RegimeDiagnosis {
    pub seed: u64,
    pub total: usize,
    pub episodes: [[Score; 6]; 6],
}
impl RegimeDiagnosis {
    pub fn json(&self) -> String {
        let episodes = REGIMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let entries = NAMES
                    .iter()
                    .enumerate()
                    .map(|(j, candidate)| format!("\"{candidate}\":{}", self.episodes[i][j].json()))
                    .collect::<Vec<_>>()
                    .join(",");
                let base = &self.episodes[i][0];
                let novel = &self.episodes[i][5];
                let improvement = if base.squared > 0.0 {
                    1.0 - (novel.squared / base.squared).sqrt()
                } else {
                    0.0
                };
                format!(
                    concat!(
                        "\"{}\":{{\"samples_per_candidate\":{},",
                        "\"causal_v028_rmse_change_vs_center_only\":{:.10},",
                        "\"scores\":{{{}}}}}"
                    ),
                    name, base.total, improvement, entries
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"synthetic_per_regime_holdout\",",
                "\"seed\":{},\"total_frames\":{},\"regimes\":{{{}}},",
                "\"identifiability_counterexample\":{{",
                "\"same_observation_can_be_neutral_with_drift_or_intentional_motion\":true,",
                "\"first_truth\":{{\"desired\":[0,0],\"drift\":[0.5,0]}},",
                "\"second_truth\":{{\"desired\":[0.5,0],\"drift\":[0,0]}},",
                "\"shared_report_after_center\":[0.5,0],",
                "\"minimum_worst_case_directional_absolute_error\":0.25,",
                "\"neither_scenario_identifiable_from_stick_report_alone\":true}},",
                "\"physical_controller_evidence\":false,",
                "\"ground_truth_labels_are_simulator_only\":true,",
                "\"candidate_automatically_promoted\":false,",
                "\"production_quality\":\"BLOCKED\"}}\n"
            ),
            self.seed, self.total, episodes
        )
    }
}
pub fn diagnose_regimes(seed: u64, count: usize) -> io::Result<RegimeDiagnosis> {
    if !(3600..=1_000_000).contains(&count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be 3600..1000000",
        ));
    }
    let center = Vec2::new(0.095, -0.04);
    let mut rng = Rng::new(seed);
    let mut engines = build(center);
    let mut buckets = [[Score::default(); 6]; 6];
    for i in 0..count {
        let frame = sequence(i, &mut rng, center);
        let regime = (i / 600) % 6;
        for (j, engine) in engines.iter_mut().enumerate() {
            let t = Instant::now();
            let output = engine.process(frame.observed, center);
            buckets[regime][j].observe(frame, output, t.elapsed().as_nanos());
        }
    }
    Ok(RegimeDiagnosis {
        seed,
        total: count,
        episodes: buckets,
    })
}
pub fn diagnose_command(seed: u64, count: usize, out: &Path) -> io::Result<()> {
    let diagnosis = diagnose_regimes(seed, count)?;
    save_new(out, &diagnosis.json())?;
    println!("V028 PER-REGIME SYNTHETIC DIAGNOSIS seed={seed} frames={count}");
    for (name, regime) in REGIMES.iter().zip(diagnosis.episodes.iter()) {
        let baseline = (regime[0].squared / regime[0].total as f64).sqrt();
        let candidate = (regime[5].squared / regime[5].total as f64).sqrt();
        println!("{name}: baseline_rmse={baseline:.8} new_candidate_rmse={candidate:.8} candidate_micro_loss={} candidate_wrong_direction={}",
            regime[5].micro_lost,regime[5].wrong_direction);
    }
    println!("UNIDENTIFIABLE INPUT EXAMPLE: shared centered raw +0.5 may be 0 intended+0.5 drift or +0.5 intended+0 drift. Worst-case absolute error lower bound 0.25.");
    println!("RESEARCH ONLY. NO AUTOMATIC PROMOTION. {}", out.display());
    Ok(())
}
pub fn save_new(path: &Path, bytes: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing basename"))?;
    let tmp = parent.join(format!(
        ".{}.{}.{}.truth-tmp",
        name.to_string_lossy(),
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created = false;
    let res = (|| -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        created = true;
        f.write_all(bytes.as_bytes())?;
        f.sync_all()?;
        drop(f);
        crate::capture::publish_new(&tmp, path)
    })();
    if res.is_err() && created {
        let _ = fs::remove_file(&tmp);
    }
    res
}
pub fn run(seed: u64, samples: usize, out: &Path) -> io::Result<()> {
    let result = evaluate(seed, samples)?;
    save_new(out, &result.json())?;
    println!(
        "SYNTHETIC ORACLE RUN: seed={} frames={} 6 candidates",
        seed, samples
    );
    for (name, s) in NAMES.iter().zip(result.scores.iter()) {
        println!(
            "{name}: RMSE={:.6} micro_lost={} wrong_direction={} reversals_wrong={}",
            (s.squared / s.total as f64).sqrt(),
            s.micro_lost,
            s.wrong_direction,
            s.reversal_wrong
        );
    }
    println!(
        "NOT physical controller evidence, NOT game latency, NOT production acceptance. {}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_truth_is_independent_of_candidate_predictions() {
        let a = evaluate(20261024, 7200).unwrap();
        let b = evaluate(20261024, 7200).unwrap();
        for (x, y) in a.scores.iter().zip(b.scores.iter()) {
            assert_eq!(x.total, y.total);
            assert_eq!(x.squared, y.squared);
            assert_eq!(x.micro_lost, y.micro_lost);
            assert_eq!(x.wrong_direction, y.wrong_direction);
        }
        assert!(a.scores[0].micro_total > 0);
        assert!(a.scores[0].wrong_direction > 0);
        assert!(a.json().contains("\"real_controller_tested\":false"));
    }
    #[test]
    fn same_fixed_truth_for_each_candidate() {
        let x = evaluate(6, 7200).unwrap();
        assert!(x.scores.iter().all(|s| s.total == 7200));
        assert!(x
            .scores
            .iter()
            .all(|s| s.micro_total == x.scores[0].micro_total));
        assert!(x
            .scores
            .iter()
            .all(|s| s.reversal_total == x.scores[0].reversal_total));
    }
    #[test]
    fn malformed_sample_limits_are_rejected() {
        for n in [0, 3599, 1_000_001] {
            assert!(evaluate(3, n).is_err());
        }
    }
    #[test]
    fn per_regime_diagnostics_cover_identical_paired_frames() {
        let d = diagnose_regimes(20261028, 72000).unwrap();
        assert_eq!(d.episodes.iter().map(|b| b[0].total).sum::<u64>(), 72000);
        for bucket in &d.episodes {
            assert!(bucket[0].total > 0);
            assert!(bucket
                .iter()
                .all(|candidate| candidate.total == bucket[0].total));
        }
        let json = d.json();
        assert!(json.contains("\"minimum_worst_case_directional_absolute_error\":0.25"));
        assert!(json.contains("\"neither_scenario_identifiable_from_stick_report_alone\":true"));
        assert!(json.contains("\"candidate_automatically_promoted\":false"));
    }
    #[test]
    fn publish_is_nonreplacing_and_preserves_existing_bytes() {
        let p = std::env::temp_dir().join(format!("guardadaptive-v24-{}.json", std::process::id()));
        let _ = fs::remove_file(&p);
        let r = evaluate(20261024, 7200).unwrap();
        save_new(&p, &r.json()).unwrap();
        let original = fs::read(&p).unwrap();
        assert!(save_new(&p, &r.json()).is_err());
        assert_eq!(fs::read(&p).unwrap(), original);
        let _ = fs::remove_file(p);
    }
}
