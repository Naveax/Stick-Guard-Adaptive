//! v0.74 train-only selection of a bounded, stable-window drift hypothesis.
//! Strictly synthetic OFFLINE research. Raw-only stable samples cannot prove
//! a physical stick release and NEVER grant production recenter permission.
use crate::drift_holdout_v40::{
    aggregated_gain, evaluate_suite, Candidate, Pair, CASES, HOLDOUT, TRAIN,
};
use std::{fmt::Write as _, io, path::Path};

const NEW_PROBE_SEEDS: [u64; 2] = [20280019, 20280023];
const CONFIGS: [(f64, f64); 4] = [(0.05, 0.035), (0.25, 0.035), (0.05, 0.08), (0.25, 0.08)];
// A deliberately overpermissive ABALATION: diagnostics only, never selection.
const LOOSE_ABLATION: Candidate = Candidate::BoundedStable {
    blend: 0.25,
    max_total: 0.40,
};
const MIN_GLOBAL_GAIN: f64 = 0.01;
const MAX_RMSE_REGRESSION_FACTOR: f64 = 1.001;

fn meets_synthetic_metrics(rows: &[Pair; 6]) -> bool {
    aggregated_gain(rows) >= MIN_GLOBAL_GAIN
        && rows.iter().all(|p| {
            p.adaptive.errors <= p.fixed.errors * MAX_RMSE_REGRESSION_FACTOR
                && p.adaptive.micro_lost <= p.fixed.micro_lost
                && p.adaptive.wrong_dir <= p.fixed.wrong_dir
                && p.adaptive.hold_lost <= p.fixed.hold_lost
        })
}
#[derive(Clone)]
pub struct Audit {
    pub chosen_blend: f64,
    pub chosen_max_total: f64,
    pub training_gain: f64,
    pub locked_gain: f64,
    pub new_probe_gain: f64,
    pub train_rows: [Pair; 6],
    pub locked_rows: [Pair; 6],
    pub new_probe_rows: [Pair; 6],
    pub locked_regressions: usize,
    pub new_probe_regressions: usize,
    pub locked_synthetic_quality_passed: bool,
    pub new_probe_synthetic_quality_passed: bool,
    pub loose_ablation_updates: u64,
    pub loose_ablation_hold_losses: u64,
    pub loose_ablation_regressions: usize,
}
fn regressed(p: Pair) -> bool {
    p.adaptive.errors > p.fixed.errors * MAX_RMSE_REGRESSION_FACTOR
        || p.adaptive.micro_lost > p.fixed.micro_lost
        || p.adaptive.wrong_dir > p.fixed.wrong_dir
        || p.adaptive.hold_lost > p.fixed.hold_lost
}
pub fn evaluate() -> io::Result<Audit> {
    // The parameter choice sees ONLY the old v40 TRAIN seeds.
    // Neither original locked holdout nor new probe seeds can choose a knob.
    let mut winner: Option<(f64, f64, [Pair; 6])> = None;
    for (blend, max_total) in CONFIGS {
        let train = evaluate_suite(&TRAIN, Candidate::BoundedStable { blend, max_total });
        if !train.iter().all(|p| {
            p.adaptive.errors.is_finite()
                && p.fixed.errors.is_finite()
                && p.adaptive.frames == p.fixed.frames
        }) {
            return Err(io::Error::other("nonfinite/mismatched training metrics"));
        }
        if winner
            .as_ref()
            .is_none_or(|w| aggregated_gain(&train) > aggregated_gain(&w.2))
        {
            winner = Some((blend, max_total, train));
        }
    }
    let (chosen_blend, chosen_max_total, train_rows) =
        winner.ok_or_else(|| io::Error::other("no predeclared configurations"))?;
    let candidate = Candidate::BoundedStable {
        blend: chosen_blend,
        max_total: chosen_max_total,
    };
    let locked_rows = evaluate_suite(&HOLDOUT, candidate);
    let new_probe_rows = evaluate_suite(&NEW_PROBE_SEEDS, candidate);
    if !locked_rows.iter().chain(new_probe_rows.iter()).all(|p| {
        p.adaptive.errors.is_finite()
            && p.fixed.errors.is_finite()
            && p.adaptive.frames == p.fixed.frames
    }) {
        return Err(io::Error::other("invalid holdout metrics"));
    }
    let loose_rows = evaluate_suite(&HOLDOUT, LOOSE_ABLATION);
    let loose_updates = loose_rows.iter().map(|p| p.adaptive.reapprovals).sum();
    let loose_hold_losses = loose_rows.iter().map(|p| p.adaptive.hold_lost).sum();
    let loose_regressions = loose_rows.iter().filter(|p| regressed(**p)).count();
    let locked_regressions = locked_rows.iter().filter(|p| regressed(**p)).count();
    let new_probe_regressions = new_probe_rows.iter().filter(|p| regressed(**p)).count();
    Ok(Audit {
        chosen_blend,
        chosen_max_total,
        training_gain: aggregated_gain(&train_rows),
        locked_gain: aggregated_gain(&locked_rows),
        new_probe_gain: aggregated_gain(&new_probe_rows),
        train_rows,
        locked_rows,
        new_probe_rows,
        locked_regressions,
        new_probe_regressions,
        locked_synthetic_quality_passed: meets_synthetic_metrics(&locked_rows),
        new_probe_synthetic_quality_passed: meets_synthetic_metrics(&new_probe_rows),
        loose_ablation_updates: loose_updates,
        loose_ablation_hold_losses: loose_hold_losses,
        loose_ablation_regressions: loose_regressions,
    })
}
fn rows_json(rows: &[Pair; 6]) -> String {
    let mut content = String::new();
    for (i, (name, pair)) in CASES.iter().zip(rows).enumerate() {
        if i > 0 {
            content.push(',');
        }
        let _ = write!(
            content,
            concat!(
                "{{\"regime\":\"{}\",\"fixed_rmse\":{:.9},",
                "\"bounded_rmse\":{:.9},\"gain\":{:.9},",
                "\"fixed_micro_lost\":{},\"bounded_micro_lost\":{},",
                "\"fixed_wrong_dir\":{},\"bounded_wrong_dir\":{},",
                "\"fixed_long_hold_lost\":{},\"bounded_long_hold_lost\":{},",
                "\"candidate_center_updates\":{},\"rejected_stable_windows\":{},",
                "\"safety_regression\":{}}}"
            ),
            name,
            pair.fixed.rmse(),
            pair.adaptive.rmse(),
            pair.gain(),
            pair.fixed.micro_lost,
            pair.adaptive.micro_lost,
            pair.fixed.wrong_dir,
            pair.adaptive.wrong_dir,
            pair.fixed.hold_lost,
            pair.adaptive.hold_lost,
            pair.adaptive.reapprovals,
            pair.adaptive.rejected_windows,
            regressed(*pair)
        );
    }
    content
}
pub fn run(out: &Path) -> io::Result<()> {
    let result = evaluate()?;
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v74_offline_bounded_stability_hypothesis\",",
            "\"candidate_family\":\"train_only_bounded_stable_window\",",
            "\"training_seeds\":[{},{}],\"locked_holdout_seeds\":[{},{}],",
            "\"additional_probe_seeds\":[{},{}],\"configurations_predeclared\":4,",
            "\"selected_blend\":{:.6},\"selected_max_total\":{:.6},",
            "\"train_global_gain\":{:.9},\"locked_global_gain\":{:.9},",
            "\"additional_probe_global_gain\":{:.9},",
            "\"locked_regressed_regimes\":{},\"additional_probe_regressed_regimes\":{},",
            "\"locked_synthetic_gate\":{},\"additional_probe_synthetic_gate\":{},",
            "\"train\":[{}],\"locked_holdout\":[{}],\"additional_probe\":[{}],",
            "\"loose_ablation_updates\":{},\"loose_ablation_hold_losses\":{},",
            "\"loose_ablation_regressions\":{},\"holdouts_used_for_parameter_selection\":false,",
            "\"physical_release_authenticated\":false,\"raw_only_ambiguous\":true,",
            "\"current_blind_v66_quality_failure_preserved\":true,",
            "\"even_synthetic_gate_pass_cannot_authorize_live_update\":true,",
            "\"live_center_policy_changed\":false,\"model_promoted\":false,",
            "\"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        TRAIN[0],
        TRAIN[1],
        HOLDOUT[0],
        HOLDOUT[1],
        NEW_PROBE_SEEDS[0],
        NEW_PROBE_SEEDS[1],
        result.chosen_blend,
        result.chosen_max_total,
        result.training_gain,
        result.locked_gain,
        result.new_probe_gain,
        result.locked_regressions,
        result.new_probe_regressions,
        result.locked_synthetic_quality_passed,
        result.new_probe_synthetic_quality_passed,
        rows_json(&result.train_rows),
        rows_json(&result.locked_rows),
        rows_json(&result.new_probe_rows),
        result.loose_ablation_updates,
        result.loose_ablation_hold_losses,
        result.loose_ablation_regressions
    );
    crate::truth_lab::save_new(out, &report)?;
    println!(
        "V074_BOUNDED_RESEARCH=PASS blend={:.3} cap={:.3} locked_gate={} new_probe_gate={} authorized=FALSE production=BLOCKED",
        result.chosen_blend,
        result.chosen_max_total,
        result.locked_synthetic_quality_passed,
        result.new_probe_synthetic_quality_passed
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn research_has_deterministic_train_only_selection() {
        let a = evaluate().unwrap();
        let b = evaluate().unwrap();
        assert_eq!(a.chosen_blend, b.chosen_blend);
        assert_eq!(a.chosen_max_total, b.chosen_max_total);
        assert!(CONFIGS.contains(&(a.chosen_blend, a.chosen_max_total)));
        assert_eq!(a.locked_rows.len(), CASES.len());
        assert_eq!(a.new_probe_rows.len(), CASES.len());
        assert!(a.loose_ablation_updates > 0);
        assert!(a.loose_ablation_hold_losses > 0);
        assert!(a.loose_ablation_regressions > 0);
    }
    #[test]
    fn per_regime_harm_cannot_hide_behind_positive_global_gain() {
        let mut rows = evaluate_suite(&TRAIN, Candidate::Fixed);
        assert!(!meets_synthetic_metrics(&rows));
        rows[4].adaptive.micro_lost = rows[4].fixed.micro_lost + 1;
        assert!(regressed(rows[4]));
        rows[4].adaptive.micro_lost = rows[4].fixed.micro_lost;
        rows[4].adaptive.wrong_dir = rows[4].fixed.wrong_dir + 1;
        assert!(regressed(rows[4]));
        rows[4].adaptive.wrong_dir = rows[4].fixed.wrong_dir;
        rows[4].adaptive.hold_lost = rows[4].fixed.hold_lost + 1;
        assert!(regressed(rows[4]));
        rows[4].adaptive.hold_lost = rows[4].fixed.hold_lost;
        rows[4].adaptive.errors = rows[4].fixed.errors * 1.002;
        assert!(regressed(rows[4]));
    }
    #[test]
    fn evidence_create_new_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("ga-v74-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("bounded.json");
        run(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(run(&path).is_err());
        assert_eq!(bytes, std::fs::read(&path).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
