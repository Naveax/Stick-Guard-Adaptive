//! v0.73 research-side fail-closed admission for ambiguous raw-only drift.
//!
//! This is deliberately NOT the live calibration/output path. Provenance
//! claims are not a substitute for independent physical intention evidence.
use crate::{blind_review_v66, Vec2};
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseClaim {
    Absent,
    UnverifiedExternal,
    SyntheticTestOracle,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NonfiniteOrOutOfRange,
    FailedQualityHoldout,
    AbsentPhysicalReleaseProof,
    UnverifiedPhysicalReleaseProof,
    SyntheticOracleNotPhysicalProof,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub retained_center: Vec2,
    pub auto_update_permitted: bool,
    pub reason: Refusal,
}
fn valid(v: Vec2) -> bool {
    v.x.is_finite() && v.y.is_finite() && (-1.0..=1.0).contains(&v.x) && (-1.0..=1.0).contains(&v.y)
}
/// Never turn an arbitrary caller-declared label into physical authorization.
/// No public variant can authorize a live recenter. Every path preserves the
/// last explicitly approved center exactly, not just approximately.
pub fn refuse_unproven_update(
    approved_center: Vec2,
    observed: Vec2,
    release: ReleaseClaim,
    locked_holdout_passed: bool,
    shadow_holdout_passed: bool,
) -> Decision {
    let reason = if !valid(approved_center) || !valid(observed) {
        Refusal::NonfiniteOrOutOfRange
    } else if !locked_holdout_passed || !shadow_holdout_passed {
        Refusal::FailedQualityHoldout
    } else {
        match release {
            ReleaseClaim::Absent => Refusal::AbsentPhysicalReleaseProof,
            ReleaseClaim::UnverifiedExternal => Refusal::UnverifiedPhysicalReleaseProof,
            ReleaseClaim::SyntheticTestOracle => Refusal::SyntheticOracleNotPhysicalProof,
        }
    };
    Decision {
        retained_center: approved_center,
        auto_update_permitted: false,
        reason,
    }
}
/// Two physical worlds, equal observable raw value and previous center, but
/// incompatible true intentions. No raw-only estimator can be correct in both.
fn raw_equivalence_witness() -> (f64, f64, f64, f64) {
    let observed = 0.50_f64;
    let drift_a = 0.25_f64; // true held-intent x=+0.25
    let drift_b = 0.50_f64; // actual neutral intent x=0
    let intent_a = observed - drift_a;
    let intent_b = observed - drift_b;
    let minimax_bound = (intent_a - intent_b).abs() / 2.0;
    (observed, intent_a, intent_b, minimax_bound)
}
#[derive(Clone, Debug)]
pub struct Audit {
    pub tested: usize,
    pub frozen: usize,
    pub invalid_input_refusals: usize,
    pub quality_refusals: usize,
    pub absent_refusals: usize,
    pub unverified_refusals: usize,
    pub synthetic_refusals: usize,
    pub witness_error_lower_bound: f64,
    pub original_quality_passed: bool,
    pub shadow_quality_passed: bool,
    pub original_quality_regressed_regimes: usize,
    pub shadow_quality_regressed_regimes: usize,
    pub false_release_held_frames_lost: u64,
}
pub fn audit(rounds: usize) -> io::Result<Audit> {
    if !(128..=65_536).contains(&rounds) || !rounds.is_multiple_of(16) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be 128..65536 and divisible by 16",
        ));
    }
    let historical = blind_review_v66::evaluate()?;
    // This is a READ-ONLY review of already locked and shadow evidence.
    if historical.blind_original_holdout_gate
        || historical.blind_shadow_strict_gate
        || historical.blind_original_regression_count != 6
        || historical.blind_shadow_regression_count != 6
        || historical.original_held_intent_lost != 0
        || historical.mislabeled_held_intent_lost < 500
        || !historical.no_auto_learning
    {
        return Err(io::Error::other("original blind safety evidence changed"));
    }
    let (observed, intent_a, intent_b, bound) = raw_equivalence_witness();
    if observed != 0.5 || intent_a != 0.25 || intent_b != 0.0 || bound != 0.125 {
        return Err(io::Error::other("raw equivalence witness changed"));
    }
    let mut out = Audit {
        tested: 0,
        frozen: 0,
        invalid_input_refusals: 0,
        quality_refusals: 0,
        absent_refusals: 0,
        unverified_refusals: 0,
        synthetic_refusals: 0,
        witness_error_lower_bound: bound,
        original_quality_passed: historical.blind_original_holdout_gate,
        shadow_quality_passed: historical.blind_shadow_strict_gate,
        original_quality_regressed_regimes: historical.blind_original_regression_count,
        shadow_quality_regressed_regimes: historical.blind_shadow_regression_count,
        false_release_held_frames_lost: historical.mislabeled_held_intent_lost,
    };
    for i in 0..rounds {
        // Each 16 cases cover 2 quality gates * 3 evidence classes,
        // finite observations and poison/invalid proposals.
        let approved = Vec2::new(0.25, -0.14);
        let observed = match i % 16 {
            0 => Vec2::new(f64::NAN, 0.3),
            1 => Vec2::new(f64::INFINITY, 0.2),
            2 => Vec2::new(1.01, -0.3),
            3 => Vec2::new(0.25, -1.01),
            4 => Vec2::new(-0.25, f64::NEG_INFINITY),
            5 => Vec2::new(f64::NEG_INFINITY, 0.4),
            6 => Vec2::new(0.5, f64::NAN),
            7 => Vec2::new(0.8, 1.2),
            _ => {
                let n = (i / 16) as f64;
                Vec2::new((n * 0.011).sin() * 0.7, (n * 0.017).cos() * 0.6)
            }
        };
        let release = match (i / 16) % 3 {
            0 => ReleaseClaim::Absent,
            1 => ReleaseClaim::UnverifiedExternal,
            _ => ReleaseClaim::SyntheticTestOracle,
        };
        let locked_ok = (i / 16) % 2 == 0;
        let shadow_ok = (i / 32) % 2 == 0;
        let decision = refuse_unproven_update(approved, observed, release, locked_ok, shadow_ok);
        if decision.auto_update_permitted || decision.retained_center != approved {
            return Err(io::Error::other("unsafe center update reached"));
        }
        match decision.reason {
            Refusal::NonfiniteOrOutOfRange => out.invalid_input_refusals += 1,
            Refusal::FailedQualityHoldout => out.quality_refusals += 1,
            Refusal::AbsentPhysicalReleaseProof => out.absent_refusals += 1,
            Refusal::UnverifiedPhysicalReleaseProof => out.unverified_refusals += 1,
            Refusal::SyntheticOracleNotPhysicalProof => out.synthetic_refusals += 1,
        }
        out.tested += 1;
        out.frozen += 1;
    }
    if out.frozen != rounds
        || out.invalid_input_refusals == 0
        || out.quality_refusals == 0
        || out.absent_refusals == 0
        || out.unverified_refusals == 0
        || out.synthetic_refusals == 0
    {
        return Err(io::Error::other("missing refusal class in synthetic audit"));
    }
    Ok(out)
}
pub fn run(rounds: usize, path: &Path) -> io::Result<()> {
    let x = audit(rounds)?;
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v73_offline_ambiguity_freeze_gate\",",
            "\"tested_synthetic_proposals\":{},\"unchanged_approved_center\":{},",
            "\"invalid_input_refusals\":{},\"failed_quality_refusals\":{},",
            "\"unknown_release_refusals\":{},\"unverified_external_release_refusals\":{},",
            "\"synthetic_oracle_not_physical_refusals\":{},",
            "\"identical_observed_x\":0.5,\"incompatible_true_intents_x\":[0.25,0.0],",
            "\"minimax_absolute_error_lower_bound\":{:.6},",
            "\"prior_original_blind_holdout_gate\":{},\"prior_shadow_blind_holdout_gate\":{},",
            "\"prior_original_regressed_regimes\":{},\"prior_shadow_regressed_regimes\":{},",
            "\"false_release_held_intent_lost_frames\":{},",
            "\"external_physical_intent_authenticated\":false,",
            "\"live_center_policy_wired_to_production\":false,",
            "\"all_hardware_write_paths_unchanged\":true,",
            "\"model_trained\":false,\"automatic_center_update_allowed\":false,",
            "\"model_promoted\":false,\"driver_writes\":0,\"game_input_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        x.tested,
        x.frozen,
        x.invalid_input_refusals,
        x.quality_refusals,
        x.absent_refusals,
        x.unverified_refusals,
        x.synthetic_refusals,
        x.witness_error_lower_bound,
        x.original_quality_passed,
        x.shadow_quality_passed,
        x.original_quality_regressed_regimes,
        x.shadow_quality_regressed_regimes,
        x.false_release_held_frames_lost
    );
    crate::truth_lab::save_new(path, &report)?;
    println!(
        "V073_AMBIGUITY_FENCE=PASS tested={} auto_center_updates=0 blind_quality=FAILED production=BLOCKED",
        x.tested
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_identifiability_bound_holds_for_any_candidate_output() {
        let (_, ia, ib, bound) = raw_equivalence_witness();
        for i in -1000..=1000 {
            let output = i as f64 / 1000.0;
            let worst = (output - ia).abs().max((output - ib).abs());
            assert!(worst + 1e-12 >= bound);
        }
        assert_eq!(bound, 0.125);
    }
    #[test]
    fn all_release_labels_are_untrusted_for_live_authorization() {
        let approved = Vec2::new(0.25, -0.14);
        for evidence in [
            ReleaseClaim::Absent,
            ReleaseClaim::UnverifiedExternal,
            ReleaseClaim::SyntheticTestOracle,
        ] {
            for original in [false, true] {
                for shadow in [false, true] {
                    let d = refuse_unproven_update(
                        approved,
                        Vec2::new(0.5, 0.1),
                        evidence,
                        original,
                        shadow,
                    );
                    assert_eq!(d.retained_center, approved);
                    assert!(!d.auto_update_permitted);
                }
            }
        }
    }
    #[test]
    fn invalid_nonfinite_and_out_of_range_proposals_fail_closed() {
        for candidate in [
            Vec2::new(f64::NAN, 0.0),
            Vec2::new(f64::INFINITY, 0.1),
            Vec2::new(-1.01, 0.0),
            Vec2::new(0.0, -1.01),
        ] {
            let d = refuse_unproven_update(
                Vec2::new(0.25, -0.14),
                candidate,
                ReleaseClaim::SyntheticTestOracle,
                true,
                true,
            );
            assert_eq!(d.reason, Refusal::NonfiniteOrOutOfRange);
            assert!(!d.auto_update_permitted);
        }
    }
    #[test]
    fn independent_1024_case_matrix_keeps_approved_center() {
        let x = audit(1024).unwrap();
        assert_eq!(x.tested, 1024);
        assert_eq!(x.frozen, 1024);
        assert_eq!(x.witness_error_lower_bound, 0.125);
        assert!(!x.original_quality_passed);
        assert!(!x.shadow_quality_passed);
    }
    #[test]
    fn invalid_round_counts_denied() {
        for n in [0, 1, 127, 129, 65_537, usize::MAX] {
            assert!(audit(n).is_err());
        }
    }
}
