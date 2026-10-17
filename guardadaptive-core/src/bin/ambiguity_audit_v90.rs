//! v0.90: offline, self-contained, synthetic ambiguity audit.
//!
//! The controller signal alone cannot distinguish a held intentional direction
//! from an equal-magnitude unknown drift. This tool measures the false-positive
//! cost of warnings, NOT confirmed physical accuracy.
//! No driver, gamepad, network, training, or real-recording access.
use guardadaptive::{
    guided::GuidedWizard,
    resolution::Resolution,
    unknown_drift_v89::{Alert, UnknownDriftObserver},
    Vec2,
};

const BIAS: Vec2 = Vec2::new(0.05, -0.04);
const RUN_MS: u64 = 5_600;
// 62.5, 125, 250, 500 and 1000 Hz synthetic polling intervals.
const INTERVAL_MS: [u64; 5] = [16, 8, 4, 2, 1];
const RAMP_MS: u64 = 4_000;
const ABRUPT_DRIFT: Vec2 = Vec2::new(0.10, 0.0);
const HELD_MOTION: Vec2 = Vec2::new(0.10, 0.0);
const MICRO: Vec2 = Vec2::new(0.003, -0.002);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Neutral,
    MicroHold,
    HeldDirection,
    AbruptUnknownDrift,
    SlowDriftThenPlateau,
    RapidReversal,
    NeutralNoise,
}
impl Scenario {
    const ALL: [Self; 7] = [
        Self::Neutral,
        Self::MicroHold,
        Self::HeldDirection,
        Self::AbruptUnknownDrift,
        Self::SlowDriftThenPlateau,
        Self::RapidReversal,
        Self::NeutralNoise,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::MicroHold => "micro_hold",
            Self::HeldDirection => "held_intent",
            Self::AbruptUnknownDrift => "abrupt_unknown_drift",
            Self::SlowDriftThenPlateau => "slow_drift_then_plateau",
            Self::RapidReversal => "rapid_reversal",
            Self::NeutralNoise => "neutral_noise",
        }
    }
    fn intentionally_held(self) -> bool {
        matches!(
            self,
            Self::MicroHold | Self::HeldDirection | Self::RapidReversal
        )
    }
    fn raw_intent_drift(self, now_ms: u64, sample: u64) -> (Vec2, Vec2) {
        match self {
            Self::Neutral => (Vec2::ZERO, Vec2::ZERO),
            Self::MicroHold => (MICRO, Vec2::ZERO),
            Self::HeldDirection => (HELD_MOTION, Vec2::ZERO),
            Self::AbruptUnknownDrift => (Vec2::ZERO, ABRUPT_DRIFT),
            Self::SlowDriftThenPlateau => (
                Vec2::ZERO,
                ABRUPT_DRIFT * (now_ms.min(RAMP_MS) as f64 / RAMP_MS as f64),
            ),
            Self::RapidReversal => (
                Vec2::new(
                    if (now_ms / 65).is_multiple_of(2) {
                        0.08
                    } else {
                        -0.08
                    },
                    0.0,
                ),
                Vec2::ZERO,
            ),
            Self::NeutralNoise => (
                Vec2::ZERO,
                Vec2::new(
                    if sample.is_multiple_of(2) {
                        0.00035
                    } else {
                        -0.00035
                    },
                    0.0,
                ),
            ),
        }
    }
}

#[derive(Debug)]
struct Audit {
    scenario: Scenario,
    sample_interval_ms: u64,
    frames: usize,
    ambiguity_episodes: u64,
    ambiguity_frames: u64,
    first_alert_ms: Option<u64>,
    max_intent_error: f64,
    micro_erased: usize,
    real_center_updates: u64,
    unverified_updates: u64,
    invalid_samples: u64,
}

fn synthetic_reference(center: Vec2) -> guardadaptive::reacquire::VerifiedCenter {
    let mut w = GuidedWizard::new();
    w.confirm_release().expect("valid synthetic first stage");
    for i in 0_u32..128 {
        let n = if i.is_multiple_of(2) {
            0.00002
        } else {
            -0.00002
        };
        w.sample(Resolution::XInput16.quantize(center + Vec2::new(n, -n)))
            .expect("synthetic neutral one");
    }
    w.sample((center + Vec2::new(0.18, 0.0)).clamp())
        .expect("synthetic exercise");
    w.confirm_movement_finished().expect("movement complete");
    w.confirm_release().expect("synthetic second release");
    for i in 0_u32..128 {
        let n = if i.is_multiple_of(2) {
            0.00002
        } else {
            -0.00002
        };
        w.sample(Resolution::XInput16.quantize(center + Vec2::new(n, -n)))
            .expect("synthetic neutral two");
    }
    w.commit().expect("synthetic center token")
}

fn run_case(scenario: Scenario, interval_ms: u64) -> Audit {
    assert!(INTERVAL_MS.contains(&interval_ms));
    let mut g = UnknownDriftObserver::new();
    g.accept_reacquisition(synthetic_reference(BIAS))
        .expect("valid synthetic reference");
    let mut result = Audit {
        scenario,
        sample_interval_ms: interval_ms,
        frames: 0,
        ambiguity_episodes: 0,
        ambiguity_frames: 0,
        first_alert_ms: None,
        max_intent_error: 0.0,
        micro_erased: 0,
        real_center_updates: 0,
        unverified_updates: 0,
        invalid_samples: 0,
    };
    let mut sample = 0_u64;
    let mut time_ms = 0_u64;
    while time_ms <= RUN_MS {
        let (intent, extra_drift) = scenario.raw_intent_drift(time_ms, sample);
        let raw = BIAS + intent + extra_drift;
        let observed = g.observe_at(raw, time_ms);
        let error = (observed.output - intent).norm();
        result.frames += 1;
        result.max_intent_error = result.max_intent_error.max(error);
        if intent.norm() > 0.0001 && observed.output.norm() <= 0.000001 {
            result.micro_erased += 1;
        }
        if observed.alert == Alert::MotionOrDriftAmbiguous {
            result.ambiguity_frames += 1;
        }
        if observed.ambiguity_started {
            result.ambiguity_episodes += 1;
            result.first_alert_ms.get_or_insert(time_ms);
        }
        assert_eq!(observed.active_center, g.center());
        assert!(observed.output.finite());
        assert!(observed.output.x.abs() <= 1.0 && observed.output.y.abs() <= 1.0);
        sample += 1;
        time_ms += interval_ms;
    }
    let metrics = g.metrics();
    result.real_center_updates = metrics.reference_updates;
    result.unverified_updates = metrics.unverified_center_updates;
    result.invalid_samples = metrics.invalid_samples;
    assert_eq!(result.ambiguity_episodes, metrics.ambiguous_episode_starts);
    assert_eq!(result.ambiguity_frames, metrics.ambiguous_stable_reports);
    result
}

fn audit_research_constraints(result: &Audit) -> bool {
    let no_learning = result.real_center_updates == 1 && result.unverified_updates == 0;
    let input_safe = !result.scenario.intentionally_held()
        || (result.micro_erased == 0 && result.max_intent_error < 0.002);
    let class_behavior = match result.scenario {
        Scenario::Neutral
        | Scenario::MicroHold
        | Scenario::RapidReversal
        | Scenario::NeutralNoise => result.ambiguity_episodes == 0,
        Scenario::HeldDirection | Scenario::AbruptUnknownDrift | Scenario::SlowDriftThenPlateau => {
            result.ambiguity_episodes == 1 && result.first_alert_ms.is_some()
        }
    };
    no_learning && input_safe && class_behavior && result.invalid_samples == 0
}

fn run_matrix() -> Vec<Audit> {
    let mut results = Vec::new();
    for interval_ms in INTERVAL_MS {
        for scenario in Scenario::ALL {
            results.push(run_case(scenario, interval_ms));
        }
    }
    results
}

fn meaning(row: &Audit) -> &'static str {
    match row.scenario {
        Scenario::HeldDirection if row.ambiguity_episodes > 0 => "FALSE_DRIFT_SUSPICION_ON_INTENT",
        Scenario::AbruptUnknownDrift | Scenario::SlowDriftThenPlateau
            if row.ambiguity_episodes > 0 =>
        {
            "TRUE_SYNTHETIC_DRIFT_SUSPICION"
        }
        _ => "NO_SUSPICION",
    }
}
fn synthetic_accuracy(row: &Audit) -> &'static str {
    match row.scenario {
        Scenario::AbruptUnknownDrift | Scenario::SlowDriftThenPlateau => {
            if row.max_intent_error < 0.009 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        _ => "NOT_A_DRIFT_ACCURACY_TRIAL",
    }
}
fn main() {
    if std::env::args().len() != 2
        || std::env::args().nth(1).as_deref() != Some("--synthetic-matrix")
    {
        eprintln!("Usage: ambiguity_audit_v90 --synthetic-matrix");
        std::process::exit(2);
    }
    let rows = run_matrix();
    println!("GA_V90 source=SYNTHETIC_ONLY intent_truth=GENERATED physical_acceptance=BLOCKED");
    println!("COLUMNS scenario,interval_ms,samples,ambiguity_episodes,ambiguity_frames,first_alert_ms,max_intent_error_pct,erased_intent,unverified_center_updates,observer_regression_gate,synthetic_drift_accuracy,warning_label");
    for row in &rows {
        println!(
            "{},{},{},{},{},{},{:.5},{},{},{},{},{}",
            row.scenario.name(),
            row.sample_interval_ms,
            row.frames,
            row.ambiguity_episodes,
            row.ambiguity_frames,
            row.first_alert_ms
                .map(|n| n.to_string())
                .unwrap_or_else(|| "none".into()),
            row.max_intent_error * 100.0,
            row.micro_erased,
            row.unverified_updates,
            if audit_research_constraints(row) {
                "PASS"
            } else {
                "FAIL"
            },
            synthetic_accuracy(row),
            meaning(row)
        );
    }
    let passed = rows
        .iter()
        .filter(|r| audit_research_constraints(r))
        .count();
    println!(
        "OBSERVER_REGRESSION_GATE={passed}/{} NOTE=NOT_A_DRIFT_CORRECTION_SUCCESS_RATE",
        rows.len()
    );
    let false_suspicions = rows
        .iter()
        .filter(|r| meaning(r) == "FALSE_DRIFT_SUSPICION_ON_INTENT")
        .count();
    let synthetic_drift_alerts = rows
        .iter()
        .filter(|r| meaning(r) == "TRUE_SYNTHETIC_DRIFT_SUSPICION")
        .count();
    let drift_accuracy_failures = rows
        .iter()
        .filter(|r| synthetic_accuracy(r) == "FAIL")
        .count();
    println!(
        "HELD_INTENT_FALSE_DRIFT_SUSPICIONS={false_suspicions}/{}",
        INTERVAL_MS.len()
    );
    println!(
        "SYNTHETIC_DRIFT_ALERTS={synthetic_drift_alerts}/{}",
        2 * INTERVAL_MS.len()
    );
    println!(
        "UNKNOWN_DRIFT_PRECISION_FAILURES={drift_accuracy_failures}/{} TARGET_LT_0_9_PERCENT",
        2 * INTERVAL_MS.len()
    );
    println!("IDENTIFIABILITY=HELD_INTENT_AND_ABRUPT_DRIFT_ARE_IDENTICAL_RAW_INPUT");
    println!("RUNTIME_GAME_OUTPUT=DISABLED PRODUCTION=BLOCKED");
    if passed != rows.len() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_matrix_has_no_hidden_center_learning_or_erased_micro() {
        let result = run_matrix();
        assert_eq!(result.len(), Scenario::ALL.len() * INTERVAL_MS.len());
        assert!(result.iter().all(audit_research_constraints));
    }
    #[test]
    fn held_intent_and_sudden_unknown_drift_have_identical_observations() {
        for hz_ms in INTERVAL_MS {
            let held = run_case(Scenario::HeldDirection, hz_ms);
            let drift = run_case(Scenario::AbruptUnknownDrift, hz_ms);
            assert_eq!(held.ambiguity_episodes, drift.ambiguity_episodes);
            assert_eq!(held.first_alert_ms, drift.first_alert_ms);
            assert_eq!(held.ambiguity_frames, drift.ambiguity_frames);
            assert!(held.first_alert_ms.is_some());
            // The same correction output is correct for held input but WRONG for drift.
            assert!(held.max_intent_error < 0.002);
            assert!(drift.max_intent_error > 0.09);
        }
    }
    #[test]
    fn static_micro_motion_survives_below_one_percent_without_deadzone() {
        for hz_ms in INTERVAL_MS {
            let x = run_case(Scenario::MicroHold, hz_ms);
            assert_eq!(x.micro_erased, 0);
            assert!(x.max_intent_error < 0.002);
            assert_eq!(x.ambiguity_episodes, 0);
        }
    }
    #[test]
    fn slow_drift_does_not_get_recategorized_as_proven_neutral() {
        for hz_ms in INTERVAL_MS {
            let x = run_case(Scenario::SlowDriftThenPlateau, hz_ms);
            assert_eq!(x.unverified_updates, 0);
            assert!(x.max_intent_error > 0.09);
            assert_eq!(x.ambiguity_episodes, 1);
            assert!(x.first_alert_ms.unwrap() >= RAMP_MS + 650);
        }
    }
    #[test]
    fn high_frequency_reversals_and_noise_do_not_falsely_trigger() {
        for hz_ms in INTERVAL_MS {
            for s in [Scenario::RapidReversal, Scenario::NeutralNoise] {
                let x = run_case(s, hz_ms);
                assert_eq!(x.ambiguity_episodes, 0);
                assert_eq!(x.unverified_updates, 0);
            }
        }
    }
}
