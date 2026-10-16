//! v0.66: train/holdout-preserving blind drift safety decision.
//! Analysis ONLY, not a game controller, live calibration, or model promotion.
//! If different physical intents can yield identical raw histories, no blind
//! estimator can guarantee recovery of both from those histories alone.
use crate::drift_holdout_v40::{
    aggregated_gain, evaluate_suite, experiment, false_release_counterfactual, Candidate, Pair,
    CASES,
};
use std::{fmt::Write as _, io, path::Path};

const SHADOW_SEEDS: [u64; 2] = [20270031, 20270117];
const PER_REGIME_ERROR_TOLERANCE: f64 = 1.001;
const MIN_GLOBAL_GAIN: f64 = 0.01;
const MIN_HYPOTHESIS_DISAGREEMENT: f64 = 0.25;
const MAX_BEST_WORST_CASE_ERROR: f64 = 0.125;

#[derive(Debug, Clone)]
pub struct Review {
    pub blind_train_selected_alpha: f64,
    pub blind_original_holdout_gate: bool,
    pub blind_shadow_gain: f64,
    pub blind_shadow_strict_gate: bool,
    pub blind_original_regression_count: usize,
    pub blind_shadow_regression_count: usize,
    pub original_held_intent_lost: u64,
    pub mislabeled_held_intent_lost: u64,
    pub mislabeled_stable_window_approved: u64,
    pub no_auto_learning: bool,
    pub rows: Vec<Row>,
}
#[derive(Debug, Clone)]
pub struct Row {
    pub regime: &'static str,
    pub split: &'static str,
    pub baseline_rmse: f64,
    pub blind_rmse: f64,
    pub gain: f64,
    pub rmse_regression: bool,
    pub micro_regression: bool,
    pub direction_regression: bool,
    pub long_hold_regression: bool,
}
impl Row {
    fn from_pair(regime: &'static str, split: &'static str, pair: Pair) -> Self {
        Self {
            regime,
            split,
            baseline_rmse: pair.fixed.rmse(),
            blind_rmse: pair.adaptive.rmse(),
            gain: pair.gain(),
            rmse_regression: pair.adaptive.errors > pair.fixed.errors * PER_REGIME_ERROR_TOLERANCE,
            micro_regression: pair.adaptive.micro_lost > pair.fixed.micro_lost,
            direction_regression: pair.adaptive.wrong_dir > pair.fixed.wrong_dir,
            long_hold_regression: pair.adaptive.hold_lost > pair.fixed.hold_lost,
        }
    }
    pub fn regressed(&self) -> bool {
        self.rmse_regression
            || self.micro_regression
            || self.direction_regression
            || self.long_hold_regression
    }
}
pub fn evaluate() -> io::Result<Review> {
    // The existing v40 train-selection and locked original HOLDOUT are
    // untouched. These NEW shadow seeds are used exclusively after alpha
    // selection; a future tuning iteration must use different unseen seeds.
    let original = experiment();
    let shadow = evaluate_suite(
        &SHADOW_SEEDS,
        Candidate::Blind {
            alpha: original.blind_alpha,
        },
    );
    let mut rows = Vec::with_capacity(CASES.len() * 2);
    for (name, pair) in CASES.iter().zip(original.holdout.iter()) {
        rows.push(Row::from_pair(name, "original_locked_holdout", *pair));
    }
    for (name, pair) in CASES.iter().zip(shadow.iter()) {
        rows.push(Row::from_pair(name, "independent_shadow_holdout", *pair));
    }
    let base_bad = rows[..6].iter().filter(|r| r.regressed()).count();
    let shadow_bad = rows[6..].iter().filter(|r| r.regressed()).count();
    let shadow_gain = aggregated_gain(&shadow);
    let shadow_gate = shadow_gain >= MIN_GLOBAL_GAIN && shadow_bad == 0;
    let (normal_lost, spoof_lost, spoof_updates) = false_release_counterfactual();

    if (MIN_HYPOTHESIS_DISAGREEMENT / 2.0 - MAX_BEST_WORST_CASE_ERROR).abs() > f64::EPSILON
        || original.blind_holdout_gate
        || base_bad == 0
        || shadow_bad == 0
        || normal_lost != 0
        || spoof_lost < 500
        || spoof_updates == 0
    {
        return Err(io::Error::other(
            "ambiguous input/release-label adversarial witness unexpectedly changed",
        ));
    }
    if !original.blind_alpha.is_finite()
        || original.blind_alpha <= 0.0
        || original.blind_live_promoted
        || original.operator_live_promoted
    {
        return Err(io::Error::other(
            "unexpected promoted/nonfinite blind experiment",
        ));
    }
    // The only fail-closed choice with UNKNOWN physical release credibility:
    // preserve the fixed/previously explicitly approved center. This doesn't
    // claim any blind performance gain. No actual calibration state is changed.
    Ok(Review {
        blind_train_selected_alpha: original.blind_alpha,
        blind_original_holdout_gate: original.blind_holdout_gate,
        blind_shadow_gain: shadow_gain,
        blind_shadow_strict_gate: shadow_gate,
        blind_original_regression_count: base_bad,
        blind_shadow_regression_count: shadow_bad,
        original_held_intent_lost: normal_lost,
        mislabeled_held_intent_lost: spoof_lost,
        mislabeled_stable_window_approved: spoof_updates,
        no_auto_learning: true,
        rows,
    })
}
fn to_json(x: &Review) -> String {
    let mut rows = String::new();
    for (i, r) in x.rows.iter().enumerate() {
        if i > 0 {
            rows.push(',');
        }
        let _ = write!(
            rows,
            concat!(
                "{{\"regime\":\"{}\",\"split\":\"{}\",\"fixed_rmse\":{:.9},",
                "\"blind_rmse\":{:.9},\"relative_gain\":{:.9},",
                "\"rmse_regression\":{},\"micro_regression\":{},",
                "\"direction_regression\":{},\"long_hold_regression\":{},",
                "\"any_regression\":{}}}"
            ),
            r.regime,
            r.split,
            r.baseline_rmse,
            r.blind_rmse,
            r.gain,
            r.rmse_regression,
            r.micro_regression,
            r.direction_regression,
            r.long_hold_regression,
            r.regressed()
        );
    }
    format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v66_unpromoted_blind_holdout_safety_review\",",
            "\"train_only_selected_alpha\":{:.6},\"original_locked_holdout_gate\":{},",
            "\"independent_shadow_seeds\":[{},{}],",
            "\"independent_shadow_global_gain\":{:.9},\"shadow_strict_gate\":{},",
            "\"original_regressed_regimes\":{},\"shadow_regressed_regimes\":{},",
            "\"physical_intent_identifiability_counterexample\":{{",
            "\"identical_observed_raw_x\":0.50,\"different_true_intentions_x\":[0.0,0.25],",
            "\"max_error_lower_bound_for_any_identical_output\":{:.3},",
            "\"no_blind_guarantee_from_raw_alone\":true}},",
            "\"false_release_window_probe\":{{",
            "\"actual_held_intent_lost_without_spoof\":{},",
            "\"actual_held_intent_lost_with_spoof\":{},",
            "\"stable_window_mistakenly_accepted\":{}}},",
            "\"per_case\":[{}],",
            "\"unverified_operator_labels_are_not_physical_attestation\":true,",
            "\"automatic_blind_center_learning_allowed\":false,",
            "\"automatic_model_promotion\":false,",
            "\"real_recorded_controller_data_present\":false,",
            "\"algorithm_quality_qualified\":false,",
            "\"synthetic_only\":true,\"driver_writes\":0,\"game_input_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        x.blind_train_selected_alpha,
        x.blind_original_holdout_gate,
        SHADOW_SEEDS[0],
        SHADOW_SEEDS[1],
        x.blind_shadow_gain,
        x.blind_shadow_strict_gate,
        x.blind_original_regression_count,
        x.blind_shadow_regression_count,
        MAX_BEST_WORST_CASE_ERROR,
        x.original_held_intent_lost,
        x.mislabeled_held_intent_lost,
        x.mislabeled_stable_window_approved,
        rows
    )
}
pub fn run(out: &Path) -> io::Result<()> {
    let analysis = evaluate()?;
    let content = to_json(&analysis);
    crate::truth_lab::save_new(out, &content)?;
    println!(
        "V066_BLIND_REVIEW=PASS original_gate={} shadow_gate={} regressions={}/{} false_release_harm={} blind_auto_learning=DENIED production=BLOCKED",
        analysis.blind_original_holdout_gate,analysis.blind_shadow_strict_gate,
        analysis.blind_original_regression_count,analysis.blind_shadow_regression_count,
        analysis.mislabeled_held_intent_lost
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn holdout_failures_are_exposed_not_tuned_away() {
        let x = evaluate().unwrap();
        assert!(!x.blind_original_holdout_gate);
        assert!(x.blind_original_regression_count > 0);
        assert!(x.blind_shadow_regression_count > 0);
        assert!(!x.blind_shadow_strict_gate);
        assert!(x.no_auto_learning);
        assert_eq!(x.rows.len(), 12);
    }
    #[test]
    fn identical_raw_input_has_incompatible_intent_hypotheses() {
        let observed = 0.50_f64;
        let drift_world = 0.50_f64;
        let intentional_world_baseline = 0.25_f64;
        let true_intent_world_a = observed - drift_world;
        let true_intent_world_b = observed - intentional_world_baseline;
        assert_eq!(true_intent_world_a, 0.0);
        assert_eq!(true_intent_world_b, MIN_HYPOTHESIS_DISAGREEMENT);
        // Any output y must have worst-case error >= half the gap.
        for predicted in [-1.0, 0.0, 0.125, 0.25, 0.5, 1.0] {
            let worst = (predicted - true_intent_world_a)
                .abs()
                .max((predicted - true_intent_world_b).abs());
            assert!(worst >= MAX_BEST_WORST_CASE_ERROR);
        }
    }
    #[test]
    fn seemingly_stable_false_operator_release_can_erase_held_input() {
        let (without_spoof, spoofed, updates) = false_release_counterfactual();
        assert_eq!(without_spoof, 0);
        assert!(updates >= 1);
        assert!(spoofed > 500);
    }
    #[test]
    fn evidence_cannot_be_overwritten() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v66-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir(&dir).unwrap();
        let p = dir.join("blind-review.json");
        run(&p).unwrap();
        let baseline = std::fs::read(&p).unwrap();
        assert!(run(&p).is_err());
        assert_eq!(baseline, std::fs::read(&p).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
