//! GuardAdaptive research: v0.92 two-stream synchronized reference audit.
//! Reads only local files, emits aggregate metrics, never controls a gamepad.
//! IMPORTANT: two CSV files do NOT authenticate independent physical provenance.
use guardadaptive::Vec2;
use std::{
    env,
    fs::File,
    io::{BufRead, BufReader},
};

const PHASES: [&str; 5] = ["neutral", "micro", "hold", "reversal", "abrupt"];
const MAX_SAMPLES: usize = 50_000;
const MIN_PHASE_SAMPLES: usize = 32;
const MIN_PHASE_DURATION_MS: u64 = 500;
const MAX_TRACE_DURATION_MS: u64 = 120_000;
const MAX_GAP_MS: u64 = 50;
const MAX_SYNC_DELTA_MS: u64 = 4;
const MIN_MATCH_RATIO: f64 = 0.95;
const NEUTRAL_LIMIT: f64 = 0.009;
const MOTION_ERROR_LIMIT: f64 = 0.002;

#[derive(Clone, Copy)]
struct Controller {
    t: u64,
    raw: Vec2,
    output: Vec2,
}
#[derive(Clone, Copy)]
struct Reference {
    phase: usize,
    t: u64,
    intent: Vec2,
}
#[derive(Clone, Copy, Default)]
struct PhaseMetrics {
    n: usize,
    first_t: Option<u64>,
    last_t: u64,
    max_raw: f64,
    max_output: f64,
    sq_raw: f64,
    sq_output: f64,
    erased: usize,
    reverse: usize,
    positive_x: bool,
    negative_x: bool,
}
impl PhaseMetrics {
    fn add(&mut self, reference: Reference, signal: Controller) {
        let raw_err = (signal.raw - reference.intent).norm();
        let output_err = (signal.output - reference.intent).norm();
        self.n += 1;
        self.first_t.get_or_insert(reference.t);
        self.last_t = reference.t;
        self.max_raw = self.max_raw.max(raw_err);
        self.max_output = self.max_output.max(output_err);
        self.sq_raw += raw_err * raw_err;
        self.sq_output += output_err * output_err;
        if reference.intent.norm() >= 0.0001 && signal.output.norm() < 0.000001 {
            self.erased += 1;
        }
        if reference.intent.norm() >= 0.0005
            && reference.intent.x * signal.output.x + reference.intent.y * signal.output.y < 0.0
        {
            self.reverse += 1;
        }
        if reference.phase == 3 {
            self.positive_x |= reference.intent.x > 0.005;
            self.negative_x |= reference.intent.x < -0.005;
        }
    }
    fn rms_raw(self) -> f64 {
        (self.sq_raw / self.n.max(1) as f64).sqrt()
    }
    fn rms_output(self) -> f64 {
        (self.sq_output / self.n.max(1) as f64).sqrt()
    }
}
struct Audit {
    phases: [PhaseMetrics; 5],
    matched: usize,
    controller_total: usize,
    reference_total: usize,
    max_clock_delta_ms: u64,
    research_pass: bool,
}
fn parse_float(s: &str) -> Result<f64, String> {
    let v = s.parse::<f64>().map_err(|_| "INVALID_AXIS_VALUE")?;
    if !v.is_finite() || v.abs() > 1.0 {
        return Err("INVALID_AXIS_RANGE".into());
    }
    Ok(v)
}
fn parse_time(s: &str) -> Result<u64, String> {
    let t = s.parse::<u64>().map_err(|_| "INVALID_TIMESTAMP")?;
    if t > MAX_TRACE_DURATION_MS {
        return Err("EXCESSIVE_DURATION".into());
    }
    Ok(t)
}
fn check_clock(previous: Option<u64>, t: u64) -> Result<(), String> {
    if let Some(last) = previous {
        if t <= last {
            return Err("NON_MONOTONIC_CLOCK".into());
        }
        if t - last > MAX_GAP_MS {
            return Err("SAMPLE_GAP_TOO_LARGE".into());
        }
    }
    Ok(())
}
fn parse_controller<R: BufRead>(source: R) -> Result<Vec<Controller>, String> {
    let mut out = Vec::new();
    let mut lines = source.lines();
    if lines
        .next()
        .transpose()
        .map_err(|_| "INPUT_READ_FAILURE")?
        .as_deref()
        != Some("t_ms,raw_x,raw_y,output_x,output_y")
    {
        return Err("CONTROLLER_HEADER_MISMATCH".into());
    }
    for line in lines {
        let line = line.map_err(|_| "INPUT_READ_FAILURE")?;
        let fields: Vec<_> = line.trim().split(',').collect();
        if fields.len() != 5 {
            return Err("CONTROLLER_COLUMNS_INVALID".into());
        }
        let t = parse_time(fields[0])?;
        check_clock(out.last().map(|x: &Controller| x.t), t)?;
        let raw = Vec2::new(parse_float(fields[1])?, parse_float(fields[2])?);
        let output = Vec2::new(parse_float(fields[3])?, parse_float(fields[4])?);
        out.push(Controller { t, raw, output });
        if out.len() > MAX_SAMPLES {
            return Err("TOO_MANY_SAMPLES".into());
        }
    }
    if out.is_empty() {
        return Err("CONTROLLER_EMPTY".into());
    }
    Ok(out)
}
fn parse_reference<R: BufRead>(source: R) -> Result<Vec<Reference>, String> {
    let mut out = Vec::new();
    let mut lines = source.lines();
    if lines
        .next()
        .transpose()
        .map_err(|_| "INPUT_READ_FAILURE")?
        .as_deref()
        != Some("t_ms,phase,intent_x,intent_y")
    {
        return Err("REFERENCE_HEADER_MISMATCH".into());
    }
    let mut last_phase = 0usize;
    for line in lines {
        let line = line.map_err(|_| "INPUT_READ_FAILURE")?;
        let fields: Vec<_> = line.trim().split(',').collect();
        if fields.len() != 4 {
            return Err("REFERENCE_COLUMNS_INVALID".into());
        }
        let t = parse_time(fields[0])?;
        check_clock(out.last().map(|x: &Reference| x.t), t)?;
        let phase = PHASES
            .iter()
            .position(|x| *x == fields[1])
            .ok_or("UNKNOWN_PHASE")?;
        if phase < last_phase || phase > last_phase + 1 {
            return Err("PHASE_ORDER_INVALID".into());
        }
        let intent = Vec2::new(parse_float(fields[2])?, parse_float(fields[3])?);
        match phase {
            0 | 4 if intent.norm() > 0.0005 => return Err("NEUTRAL_REFERENCE_NONZERO".into()),
            1 if !(0.0001..=0.015).contains(&intent.norm()) => {
                return Err("MICRO_REFERENCE_RANGE".into())
            }
            2 if intent.norm() < 0.01 => return Err("HOLD_REFERENCE_RANGE".into()),
            3 if intent.x.abs() < 0.005 => return Err("REVERSAL_REFERENCE_RANGE".into()),
            _ => (),
        }
        out.push(Reference { t, phase, intent });
        last_phase = phase;
        if out.len() > MAX_SAMPLES {
            return Err("TOO_MANY_SAMPLES".into());
        }
    }
    if out.is_empty() {
        return Err("REFERENCE_EMPTY".into());
    }
    Ok(out)
}
fn score(controller: &[Controller], reference: &[Reference]) -> Result<Audit, String> {
    if controller.is_empty() || reference.is_empty() {
        return Err("EMPTY_TRACE".into());
    }
    // Defense in depth: score() also rejects invalid clocks if called directly.
    for window in controller.windows(2) {
        check_clock(Some(window[0].t), window[1].t)?;
    }
    for window in reference.windows(2) {
        check_clock(Some(window[0].t), window[1].t)?;
    }
    let mut phases = [PhaseMetrics::default(); 5];
    let mut next_controller = 0usize;
    let mut matched = 0usize;
    let mut worst_delta = 0u64;
    for item in reference {
        while next_controller < controller.len()
            && controller[next_controller]
                .t
                .saturating_add(MAX_SYNC_DELTA_MS)
                < item.t
        {
            next_controller += 1;
        }
        if next_controller >= controller.len() {
            break;
        }
        let mut select = next_controller;
        if select + 1 < controller.len()
            && controller[select + 1].t.abs_diff(item.t) < controller[select].t.abs_diff(item.t)
        {
            select += 1;
        }
        let delta = controller[select].t.abs_diff(item.t);
        if delta > MAX_SYNC_DELTA_MS {
            continue;
        }
        phases[item.phase].add(*item, controller[select]);
        next_controller = select + 1; // same controller sample cannot be used twice
        worst_delta = worst_delta.max(delta);
        matched += 1;
    }
    let fraction_ref = matched as f64 / reference.len() as f64;
    let fraction_ctrl = matched as f64 / controller.len() as f64;
    if fraction_ref < MIN_MATCH_RATIO || fraction_ctrl < MIN_MATCH_RATIO {
        return Err("CLOCK_ALIGNMENT_OR_MISSING_SAMPLES".into());
    }
    for phase in &phases {
        if phase.n < MIN_PHASE_SAMPLES {
            return Err("INSUFFICIENT_PHASE_SAMPLES".into());
        }
        if phase.last_t - phase.first_t.unwrap_or(phase.last_t) < MIN_PHASE_DURATION_MS {
            return Err("PHASE_DURATION_TOO_SHORT".into());
        }
    }
    if !(phases[3].positive_x && phases[3].negative_x) {
        return Err("MISSING_REVERSAL_POLARITY".into());
    }
    let research_pass = phases.iter().enumerate().all(|(i, m)| {
        if i == 0 || i == 4 {
            m.max_output < NEUTRAL_LIMIT
        } else {
            m.max_output < MOTION_ERROR_LIMIT && m.erased == 0 && m.reverse == 0
        }
    });
    Ok(Audit {
        phases,
        matched,
        controller_total: controller.len(),
        reference_total: reference.len(),
        max_clock_delta_ms: worst_delta,
        research_pass,
    })
}
fn print_report(a: &Audit) {
    println!(
        "V92_RECORDS controller={} reference={} matched={} max_alignment_delta_ms={}",
        a.controller_total, a.reference_total, a.matched, a.max_clock_delta_ms
    );
    for (i, m) in a.phases.iter().enumerate() {
        println!("PHASE={} n={} raw_max_pct={:.5} output_max_pct={:.5} raw_rms_pct={:.5} output_rms_pct={:.5} erased_micro={} reversed_motion={}",
            PHASES[i],m.n,m.max_raw*100.0,m.max_output*100.0,
            m.rms_raw()*100.0,m.rms_output()*100.0,m.erased,m.reverse);
    }
    println!(
        "V92_NUMERIC_RESEARCH_TARGET={} (EXTERNAL_PROVENANCE_UNVERIFIED)",
        if a.research_pass { "PASS" } else { "FAIL" }
    );
    println!("V92_PHYSICAL_ACCEPTANCE=BLOCKED V92_PRODUCTION=BLOCKED V92_GAME_OUTPUT=DISABLED");
}
fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 || args[1] != "--controller" || args[3] != "--reference" {
        return Err("USAGE: reference_gate_v92 --controller path.csv --reference path.csv".into());
    }
    let ctrl_path = std::fs::canonicalize(&args[2]).map_err(|_| "CONTROLLER_FILE_UNREADABLE")?;
    let ref_path = std::fs::canonicalize(&args[4]).map_err(|_| "REFERENCE_FILE_UNREADABLE")?;
    if ctrl_path == ref_path {
        return Err("SAME_SOURCE_PATH_NOT_ALLOWED".into());
    }
    let controller = File::open(ctrl_path).map_err(|_| "CONTROLLER_FILE_UNREADABLE")?;
    let reference = File::open(ref_path).map_err(|_| "REFERENCE_FILE_UNREADABLE")?;
    if controller
        .metadata()
        .map_err(|_| "CONTROLLER_FILE_UNREADABLE")?
        .len()
        > 8_000_000
        || reference
            .metadata()
            .map_err(|_| "REFERENCE_FILE_UNREADABLE")?
            .len()
            > 8_000_000
    {
        return Err("FILE_SIZE_LIMIT".into());
    }
    let a = parse_controller(BufReader::new(controller))?;
    let b = parse_reference(BufReader::new(reference))?;
    let report = score(&a, &b)?;
    print_report(&report);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("V92_DATA_REJECTED={error} V92_PHYSICAL_ACCEPTANCE=BLOCKED");
        std::process::exit(2);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn good_data(with_abrupt_error: bool) -> (Vec<Controller>, Vec<Reference>) {
        let mut c = Vec::new();
        let mut r = Vec::new();
        for phase in 0..5 {
            for step in 0..64 {
                let t = (phase * 64 + step) as u64 * 16;
                let intended = match phase {
                    0 | 4 => Vec2::ZERO,
                    1 => Vec2::new(0.003, -0.002),
                    2 => Vec2::new(0.04, -0.02),
                    3 => Vec2::new(if step % 16 < 8 { 0.04 } else { -0.04 }, 0.01),
                    _ => unreachable!(),
                };
                let bias = if phase == 4 {
                    Vec2::new(0.15, -0.04)
                } else {
                    Vec2::new(0.05, -0.04)
                };
                let raw = intended + bias;
                let output = if phase == 4 && with_abrupt_error {
                    Vec2::new(0.10, 0.0)
                } else {
                    intended
                };
                c.push(Controller { t, raw, output });
                r.push(Reference {
                    phase,
                    t,
                    intent: intended,
                });
            }
        }
        (c, r)
    }
    #[test]
    fn five_phases_good_reference_can_score_without_physical_proof() {
        let (c, r) = good_data(false);
        let scored = score(&c, &r).unwrap();
        assert!(scored.research_pass); // could be forged, NOT a physical certificate
        assert_eq!(scored.matched, 320);
        assert!(scored.phases[1].max_output < 0.002);
    }
    #[test]
    fn sudden_unknown_drift_fails_accuracy_without_rewriting_signal() {
        let (c, r) = good_data(true);
        let a = score(&c, &r).unwrap();
        assert!(!a.research_pass);
        assert!(a.phases[4].max_output > 0.009);
        assert!(a.phases[4].max_raw > 0.09);
    }
    #[test]
    fn reject_timestamp_replay_missing_samples_and_large_gaps() {
        let (mut c, r) = good_data(false);
        c[50].t = c[49].t;
        assert!(score(&c, &r).is_err()); // defense-in-depth
        let mut parsed = String::from("t_ms,raw_x,raw_y,output_x,output_y\n");
        for row in &c {
            parsed.push_str(&format!(
                "{},{},{},{},{}\n",
                row.t, row.raw.x, row.raw.y, row.output.x, row.output.y
            ));
        }
        assert!(parse_controller(parsed.as_bytes()).is_err());
        let (mut c, r) = good_data(false);
        c.drain(30..80);
        assert!(score(&c, &r).is_err());
    }
    #[test]
    fn rejects_missing_or_short_phases_and_one_direction_reversals() {
        let (c, mut r) = good_data(false);
        r.retain(|row| row.phase != 2);
        assert!(score(&c, &r).is_err());
        let (c, mut r) = good_data(false);
        for x in &mut r {
            if x.phase == 3 {
                x.intent.x = 0.04;
            }
        }
        assert!(score(&c, &r).is_err());
        let (c, mut r) = good_data(false);
        r.retain(|x| x.phase != 1 || x.t < 64 * 16 + 100);
        assert!(score(&c, &r).is_err());
    }
    #[test]
    fn strict_headers_and_untrusted_intent_are_not_auto_authenticated() {
        assert!(parse_controller("time,x,y,a,b\n0,0,0,0,0\n".as_bytes()).is_err());
        assert!(parse_reference("time,phase,x,y\n0,neutral,0,0\n".as_bytes()).is_err());
        assert!(
            parse_reference("t_ms,phase,intent_x,intent_y\n0,neutral,NaN,0\n".as_bytes()).is_err()
        );
        assert!(
            parse_reference("t_ms,phase,intent_x,intent_y\n0,neutral,0.09,0\n".as_bytes()).is_err()
        );
    }
    #[test]
    fn erased_micro_input_must_fail_even_when_other_phases_are_perfect() {
        let (mut ctl, reference) = good_data(false);
        for (i, row) in ctl.iter_mut().enumerate() {
            if reference[i].phase == 1 {
                row.output = Vec2::ZERO;
            }
        }
        let report = score(&ctl, &reference).unwrap();
        assert!(!report.research_pass);
        assert_eq!(report.phases[1].erased, 64);
        assert!(report.phases[1].max_output > 0.002);
    }
    #[test]
    fn direction_reversal_and_inverted_output_must_fail() {
        let (mut ctl, reference) = good_data(false);
        for (i, row) in ctl.iter_mut().enumerate() {
            if reference[i].phase == 3 {
                row.output = Vec2::new(-reference[i].intent.x, -reference[i].intent.y);
            }
        }
        let report = score(&ctl, &reference).unwrap();
        assert!(!report.research_pass);
        assert_eq!(report.phases[3].reverse, 64);
    }
    #[test]
    fn reject_actual_long_gaps_in_both_independent_streams() {
        let (mut ctl, mut reference) = good_data(false);
        ctl[20].t = ctl[19].t + 60;
        assert!(score(&ctl, &reference).is_err());
        ctl = good_data(false).0;
        reference[20].t = reference[19].t + 60;
        assert!(score(&ctl, &reference).is_err());
    }
    #[test]
    fn tolerates_only_bounded_clock_jitter_with_one_to_one_alignment() {
        let (ctl, mut reference) = good_data(false);
        for row in &mut reference {
            row.t += if row.t % 32 == 0 { 4 } else { 0 };
        }
        let a = score(&ctl, &reference).unwrap();
        assert!(a.research_pass);
        assert_eq!(a.matched, reference.len());
        assert_eq!(a.max_clock_delta_ms, 4);
        let mut bad = reference.clone();
        for row in &mut bad {
            row.t += 6;
        }
        assert!(score(&ctl, &bad).is_err());
    }
    #[test]
    fn strict_sync_window_catches_clock_offset() {
        let (c, mut r) = good_data(false);
        for item in &mut r {
            item.t += 9;
        }
        assert!(score(&c, &r).is_err());
    }
}
