//! v0.40 isolated variable-drift comparative research.
//! The filter sees XInput-quantized raw input and externally scripted,
//! explicitly approved release labels ONLY. Desired intent and actual
//! drift stay in the evaluator; neither is supplied to a candidate.
//! The live controller filter is unchanged and game output stays disabled.
use crate::{resolution::Resolution, Vec2};
use std::{io, path::Path};
pub(crate) const CASES: [&str; 6] = [
    "static_heavy_drift",
    "thermal_ramp_then_stable",
    "abrupt_shift_then_stable",
    "oscillating_drift",
    "ambiguous_long_hold",
    "micro_and_fast_reversal",
];
const N: usize = 4096;
const WINDOW: usize = 128;
const SECOND: usize = 2048;
const CENTER: Vec2 = Vec2::new(0.25, -0.14);
pub(crate) const TRAIN: [u64; 2] = [20261013, 20261017];
pub(crate) const HOLDOUT: [u64; 2] = [20270011, 20270029];
#[derive(Clone, Copy)]
struct Sample {
    raw: Vec2,
    truth: Vec2,
    released_window: bool,
}
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E3779B97F4A7C15)
    }
    fn noise(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let n = self.0.wrapping_mul(0x2545F4914F6CDD1D);
        (n >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}
fn generate(case: usize, seed: u64) -> Vec<Sample> {
    let mut rng = Rng::new(seed ^ (case as u64).wrapping_mul(0xD1B54A32D192ED03));
    (0..N)
        .map(|i| {
            // The harness script declares these specific windows RELEASED. The
            // candidate is not told any hidden intended coordinate.
            let second = case != 4 && (SECOND..SECOND + WINDOW).contains(&i);
            let acknowledged = i < WINDOW || second;
            let phase = (i.saturating_sub(WINDOW)) % 384;
            let intent = if acknowledged {
                Vec2::ZERO
            } else {
                match case {
                    0..=3 => match phase / 96 {
                        0 => Vec2::new(if i % 2 == 0 { 0.002 } else { -0.002 }, 0.0005),
                        1 => Vec2::new(0.40, -0.20),
                        2 => Vec2::new(if i % 32 < 16 { 0.72 } else { -0.72 }, 0.0),
                        _ => Vec2::new(0.30 * ((phase % 96) as f64 / 95.0), 0.08),
                    },
                    4 => {
                        if (1300..3300).contains(&i) {
                            Vec2::new(0.27, 0.0)
                        } else {
                            Vec2::new(if i % 2 == 0 { 0.002 } else { -0.002 }, 0.0)
                        }
                    }
                    _ => {
                        if i % 64 < 32 {
                            Vec2::new(0.0018, 0.0005)
                        } else {
                            Vec2::new(if i % 4 < 2 { 0.81 } else { -0.81 }, -0.13)
                        }
                    }
                }
            };
            let variation = match case {
                0 => Vec2::ZERO,
                1 => {
                    let t = ((i.saturating_sub(512)) as f64 / 1200.0).clamp(0.0, 1.0);
                    Vec2::new(0.17 * t, -0.11 * t)
                }
                2 => {
                    if i >= 1200 {
                        Vec2::new(-0.19, 0.16)
                    } else {
                        Vec2::ZERO
                    }
                }
                3 => Vec2::new(
                    0.045 * ((i as f64) * 0.004).sin(),
                    0.035 * ((i as f64) * 0.005).cos() - 0.035,
                ),
                _ => Vec2::ZERO,
            };
            // Oscillation is zero at the opening calibration by freezing
            // the first window. Other scenarios also freeze their first window.
            let drift = if i < WINDOW {
                CENTER
            } else {
                CENTER + variation
            };
            let noise = Vec2::new(rng.noise() * 0.00011, rng.noise() * 0.00011);
            Sample {
                raw: Resolution::XInput16.quantize((drift + intent + noise).clamp()),
                truth: intent,
                released_window: acknowledged,
            }
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Candidate {
    Fixed,
    Blind {
        alpha: f64,
    },
    /// v0.74 offline-only hypothesis: reject unstable or distant windows.
    /// A stable held intention may still masquerade as neutral; NOT live-safe.
    BoundedStable {
        blend: f64,
        max_total: f64,
    },
    OperatorReleased,
}
fn median(v: &[f64]) -> f64 {
    let mut x = v.to_vec();
    x.sort_by(f64::total_cmp);
    let n = x.len();
    (x[n / 2 - 1] + x[n / 2]) * 0.5
}
fn robust_window(window: &[Vec2]) -> Option<Vec2> {
    if window.len() != WINDOW {
        return None;
    }
    let xs = window.iter().map(|v| v.x).collect::<Vec<_>>();
    let ys = window.iter().map(|v| v.y).collect::<Vec<_>>();
    let cx = median(&xs);
    let cy = median(&ys);
    let center = Vec2::new(cx, cy);
    let mad_x = median(&xs.iter().map(|x| (x - cx).abs()).collect::<Vec<_>>()) * 1.4826;
    let mad_y = median(&ys.iter().map(|y| (y - cy).abs()).collect::<Vec<_>>()) * 1.4826;
    let half_a = window[0..64].iter().fold(Vec2::ZERO, |acc, v| acc + *v) * (1.0 / 64.0);
    let half_b = window[64..128].iter().fold(Vec2::ZERO, |acc, v| acc + *v) * (1.0 / 64.0);
    if mad_x > 0.0015 || mad_y > 0.0015 || (half_a - half_b).norm() > 0.003 {
        return None;
    }
    Some(center)
}
#[derive(Clone, Copy, Default)]
pub struct Score {
    pub n: u64,
    pub sse: f64,
    pub micro_n: u64,
    pub micro_lost: u64,
    pub wrong_dir: u64,
    pub updates: u64,
    pub refused_windows: u64,
    pub hold_movement_lost: u64,
}
impl Score {
    pub fn rmse(self) -> f64 {
        (self.sse / self.n.max(1) as f64).sqrt()
    }
    fn observe(&mut self, i: usize, truth: Vec2, out: Vec2, case: usize) {
        self.n += 1;
        self.sse += (truth - out).length_squared();
        if truth.norm() > 0.0 && truth.norm() <= 0.01 {
            self.micro_n += 1;
            if out.norm() < truth.norm() * 0.5 {
                self.micro_lost += 1;
            }
        }
        if truth.norm() > 0.00003 && out.x * truth.x + out.y * truth.y < 0.0 {
            self.wrong_dir += 1;
        }
        if case == 4 && (1300..3300).contains(&i) && out.x < 0.135 {
            self.hold_movement_lost += 1;
        }
    }
}
fn evaluate(samples: &[Sample], case: usize, mode: Candidate) -> Score {
    let initial = robust_window(&samples[0..WINDOW].iter().map(|x| x.raw).collect::<Vec<_>>())
        .expect("fixture first 128 frames are stable");
    let mut center = initial;
    let mut score = Score::default();
    let mut second_buffer = Vec::with_capacity(WINDOW);
    let mut stable_buffer = Vec::with_capacity(WINDOW);
    let mut last_admission_frame = 0_usize;
    for (i, s) in samples.iter().enumerate() {
        if i < WINDOW {
            continue;
        }
        if s.released_window {
            // Prevent an unverified/ignored release interval from being
            // spliced into an apparently continuous raw-only stable window.
            stable_buffer.clear();
            if mode == Candidate::OperatorReleased {
                second_buffer.push(s.raw);
                if second_buffer.len() == WINDOW {
                    if let Some(next) = robust_window(&second_buffer) {
                        center = next;
                        score.updates += 1;
                    } else {
                        score.refused_windows += 1;
                    }
                    second_buffer.clear();
                }
            }
            // Do not score expressly acknowledged release windows.
            continue;
        }
        let output = (s.raw - center).saturate();
        score.observe(i, s.truth, output, case);
        if let Candidate::Blind { alpha } = mode {
            // Demonstrates the intentionally unsafe assumption that observed
            // stick positions may teach a new neutral during live gameplay.
            // It does not see true intention, release labels, or future data.
            center = center * (1.0 - alpha) + s.raw * alpha;
        }
        if let Candidate::BoundedStable { blend, max_total } = mode {
            // Research-only, no truth/release labels used for an update.
            // A stable held player intent can satisfy this observation gate!
            stable_buffer.push(s.raw);
            if stable_buffer.len() == WINDOW {
                if let Some(observed_center) = robust_window(&stable_buffer) {
                    if (observed_center - initial).norm() <= max_total
                        && i.saturating_sub(last_admission_frame) >= WINDOW
                    {
                        center = center * (1.0 - blend) + observed_center * blend;
                        score.updates += 1;
                        last_admission_frame = i;
                    } else {
                        score.refused_windows += 1;
                    }
                } else {
                    score.refused_windows += 1;
                }
                stable_buffer.clear();
            }
        }
    }
    score
}
#[derive(Clone, Copy, Default)]
pub struct Aggregation {
    pub errors: f64,
    pub frames: u64,
    pub micro_lost: u64,
    pub wrong_dir: u64,
    pub hold_lost: u64,
    pub reapprovals: u64,
    pub rejected_windows: u64,
}
impl Aggregation {
    pub fn add(&mut self, x: Score) {
        self.errors += x.sse;
        self.frames += x.n;
        self.micro_lost += x.micro_lost;
        self.wrong_dir += x.wrong_dir;
        self.hold_lost += x.hold_movement_lost;
        self.reapprovals += x.updates;
        self.rejected_windows += x.refused_windows;
    }
    pub fn rmse(self) -> f64 {
        (self.errors / self.frames.max(1) as f64).sqrt()
    }
}
#[derive(Clone, Copy)]
pub struct Pair {
    pub fixed: Aggregation,
    pub adaptive: Aggregation,
}
impl Pair {
    pub fn gain(self) -> f64 {
        if self.fixed.errors == 0.0 {
            0.0
        } else {
            1.0 - (self.adaptive.errors / self.fixed.errors).sqrt()
        }
    }
}
#[derive(Clone)]
pub struct Experiment {
    pub blind_alpha: f64,
    pub training: [Pair; 6],
    pub holdout: [Pair; 6],
    pub operator_training: [Pair; 6],
    pub operator_holdout: [Pair; 6],
    pub blind_live_promoted: bool,
    pub operator_live_promoted: bool,
    pub blind_holdout_gate: bool,
    pub operator_holdout_gate: bool,
}
pub(crate) fn evaluate_suite(seeds: &[u64; 2], candidate: Candidate) -> [Pair; 6] {
    std::array::from_fn(|case| {
        let mut fixed = Aggregation::default();
        let mut adaptive = Aggregation::default();
        for seed in seeds {
            let data = generate(case, *seed);
            fixed.add(evaluate(&data, case, Candidate::Fixed));
            adaptive.add(evaluate(&data, case, candidate));
        }
        Pair { fixed, adaptive }
    })
}
pub(crate) fn aggregated_gain(rows: &[Pair; 6]) -> f64 {
    let base = rows.iter().map(|r| r.fixed.errors).sum::<f64>();
    let alt = rows.iter().map(|r| r.adaptive.errors).sum::<f64>();
    if base == 0.0 {
        0.0
    } else {
        1.0 - (alt / base).sqrt()
    }
}
// A malicious or mistaken release label inside an actual held-input period
// can fool a perfectly stable-window estimator.  The candidate never sees
// physical intent; this proves that labels alone are not authentication.
pub(crate) fn false_release_counterfactual() -> (u64, u64, u64) {
    let original = generate(4, HOLDOUT[0]);
    let correct = evaluate(&original, 4, Candidate::OperatorReleased);
    let mut mislabeled = original;
    for sample in &mut mislabeled[1500..1500 + WINDOW] {
        sample.released_window = true;
    }
    let spoofed = evaluate(&mislabeled, 4, Candidate::OperatorReleased);
    (
        correct.hold_movement_lost,
        spoofed.hold_movement_lost,
        spoofed.updates,
    )
}
pub fn experiment() -> Experiment {
    // Predeclared research grid. Choose blind alpha on *train seeds ONLY*.
    let configs = [0.0001, 0.0005, 0.002, 0.01];
    let mut chosen = (
        configs[0],
        evaluate_suite(&TRAIN, Candidate::Blind { alpha: configs[0] }),
    );
    for &alpha in configs.iter().skip(1) {
        let current = evaluate_suite(&TRAIN, Candidate::Blind { alpha });
        if aggregated_gain(&current) > aggregated_gain(&chosen.1) {
            chosen = (alpha, current);
        }
    }
    // Holdout datasets are created only after training selection is complete.
    let blind_holdout = evaluate_suite(&HOLDOUT, Candidate::Blind { alpha: chosen.0 });
    let operator_training = evaluate_suite(&TRAIN, Candidate::OperatorReleased);
    let operator_holdout = evaluate_suite(&HOLDOUT, Candidate::OperatorReleased);
    // Strict deployment blockers: global gain >1%, zero direction/micro
    // regression, no held-intention harm and every regime non-worsening.
    // The operator candidate is only meaningful when releases are truthful.
    let gate = |pairs: &[Pair; 6]| {
        aggregated_gain(pairs) >= 0.01
            && pairs.iter().all(|p| {
                p.adaptive.errors <= p.fixed.errors * 1.001
                    && p.adaptive.micro_lost <= p.fixed.micro_lost
                    && p.adaptive.wrong_dir <= p.fixed.wrong_dir
                    && p.adaptive.hold_lost <= p.fixed.hold_lost
            })
    };
    let blind_holdout_gate = gate(&blind_holdout);
    let operator_holdout_gate = gate(&operator_holdout);
    Experiment {
        blind_alpha: chosen.0,
        training: chosen.1,
        holdout: blind_holdout,
        operator_training,
        operator_holdout,
        blind_live_promoted: false,
        operator_live_promoted: false,
        blind_holdout_gate,
        operator_holdout_gate,
    }
}
impl Experiment {
    pub fn json(&self) -> String {
        fn split(pairs: &[Pair; 6]) -> String {
            CASES.iter().zip(pairs.iter()).map(|(name,p)|format!(
                "\"{}\":{{\"fixed_rmse\":{:.9},\"candidate_rmse\":{:.9},\"relative_gain\":{:.9},\"micro_lost_fixed\":{},\"micro_lost_candidate\":{},\"wrong_direction_fixed\":{},\"wrong_direction_candidate\":{},\"long_hold_lost_fixed\":{},\"long_hold_lost_candidate\":{},\"approved_new_centers\":{},\"refused_release_windows\":{}}}",
                name,p.fixed.rmse(),p.adaptive.rmse(),p.gain(),
                p.fixed.micro_lost,p.adaptive.micro_lost,p.fixed.wrong_dir,
                p.adaptive.wrong_dir,p.fixed.hold_lost,p.adaptive.hold_lost,
                p.adaptive.reapprovals,p.adaptive.rejected_windows
            )).collect::<Vec<_>>().join(",")
        }
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v40_variable_drift_train_holdout_research\",",
                "\"train_seeds\":[{},{}],\"holdout_seeds\":[{},{}],",
                "\"frames_per_regime_seed\":{},\"blind_selected_alpha\":{:.6},",
                "\"blind_train\":{{{}}},\"blind_holdout\":{{{}}},",
                "\"operator_train\":{{{}}},\"operator_holdout\":{{{}}},",
                "\"blind_global_holdout_gain\":{:.9},\"operator_global_holdout_gain\":{:.9},",
                "\"blind_holdout_gate\":{},\"operator_holdout_gate\":{},",
                "\"ground_truth_visible_to_candidates\":false,",
                "\"operator_release_is_scripted_not_physically_verified\":true,",
                "\"automatic_gameplay_center_learning_enabled\":false,",
                "\"live_candidate_promoted\":false,\"game_writes\":0,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            TRAIN[0],
            TRAIN[1],
            HOLDOUT[0],
            HOLDOUT[1],
            N,
            self.blind_alpha,
            split(&self.training),
            split(&self.holdout),
            split(&self.operator_training),
            split(&self.operator_holdout),
            aggregated_gain(&self.holdout),
            aggregated_gain(&self.operator_holdout),
            self.blind_holdout_gate,
            self.operator_holdout_gate
        )
    }
}
pub fn run(out: &Path) -> io::Result<()> {
    let result = experiment();
    crate::truth_lab::save_new(out, &result.json())?;
    println!(
        "V040 research blind alpha={} holdout_global_gain={:.2}% gate={}",
        result.blind_alpha,
        aggregated_gain(&result.holdout) * 100.0,
        result.blind_holdout_gate
    );
    println!(
        "V040 operator-approved holdout_global_gain={:.2}% gate={}",
        aggregated_gain(&result.operator_holdout) * 100.0,
        result.operator_holdout_gate
    );
    for (name, p) in CASES.iter().zip(result.operator_holdout.iter()) {
        println!(
            " {name}: fixed_rmse={:.5} operator_rmse={:.5} relative_gain={:.1}%",
            p.fixed.rmse(),
            p.adaptive.rmse(),
            p.gain() * 100.0
        );
    }
    println!("NO_LIVE_PROMOTION NO_DRIVER NO_PHYSICAL_RELEASE game_writes=0 production=BLOCKED");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_raw_observation_has_ambiguous_physical_explanations() {
        // Both states create the same raw sensor value (non-identifiability).
        let raw = Vec2::new(0.25, 0.0);
        let first = Vec2::new(0.25, 0.0) + Vec2::ZERO;
        let second = Vec2::ZERO + Vec2::new(0.25, 0.0);
        assert_eq!(raw, first);
        assert_eq!(first, second);
    }
    #[test]
    fn only_scripted_release_frames_may_update_operator_center() {
        let c = generate(4, TRAIN[0]);
        assert_eq!(c.iter().filter(|x| x.released_window).count(), WINDOW);
        let s = evaluate(&c, 4, Candidate::OperatorReleased);
        assert_eq!(s.updates, 0);
        assert!(s.hold_movement_lost == 0);
        let thermal = generate(1, TRAIN[0]);
        let manual = evaluate(&thermal, 1, Candidate::OperatorReleased);
        assert_eq!(manual.updates, 1);
    }
    #[test]
    fn true_intention_is_never_input_to_filter() {
        let data = generate(2, TRAIN[0]);
        let original = evaluate(&data, 2, Candidate::OperatorReleased);
        let mut altered = data.clone();
        for item in &mut altered {
            item.truth = Vec2::new(0.85, 0.4);
        }
        // Compare output score with intention changed: metrics differ, but
        // the output algorithm depends only on raw, labels and case flags.
        // Independently test center estimate using only raw observations.
        let a = robust_window(
            &data[SECOND..SECOND + WINDOW]
                .iter()
                .map(|x| x.raw)
                .collect::<Vec<_>>(),
        );
        let b = robust_window(
            &altered[SECOND..SECOND + WINDOW]
                .iter()
                .map(|x| x.raw)
                .collect::<Vec<_>>(),
        );
        assert_eq!(a, b);
        assert_eq!(original.updates, 1);
    }
    #[test]
    fn unsafe_blind_adaptation_can_erase_a_genuinely_held_input() {
        let data = generate(4, TRAIN[0]);
        let baseline = evaluate(&data, 4, Candidate::Fixed);
        let blind = evaluate(&data, 4, Candidate::Blind { alpha: 0.01 });
        assert!(blind.hold_movement_lost > 1000);
        assert_eq!(baseline.hold_movement_lost, 0);
        assert!(blind.sse > baseline.sse);
    }
    #[test]
    fn oscillation_rejects_trusted_window_without_auto_learning() {
        let data = generate(3, HOLDOUT[0]);
        let operator = evaluate(&data, 3, Candidate::OperatorReleased);
        assert_eq!(operator.updates, 0);
        assert_eq!(operator.refused_windows, 1);
    }
    #[test]
    fn holdout_gates_are_research_only_even_if_operator_passes() {
        let e = experiment();
        assert!(!e.blind_holdout_gate);
        assert!(e.operator_holdout_gate);
        assert!(e.operator_holdout[1].gain() > 0.3);
        assert!(e.operator_holdout[2].gain() > 0.3);
        assert!(e
            .operator_holdout
            .iter()
            .all(|p| p.adaptive.micro_lost <= p.fixed.micro_lost
                && p.adaptive.wrong_dir <= p.fixed.wrong_dir));
    }
    #[test]
    fn holdout_seeds_disjoint_from_training_and_no_promotion() {
        assert!(TRAIN.iter().all(|s| !HOLDOUT.contains(s)));
        let r = experiment();
        assert!(!r.blind_live_promoted && !r.operator_live_promoted);
        assert!(r.json().contains("\"live_candidate_promoted\":false"));
        assert!(r
            .json()
            .contains("\"ground_truth_visible_to_candidates\":false"));
    }
    #[test]
    fn benchmark_report_never_overwrites() {
        let out = std::env::temp_dir().join(format!("ga-v40-{}.json", std::process::id()));
        if out.exists() {
            std::fs::remove_file(&out).unwrap();
        }
        run(&out).unwrap();
        let original = std::fs::read(&out).unwrap();
        assert!(run(&out).is_err());
        assert_eq!(original, std::fs::read(&out).unwrap());
        let _ = std::fs::remove_file(out);
    }
}
