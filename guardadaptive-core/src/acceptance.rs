//! v0.25: hard-blocked evidence-oriented acceptance matrix.
//! Synthetic truth is useful research evidence but never a physical or
//! playable-output proof. Deliberate fail-closed release gating.
use crate::truth_lab::{self, ResultSet, Score};
use std::{io, path::Path};
const MODELS: [&str; 6] = [
    "center_only",
    "motion_first",
    "hybrid",
    "direction_guard",
    "confidence_direction_rejected_v015",
    "causal_bounded_v028_research_only",
];
pub struct Matrix {
    pub primary: ResultSet,
    pub holdout: ResultSet,
    pub primary_seed: u64,
    pub holdout_seed: u64,
}
pub struct SyntheticVerdict {
    pub candidate: &'static str,
    pub primary_error_improvement: f64,
    pub holdout_error_improvement: f64,
    pub no_micro_regression: bool,
    pub no_wrong_direction_regression: bool,
    pub both_seeds_pass: bool,
}
fn error(s: &Score) -> f64 {
    (s.squared / s.total as f64).sqrt()
}
fn improvement(before: &Score, after: &Score) -> f64 {
    let baseline = error(before);
    if baseline <= f64::EPSILON {
        0.0
    } else {
        (baseline - error(after)) / baseline
    }
}
impl Matrix {
    pub fn evaluate(seed: u64, samples: usize) -> io::Result<Self> {
        let holdout_seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        if seed == holdout_seed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "independent seeds required",
            ));
        }
        Ok(Self {
            primary: truth_lab::evaluate(seed, samples)?,
            holdout: truth_lab::evaluate(holdout_seed, samples)?,
            primary_seed: seed,
            holdout_seed,
        })
    }
    pub fn verdicts(&self) -> Vec<SyntheticVerdict> {
        (1..6)
            .map(|i| {
                let a = &self.primary.scores[i];
                let b = &self.holdout.scores[i];
                let c = &self.primary.scores[0];
                let d = &self.holdout.scores[0];
                let gain_a = improvement(c, a);
                let gain_b = improvement(d, b);
                let no_micro = a.micro_lost <= c.micro_lost && b.micro_lost <= d.micro_lost;
                let no_wrong = a.wrong_direction <= c.wrong_direction
                    && b.wrong_direction <= d.wrong_direction;
                SyntheticVerdict {
                    candidate: MODELS[i],
                    primary_error_improvement: gain_a,
                    holdout_error_improvement: gain_b,
                    no_micro_regression: no_micro,
                    no_wrong_direction_regression: no_wrong,
                    both_seeds_pass: gain_a >= 0.01 && gain_b >= 0.01 && no_micro && no_wrong,
                }
            })
            .collect()
    }
    pub fn json(&self) -> String {
        let candidates = self
            .verdicts()
            .iter()
            .map(|v| {
                format!(
                    concat!(
                        "\"{}\":{{\"primary_rmse_improvement\":{:.9},",
                        "\"holdout_rmse_improvement\":{:.9},",
                        "\"no_micro_regression\":{},\"no_wrong_direction_regression\":{},",
                        "\"synthetic_preconditions_pass\":{}}}"
                    ),
                    v.candidate,
                    v.primary_error_improvement,
                    v.holdout_error_improvement,
                    v.no_micro_regression,
                    v.no_wrong_direction_regression,
                    v.both_seeds_pass
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v025_nonpromoting_release_matrix\",",
                "\"primary_seed\":{},\"holdout_seed\":{},\"samples_per_seed\":{},",
                "\"thresholds\":{{\"minimum_rmse_improvement_per_seed\":0.01,",
                "\"no_micro_loss_regression\":true,\"no_wrong_direction_regression\":true}},",
                "\"models\":{{{}}},",
                "\"requirements\":{{",
                "\"real_controller_recordings\":false,",
                "\"authenticated_stable_device_identity\":false,",
                "\"independent_player_intent_ground_truth\":false,",
                "\"input_to_game_latency_evidence\":false,",
                "\"reversible_virtual_output_verified\":false,",
                "\"long_duration_real_game_acceptance\":false}},",
                "\"automated_candidate_promotion\":false,",
                "\"production_gate\":\"BLOCKED\",",
                "\"purpose\":\"synthetic_candidate_exploration_not_hardware_certification\"}}\n"
            ),
            self.primary_seed, self.holdout_seed, self.primary.frames, candidates
        )
    }
}
pub fn run(seed: u64, samples: usize, out: &Path) -> io::Result<()> {
    let m = Matrix::evaluate(seed, samples)?;
    truth_lab::save_new(out, &m.json())?;
    println!(
        "V0.25 SYNTHETIC QUALITY MATRIX primary={} holdout={} samples_each={}",
        m.primary_seed, m.holdout_seed, samples
    );
    for v in m.verdicts() {
        println!("{} gain_primary={:.3}% gain_holdout={:.3}% micro_safe={} wrong_dir_safe={} synthetic_pass={}",
            v.candidate,100.0*v.primary_error_improvement,
            100.0*v.holdout_error_improvement,v.no_micro_regression,
            v.no_wrong_direction_regression,v.both_seeds_pass);
    }
    println!("PRODUCTION_GATE=BLOCKED physical device, independent intent, E2E latency and game output unverified.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reproducible_matrix_never_promotes_research_candidate() {
        let x = Matrix::evaluate(101, 7200).unwrap();
        assert_eq!(x.verdicts().len(), 5);
        let j = x.json();
        assert!(j.contains("\"production_gate\":\"BLOCKED\""));
        assert!(j.contains("\"automated_candidate_promotion\":false"));
        assert!(j.contains("\"independent_player_intent_ground_truth\":false"));
        assert!(j.contains("\"real_controller_recordings\":false"));
        assert_eq!(x.primary_seed, 101);
        assert_ne!(x.primary_seed, x.holdout_seed);
    }
    #[test]
    fn synthetic_comparisons_are_bounded_not_guaranteed() {
        let m = Matrix::evaluate(20261025, 7200).unwrap();
        for v in m.verdicts() {
            assert!(v.primary_error_improvement.is_finite());
            assert!(v.holdout_error_improvement.is_finite());
            if v.both_seeds_pass {
                assert!(v.no_micro_regression && v.no_wrong_direction_regression);
                assert!(v.primary_error_improvement >= 0.01);
                assert!(v.holdout_error_improvement >= 0.01);
            }
        }
        assert!(m
            .json()
            .contains("\"reversible_virtual_output_verified\":false"));
    }
    #[test]
    fn failed_v028_candidate_cannot_pass_two_seed_approval() {
        let m = Matrix::evaluate(20261028, 72000).unwrap();
        let v = m
            .verdicts()
            .into_iter()
            .find(|v| v.candidate == "causal_bounded_v028_research_only")
            .unwrap();
        assert!(!v.both_seeds_pass);
        assert!(v.primary_error_improvement < 0.0);
        assert!(v.holdout_error_improvement < 0.0);
        assert!(!v.no_micro_regression || !v.no_wrong_direction_regression);
    }
    #[test]
    fn invalid_horizon_rejected_without_artifact() {
        assert!(Matrix::evaluate(8, 8).is_err());
    }
}
