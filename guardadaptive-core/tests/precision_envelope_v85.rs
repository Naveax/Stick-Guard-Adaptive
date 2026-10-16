//! GuardAdaptive v0.85: synthetic precision envelope, not a physical device claim.
use guardadaptive::{resolution::Resolution, Config, GuardAdaptive, InputMode, OutputPolicy, Vec2};

const GOAL: f64 = 0.009; // strictly below 0.9% radial residual
const MICRO_TOLERANCE: f64 = 0.002;

fn candidate() -> GuardAdaptive {
    GuardAdaptive::with_policy(
        Config {
            auto_alpha: 0.0,
            ..Config::default()
        },
        OutputPolicy::PreserveMicro,
    )
}

fn input(raw: Vec2) -> Vec2 {
    Resolution::XInput16.quantize(raw.clamp())
}

fn calibrate(g: &mut GuardAdaptive, known_neutral: Vec2) {
    // The *synthetic oracle* establishes true neutral; real hardware cannot.
    for tick in 0..320 {
        let jitter = if tick % 2 == 0 { 0.0002 } else { -0.0002 };
        g.process(
            input(known_neutral + Vec2::new(jitter, -jitter)),
            InputMode::CalibrateNeutral,
        );
    }
    g.begin_live();
}

#[test]
fn constant_signed_ten_percent_grid_target_and_micro_preservation() {
    let mut worst_neutral: f64 = 0.0;
    let mut worst_micro_error: f64 = 0.0;
    let movements = [
        Vec2::new(0.0002, 0.0),
        Vec2::new(-0.0007, 0.0),
        Vec2::new(0.0, 0.001),
        Vec2::new(0.002, -0.001),
        Vec2::new(-0.005, 0.004),
        Vec2::new(0.01, -0.012),
        Vec2::new(0.025, 0.03),
        Vec2::new(-0.05, -0.04),
    ];
    let mut cases = 0_usize;
    for x in -10..=10 {
        for y in -10..=10 {
            let drift = Vec2::new(x as f64 * 0.01, y as f64 * 0.01);
            let mut g = candidate();
            calibrate(&mut g, drift);
            for step in 0..48 {
                let jitter = if step % 2 == 0 { 0.00022 } else { -0.00022 };
                let out = g.process(input(drift + Vec2::new(jitter, -jitter)), InputMode::Live);
                worst_neutral = worst_neutral.max(out.norm());
            }
            for (step, intention) in movements.iter().enumerate() {
                let jitter = if step % 2 == 0 { 0.00018 } else { -0.00018 };
                let out = g.process(
                    input(drift + *intention + Vec2::new(jitter, -jitter)),
                    InputMode::Live,
                );
                worst_micro_error = worst_micro_error.max((out - *intention).norm());
                assert!(out.norm() > 0.000001, "lost micro input at {x}, {y}");
            }
            assert_eq!(g.telemetry().auto_updates, 0);
            assert_eq!(g.telemetry().effective_deadzone, 0.0);
            cases += 1;
        }
    }
    println!(
        "GA_V85_STATIC_GRID cases={} max_neutral_pct={:.4} max_intended_error_pct={:.4} target_pct=0.9 synthetic=true",
        cases, worst_neutral * 100.0, worst_micro_error * 100.0
    );
    assert_eq!(cases, 441);
    assert!(
        worst_neutral < GOAL,
        "verified synthetic neutral target FAIL"
    );
    assert!(worst_micro_error < MICRO_TOLERANCE, "micro fidelity FAIL");
}

#[test]
fn unverified_sudden_jump_must_not_be_silently_recentered() {
    let mut g = candidate();
    calibrate(&mut g, Vec2::ZERO);
    let new_drift = Vec2::new(0.10, -0.10);
    for _ in 0..5000 {
        assert!(g.process(new_drift, InputMode::Live).norm() > GOAL);
    }
    assert_eq!(g.telemetry().auto_updates, 0);
    println!("GA_V85_ABRUPT_UNVERIFIED target_unmet=true no_blind_update=true");
    // The same shift becomes correctable only after an externally verified re-release.
    calibrate(&mut g, new_drift);
    assert!(g.process(new_drift, InputMode::Live).norm() < GOAL);
}

#[test]
fn false_neutral_acknowledgment_is_fundamentally_ambiguous() {
    let mut g = candidate();
    // The model cannot tell drift=1% from a deliberate 1% held movement.
    calibrate(&mut g, Vec2::new(0.01, 0.0));
    assert!(g.process(Vec2::ZERO, InputMode::Live).norm() > GOAL);
    assert_eq!(g.telemetry().auto_updates, 0);
    println!("GA_V85_FALSE_NEUTRAL target_unmet=true reliable_release_required=true");
}

#[test]
fn held_micro_movement_remains_intact_over_ten_thousand_frames() {
    let mut g = candidate();
    calibrate(&mut g, Vec2::ZERO);
    let intention = Vec2::new(0.003, -0.002);
    let mut out = Vec2::ZERO;
    for _ in 0..10000 {
        out = g.process(intention, InputMode::Live);
    }
    assert!((out - intention).norm() < 0.0005);
    assert_eq!(g.telemetry().auto_updates, 0);
}
