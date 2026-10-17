//! GuardAdaptive v0.88 offline precision-acceptance scorer.
//! Data labels are NOT authenticated; NEVER a physical-release/production gate.
//! CSV stdin: phase,tick_ms,raw_x,raw_y,intended_x,intended_y,output_x,output_y
//! Phases: neutral, micro, hold, reversal, abrupt (each >=32 observations).
use guardadaptive::{Config, GuardAdaptive, InputMode, OutputPolicy, Vec2};
use std::io::{self, BufRead};

const PHASES: [&str; 5] = ["neutral", "micro", "hold", "reversal", "abrupt"];
const MIN_PER_PHASE: usize = 32;
const MAX_RECORDS: usize = 9000;
const NEUTRAL_GOAL: f64 = 0.009; // strictly below 0.9% radial
const INTENDED_ERROR_GOAL: f64 = 0.002; // <0.2 percentage points
const ABRUPT_GOAL: f64 = 0.009; // intentionally tested; may FAIL
#[derive(Clone, Copy, Debug)]
struct Sample {
    phase: usize,
    ms: u64,
    raw: Vec2,
    intended: Vec2,
    output: Vec2,
}
#[derive(Default, Clone, Copy, Debug)]
struct Metrics {
    count: usize,
    max_error: f64,
    sq_error: f64,
    erased_micro: usize,
    reversed_intent: usize,
}
impl Metrics {
    fn include(&mut self, item: Sample) {
        let err = (item.output - item.intended).norm();
        self.count += 1;
        self.max_error = self.max_error.max(err);
        self.sq_error += err * err;
        if item.intended.norm() >= 0.0001 && item.output.norm() < 0.000001 {
            self.erased_micro += 1;
        }
        if item.intended.norm() >= 0.0005
            && (item.intended.x * item.output.x + item.intended.y * item.output.y) < 0.
        {
            self.reversed_intent += 1;
        }
    }
    fn rms(&self) -> f64 {
        (self.sq_error / self.count.max(1) as f64).sqrt()
    }
}
#[derive(Debug)]
struct Report {
    metrics: [Metrics; 5],
    constraints_pass: bool,
}
fn validate_and_score(data: &[Sample]) -> Result<Report, String> {
    if data.len() < PHASES.len() * MIN_PER_PHASE || data.len() > MAX_RECORDS {
        return Err("Incomplete or excessive sample count".into());
    }
    let mut counts = [Metrics::default(); 5];
    let mut prev_stage = 0usize;
    let mut last_time = None;
    for row in data {
        if row.phase >= PHASES.len() || row.phase < prev_stage || row.phase > prev_stage + 1 {
            return Err("Phase sequence invalid".into());
        }
        if last_time.is_some_and(|x| row.ms <= x) || row.ms > 180000 {
            return Err("Timestamp order invalid".into());
        }
        if [row.raw, row.intended, row.output]
            .iter()
            .any(|x| !x.finite() || x.x.abs() > 1.0 || x.y.abs() > 1.0)
        {
            return Err("Non-finite or out-of-range axis".into());
        }
        // Intent is an *external annotation*, not a signal exposed to the filter.
        if row.phase == 0 && row.intended != Vec2::ZERO {
            return Err("Neutral phase requires independently documented zero intent".into());
        }
        if (row.phase == 1 || row.phase == 2 || row.phase == 3) && row.intended.norm() < 0.0001 {
            return Err("Intentional-motion phase contains neutral intention".into());
        }
        if row.phase == 4 && row.intended != Vec2::ZERO {
            return Err("Abrupt-release phase needs stated zero intent".into());
        }
        counts[row.phase].include(*row);
        last_time = Some(row.ms);
        prev_stage = row.phase;
    }
    if counts.iter().any(|x| x.count < MIN_PER_PHASE) {
        return Err("Every phase needs >=32 samples".into());
    }
    let neutral = counts[0].max_error < NEUTRAL_GOAL;
    let movement = counts[1..4].iter().all(|x| {
        x.max_error < INTENDED_ERROR_GOAL && x.erased_micro == 0 && x.reversed_intent == 0
    });
    let abrupt = counts[4].max_error < ABRUPT_GOAL;
    Ok(Report {
        metrics: counts,
        constraints_pass: neutral && movement && abrupt,
    })
}
fn parse(data: impl BufRead) -> Result<Vec<Sample>, String> {
    let mut records = Vec::new();
    for line in data.lines() {
        let line = line.map_err(|_| "Read failure")?;
        if line.trim().is_empty() {
            continue;
        }
        let vals: Vec<&str> = line.trim().split(',').collect();
        if vals.len() != 8 {
            return Err("Require 8 comma-separated columns".into());
        }
        let phase = PHASES
            .iter()
            .position(|s| *s == vals[0])
            .ok_or("Unknown phase; no unlabelled data accepted")?;
        let ms = vals[1].parse::<u64>().map_err(|_| "Bad timestamp")?;
        let mut f = [0.0; 6];
        for (i, value) in f.iter_mut().enumerate() {
            *value = vals[i + 2].parse().map_err(|_| "Invalid decimal axis")?;
        }
        records.push(Sample {
            phase,
            ms,
            raw: Vec2::new(f[0], f[1]),
            intended: Vec2::new(f[2], f[3]),
            output: Vec2::new(f[4], f[5]),
        });
        if records.len() > MAX_RECORDS {
            return Err("Too many records".into());
        }
    }
    Ok(records)
}
fn synthetic_case(offset: Vec2) -> Vec<Sample> {
    let mut filter = GuardAdaptive::with_policy(
        Config {
            auto_alpha: 0.0,
            ..Config::default()
        },
        OutputPolicy::PreserveMicro,
    );
    for _ in 0..320 {
        filter.process(offset, InputMode::CalibrateNeutral);
    }
    filter.begin_live();
    let mut rows = Vec::with_capacity(5 * MIN_PER_PHASE);
    let mut ms = 0_u64;
    for phase in 0..PHASES.len() {
        for step in 0..MIN_PER_PHASE {
            let desired = match phase {
                0 | 4 => Vec2::ZERO,
                1 => Vec2::new(if step % 2 == 0 { 0.003 } else { -0.003 }, 0.001),
                2 => Vec2::new(0.004, -0.002),
                3 => Vec2::new(if (step / 4) % 2 == 0 { 0.04 } else { -0.04 }, 0.01),
                _ => unreachable!(),
            };
            // Abrupt drift change: no confirmation, and no blind center update.
            let observed_center = if phase == 4 {
                offset + Vec2::new(0.10, 0.0)
            } else {
                offset
            };
            let raw = (observed_center + desired).clamp();
            let output = filter.process(raw, InputMode::Live);
            rows.push(Sample {
                phase,
                ms,
                raw,
                intended: desired,
                output,
            });
            ms += 20;
        }
    }
    rows
}
fn print_report(report: &Report, origin: &str) {
    println!("GUARDADAPTIVE_V88 origin={origin} intent_source=ANNOTATED_UNAUTHENTICATED");
    for (i, m) in report.metrics.iter().enumerate() {
        println!("PHASE={} count={} max_error_pct={:.5} rms_error_pct={:.5} erased_nonzero={} direction_reversals={}",
            PHASES[i],m.count,m.max_error*100.0,m.rms()*100.0,
            m.erased_micro,m.reversed_intent);
    }
    println!(
        "V88_RESEARCH_THRESHOLDS={}",
        if report.constraints_pass {
            "PASS"
        } else {
            "FAIL"
        }
    );
    println!("V88_PHYSICAL_ACCEPTANCE=BLOCKED independent_motion_reference_unverified");
    println!("V88_GAME_OUTPUT=DISABLED V88_PRODUCTION=BLOCKED");
}
fn main() {
    let arg = std::env::args().nth(1);
    if std::env::args().len() != 2
        || !matches!(
            arg.as_deref(),
            Some("--synthetic-demo" | "--score-unverified")
        )
    {
        eprintln!(
            "Usage: replay_v88 --synthetic-demo | --score-unverified < eight-column CSV on stdin"
        );
        std::process::exit(2);
    }
    let result = if arg.as_deref() == Some("--synthetic-demo") {
        Ok((synthetic_case(Vec2::new(0.05, -0.04)), "SYNTHETIC_ORACLE"))
    } else {
        parse(io::stdin().lock()).map(|rows| (rows, "EXTERNALLY_ANNOTATED_UNVERIFIED"))
    };
    match result.and_then(|(rows, origin)| validate_and_score(&rows).map(|report| (report, origin)))
    {
        Ok((report, origin)) => print_report(&report, origin),
        Err(reason) => {
            eprintln!("V88_REJECTED={reason} V88_PRODUCTION=BLOCKED");
            std::process::exit(2)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_static_preserves_micro_but_unverified_abrupt_jump_fails() {
        let result = validate_and_score(&synthetic_case(Vec2::new(0.05, -0.04))).unwrap();
        assert!(result.metrics[0].max_error < NEUTRAL_GOAL);
        for x in &result.metrics[1..4] {
            assert!(x.max_error < INTENDED_ERROR_GOAL);
            assert_eq!(x.erased_micro, 0);
            assert_eq!(x.reversed_intent, 0);
        }
        assert!(result.metrics[4].max_error > ABRUPT_GOAL);
        assert!(!result.constraints_pass);
    }
    #[test]
    fn full_signed_grid_converges_when_static_without_creating_a_fake_physical_pass() {
        for x in -10..=10 {
            for y in -10..=10 {
                let data = synthetic_case(Vec2::new(x as f64 * 0.01, y as f64 * 0.01));
                let scored = validate_and_score(&data).unwrap();
                assert!(scored.metrics[0].max_error < NEUTRAL_GOAL);
                assert!(!scored.constraints_pass);
            }
        }
    }
    #[test]
    fn reject_gaps_order_overflow_nan_false_neutral_and_missing_motion() {
        let source = synthetic_case(Vec2::ZERO);
        let mut data = source.clone();
        data.remove(4);
        assert!(validate_and_score(&data).is_err()); // below phase minimum fails closed
        let mut data = source.clone();
        data[0].phase = 4;
        assert!(validate_and_score(&data).is_err());
        let mut data = source.clone();
        data[0].raw.x = f64::NAN;
        assert!(validate_and_score(&data).is_err());
        let mut data = source.clone();
        data[0].intended.x = 0.01;
        assert!(validate_and_score(&data).is_err());
        let mut data = source.clone();
        data[40].intended = Vec2::ZERO;
        assert!(validate_and_score(&data).is_err());
        let mut data = source.clone();
        data.truncate(130);
        assert!(validate_and_score(&data).is_err());
    }
    #[test]
    fn declared_perfect_output_can_score_but_is_not_independent_physical_proof() {
        let mut dataset = synthetic_case(Vec2::new(0.05, -0.01));
        for row in &mut dataset {
            row.output = row.intended;
        }
        let scored = validate_and_score(&dataset).unwrap();
        assert!(scored.constraints_pass); // externally supplied outputs can be forged
                                          // This CLI still hard-blocks physical acceptance, regardless of this score.
        assert_eq!(scored.metrics[0].erased_micro, 0);
    }
    #[test]
    fn erased_micro_and_direction_reversal_fail_research_gate() {
        let mut erased = synthetic_case(Vec2::new(0.03, 0.01));
        for row in &mut erased {
            if row.phase == 1 {
                row.output = Vec2::ZERO;
            }
        }
        let m = validate_and_score(&erased).unwrap();
        assert!(!m.constraints_pass);
        assert_eq!(m.metrics[1].erased_micro, MIN_PER_PHASE);

        let mut reversed = synthetic_case(Vec2::ZERO);
        for row in &mut reversed {
            if row.phase == 3 {
                row.output = Vec2::new(-row.intended.x, -row.intended.y);
            }
        }
        let m = validate_and_score(&reversed).unwrap();
        assert!(!m.constraints_pass);
        assert_eq!(m.metrics[3].reversed_intent, MIN_PER_PHASE);
    }
    #[test]
    fn externally_annotated_csv_parses_but_is_not_a_physical_certificate() {
        let mut rows = String::new();
        for (phase, label) in PHASES.iter().enumerate() {
            for i in 0..MIN_PER_PHASE {
                let ms = phase * 1000 + i * 20;
                let desired = if (1..=3).contains(&phase) {
                    "0.003,0.001"
                } else {
                    "0,0"
                };
                let line = format!("{label},{ms},0.03,0.01,{desired},0.003,0.001\n");
                rows.push_str(&line);
            }
        }
        let parsed = parse(rows.as_bytes()).unwrap();
        assert_eq!(parsed.len(), 5 * MIN_PER_PHASE);
        let result = validate_and_score(&parsed).unwrap();
        assert!(result.constraints_pass); // scores may PASS with completely unauthenticated annotations
    }
    #[test]
    fn parse_rejects_unlabelled_existing_volunteer_csv() {
        let raw = "0,1,0.1,0.1,0.0,0.0\n";
        assert!(parse(raw.as_bytes()).is_err());
    }
}
