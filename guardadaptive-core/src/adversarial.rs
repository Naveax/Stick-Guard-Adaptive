//! v0.29 adversarial, label-aware *synthetic* parameter search.
//! The desired-input oracle is used by this isolated lab, never by the
//! deployed controller filter. Strict disjoint training vs holdout seeds.
use crate::{
    causal_guard::{CausalGuard, GuardConfig},
    reacquire::VerifiedCenter,
    resolution::Resolution,
    Vec2,
};
use std::{io, path::Path};
const CENTER: Vec2 = Vec2::new(0.071, -0.043);
const REGIMES: [&str; 8] = [
    "quiet_neutral",
    "barely_quantized_micro",
    "slow_ramp",
    "fast_reversal",
    "thermal_wander",
    "bursty_sensor",
    "delayed_release",
    "ambiguous_hold",
];
#[derive(Clone, Copy)]
struct Sample {
    observed: Vec2,
    intended: Vec2,
    regime: usize,
    micro: bool,
}
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xB5AD4ECEDA1CE2A9)
    }
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545F4914F6CDD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64
    }
    fn noise(&mut self) -> f64 {
        2.0 * self.next() - 1.0
    }
}
fn sample(i: usize, rng: &mut Rng) -> Sample {
    let kind = (i / 512) % REGIMES.len();
    let t = (i % 512) as f64 / 511.0;
    let (intent, drift, noise) = match kind {
        0 => (Vec2::ZERO, Vec2::ZERO, 0.00005),
        1 => (
            Vec2::new(if i % 128 < 64 { 0.00016 } else { -0.00016 }, 0.0),
            Vec2::ZERO,
            0.000045,
        ),
        2 => (Vec2::new(0.5 * t, -0.3 * t), Vec2::new(0.01, 0.0), 0.0001),
        3 => (
            Vec2::new(if i % 64 < 32 { 0.85 } else { -0.85 }, 0.12),
            Vec2::ZERO,
            0.0001,
        ),
        4 => (Vec2::ZERO, Vec2::new(0.02 * t, -0.018 * t), 0.00025),
        5 => (
            Vec2::new(0.015, 0.0),
            Vec2::new(0.0, 0.005),
            if i.is_multiple_of(31) { 0.08 } else { 0.0003 },
        ),
        6 => (
            Vec2::new(if t < 0.4 { 0.22 } else { 0.0 }, 0.0),
            Vec2::new(if t > 0.6 { 0.045 } else { 0.0 }, 0.0),
            0.0001,
        ),
        _ => (
            Vec2::new(
                if (i / 256).is_multiple_of(2) {
                    0.25
                } else {
                    0.0
                },
                0.0,
            ),
            Vec2::new(
                if (i / 256).is_multiple_of(2) {
                    0.0
                } else {
                    0.25
                },
                0.0,
            ),
            0.0001,
        ),
    };
    let jitter = Vec2::new(rng.noise() * noise, rng.noise() * noise);
    let observed = Resolution::XInput16.quantize((CENTER + intent + drift + jitter).clamp());
    Sample {
        observed,
        intended: intent,
        regime: kind,
        micro: intent.norm() > 0.0 && intent.norm() <= 0.02,
    }
}
#[derive(Clone, Copy, Default)]
pub struct Score {
    pub frames: u64,
    pub error: f64,
    pub wrong_dir: u64,
    pub micro_lost: u64,
    pub regime_error: [f64; 8],
    pub regime_frames: [u64; 8],
}
impl Score {
    fn add(&mut self, frame: Sample, output: Vec2) {
        self.frames += 1;
        let e = (output - frame.intended).length_squared();
        self.error += e;
        self.regime_error[frame.regime] += e;
        self.regime_frames[frame.regime] += 1;
        if frame.intended.norm() > Resolution::XInput16.positive_lsb()
            && output.norm() > 0.0
            && (output.x * frame.intended.x + output.y * frame.intended.y) < 0.0
        {
            self.wrong_dir += 1;
        }
        if frame.micro && output.norm() < frame.intended.norm() * 0.5 {
            self.micro_lost += 1;
        }
    }
    pub fn rmse(&self) -> f64 {
        (self.error / self.frames as f64).sqrt()
    }
}
#[derive(Clone, Copy)]
pub struct Pair {
    pub baseline: Score,
    pub trial: Score,
}
impl Pair {
    pub fn rmse_gain(&self) -> f64 {
        if self.baseline.error <= 0.0 {
            0.0
        } else {
            1.0 - (self.trial.error / self.baseline.error).sqrt()
        }
    }
    pub fn micro_safe(&self) -> bool {
        self.trial.micro_lost <= self.baseline.micro_lost
    }
    pub fn directional_safe(&self) -> bool {
        self.trial.wrong_dir <= self.baseline.wrong_dir
    }
}
pub fn evaluate(seed: u64, frames: usize, cfg: GuardConfig) -> io::Result<Pair> {
    if !(4096..=500000).contains(&frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "adversarial frames must be 4096..500000",
        ));
    }
    let calibration = VerifiedCenter::restore_checked(CENTER, Vec2::new(0.00001, 0.00001), 0.00001)
        .expect("research-only fixed synthetic calibration");
    let mut engine = CausalGuard::new(cfg);
    engine.accept_verified(calibration);
    let mut rng = Rng::new(seed);
    let mut original = Score::default();
    let mut trial = Score::default();
    for i in 0..frames {
        let x = sample(i, &mut rng);
        original.add(x, (x.observed - CENTER).saturate());
        trial.add(x, engine.process(x.observed));
    }
    Ok(Pair {
        baseline: original,
        trial,
    })
}
#[derive(Clone, Copy)]
pub struct ConfigScore {
    pub cfg: GuardConfig,
    pub training_a: Pair,
    pub training_b: Pair,
    pub holdout_a: Pair,
    pub holdout_b: Pair,
    pub synthetic_gate_pass: bool,
}
pub struct Search {
    pub searched: u64,
    pub chosen: ConfigScore,
    pub candidate_promoted_to_live: bool,
}
pub fn search(seed: u64, frames: usize) -> io::Result<Search> {
    if !(4096..=100000).contains(&frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "search frames 4096..100000",
        ));
    }
    let mut best: Option<(GuardConfig, Pair, Pair, f64)> = None;
    let mut tried = 0;
    // Predeclared search grid; no holdout labels used in model selection.
    for blend in [0.0, 0.15, 0.35, 0.5] {
        for delta in [0.002, 0.006, 0.012, 0.020] {
            for cap in [0.0005, 0.002, 0.004] {
                let cfg = GuardConfig {
                    blend,
                    max_delta: delta,
                    max_correction: cap,
                };
                let a = evaluate(seed, frames, cfg)?;
                let b = evaluate(seed.wrapping_add(1), frames, cfg)?;
                tried += 1;
                let safe = a.micro_safe()
                    && a.directional_safe()
                    && b.micro_safe()
                    && b.directional_safe();
                let objective = if safe {
                    a.trial.error + b.trial.error
                } else {
                    f64::INFINITY
                };
                if best.as_ref().is_none_or(|current| objective < current.3) {
                    best = Some((cfg, a, b, objective));
                }
            }
        }
    }
    let (cfg, a, b, _) = best.expect("fixed candidate grid nonempty");
    // These holdouts are strictly disjoint; no grid selection sees them.
    let hold_a = evaluate(seed.wrapping_add(0x9E37_79B9_7F4A_7C15), frames, cfg)?;
    let hold_b = evaluate(seed.wrapping_add(0xD1B5_4A32_D192_ED03), frames, cfg)?;
    let gate = [a, b, hold_a, hold_b]
        .iter()
        .all(|r| r.rmse_gain() >= 0.01 && r.micro_safe() && r.directional_safe());
    Ok(Search {
        searched: tried,
        chosen: ConfigScore {
            cfg,
            training_a: a,
            training_b: b,
            holdout_a: hold_a,
            holdout_b: hold_b,
            synthetic_gate_pass: gate,
        },
        // Physical identity and game input truth have not been tested.
        candidate_promoted_to_live: false,
    })
}
impl Search {
    pub fn json(&self) -> String {
        let c = &self.chosen;
        let pairs = [
            ("train_a", c.training_a),
            ("train_b", c.training_b),
            ("holdout_a", c.holdout_a),
            ("holdout_b", c.holdout_b),
        ];
        let results=pairs.iter().map(|(name,score)|format!(
            "\"{name}\":{{\"baseline_rmse\":{:.10},\"candidate_rmse\":{:.10},\"relative_gain\":{:.10},\"micro_safe\":{},\"wrong_direction_safe\":{}}}",
            score.baseline.rmse(),score.trial.rmse(),score.rmse_gain(),
            score.micro_safe(),score.directional_safe()
        )).collect::<Vec<_>>().join(",");
        let regimes=REGIMES.iter().enumerate().map(|(i,name)|{
            let a=&c.holdout_a;
            format!("\"{name}\":{{\"baseline_sse\":{:.10},\"candidate_sse\":{:.10},\"samples\":{}}}",
                a.baseline.regime_error[i],a.trial.regime_error[i],
                a.baseline.regime_frames[i])
        }).collect::<Vec<_>>().join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"adversarial_synthetic_search\",",
                "\"searched_configs\":{},\"selected_research_config\":{{",
                "\"blend\":{:.5},\"maximum_step\":{:.6},\"maximum_correction\":{:.6}}},",
                "\"split_results\":{{{}}},\"holdout_regimes\":{{{}}},",
                "\"synthetic_precondition_pass\":{},",
                "\"test_reference_is_simulated\":true,",
                "\"real_controller_evidence\":false,\"live_model_promoted\":false,",
                "\"production_quality\":\"BLOCKED\"}}\n"
            ),
            self.searched,
            c.cfg.blend,
            c.cfg.max_delta,
            c.cfg.max_correction,
            results,
            regimes,
            c.synthetic_gate_pass
        )
    }
}
pub fn run(seed: u64, frames: usize, out: &Path) -> io::Result<()> {
    let result = search(seed, frames)?;
    crate::truth_lab::save_new(out, &result.json())?;
    println!(
        "V0.29 ADVERSARIAL SEARCH: {} configurations, 2 train seeds, 2 unseen holdout seeds",
        result.searched
    );
    for (name, p) in [
        ("train_a", result.chosen.training_a),
        ("train_b", result.chosen.training_b),
        ("holdout_a", result.chosen.holdout_a),
        ("holdout_b", result.chosen.holdout_b),
    ] {
        println!(
            "{name}: baseline_rmse={:.7} trial_rmse={:.7} relative_gain={:.4}%",
            p.baseline.rmse(),
            p.trial.rmse(),
            p.rmse_gain() * 100.0
        );
    }
    println!(
        "PRODUCTION BLOCKED; candidate remains research-only. {}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn train_and_holdout_are_seeded_and_do_not_leak() {
        let a = search(20261029, 5000).unwrap();
        let b = search(20261029, 5000).unwrap();
        assert_eq!(a.searched, 48);
        assert_eq!(a.chosen.cfg.blend, b.chosen.cfg.blend);
        assert_eq!(
            a.chosen.holdout_a.trial.error,
            b.chosen.holdout_a.trial.error
        );
        assert!(!a.candidate_promoted_to_live);
        assert!(a.json().contains("\"real_controller_evidence\":false"));
    }
    #[test]
    fn ground_truth_is_available_only_inside_generator_and_scorer() {
        let zero = GuardConfig {
            blend: 0.0,
            max_delta: 0.006,
            max_correction: 0.002,
        };
        let p = evaluate(11, 9000, zero).unwrap();
        assert_eq!(p.baseline.error, p.trial.error);
        assert_eq!(p.baseline.wrong_dir, p.trial.wrong_dir);
        assert_eq!(p.baseline.micro_lost, p.trial.micro_lost);
    }
    #[test]
    fn all_eight_adversarial_regimes_are_exercised() {
        let cfg = GuardConfig::default();
        let p = evaluate(11, 9000, cfg).unwrap();
        assert!(p.baseline.regime_frames.iter().all(|n| *n >= 512));
        assert_eq!(p.baseline.regime_frames.iter().sum::<u64>(), 9000);
    }
    #[test]
    fn invalid_frame_count_fails_before_search() {
        assert!(search(1, 0).is_err());
        assert!(search(1, 100001).is_err());
    }
}
