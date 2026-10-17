//! v0.86: read-only replay of *unverified* controller samples from stdin.
//! Not a gamepad backend, device test, or physically verified drift score.
use guardadaptive::{fixed_deadzone, Config, GuardAdaptive, InputMode, OutputPolicy, Vec2};
use std::io::{self, BufRead};

const FIXED_DZ: f64 = 0.08;
#[derive(Clone, Copy)]
struct Sample {
    left: Vec2,
    right: Vec2,
}
#[derive(Default)]
struct Stats {
    raw: f64,
    fixed: f64,
    shadow: f64,
    total: usize,
    erasures: usize,
}
impl Stats {
    fn push(&mut self, raw: Vec2, corrected: Vec2, shadow: Vec2) {
        self.raw += raw.length_squared();
        self.fixed += corrected.length_squared();
        self.shadow += shadow.length_squared();
        self.total += 1;
        if raw.norm() > 0.00003 && corrected == Vec2::ZERO {
            self.erasures += 1;
        }
    }
    fn rms(&self, which: f64) -> f64 {
        (which / self.total as f64).sqrt() * 100.0
    }
}
fn review_idle(data: &[Sample], right: bool) -> bool {
    if data.len() < 35 {
        return false;
    }
    let points: Vec<Vec2> = data
        .iter()
        .map(|r| if right { r.right } else { r.left })
        .collect();
    let mut xs: Vec<_> = points.iter().map(|p| p.x).collect();
    let mut ys: Vec<_> = points.iter().map(|p| p.y).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let mid = xs.len() / 2;
    let median = |a: &[f64]| {
        if a.len().is_multiple_of(2) {
            (a[mid - 1] + a[mid]) * 0.5
        } else {
            a[mid]
        }
    };
    let center = Vec2::new(median(&xs), median(&ys));
    let mut sum = 0.0_f64;
    for p in &points {
        let delta = (*p - center).norm();
        if delta > 0.012 {
            return false;
        }
        sum += delta * delta;
    }
    (sum / points.len() as f64).sqrt() <= 0.003
}
fn analyze(phases: &[Vec<Sample>; 6]) -> String {
    let mut left = GuardAdaptive::with_policy(
        Config {
            auto_alpha: 0.0,
            ..Config::default()
        },
        OutputPolicy::PreserveMicro,
    );
    let mut right = GuardAdaptive::with_policy(
        Config {
            auto_alpha: 0.0,
            ..Config::default()
        },
        OutputPolicy::PreserveMicro,
    );
    let mut report = String::new();
    report.push_str("GA_OFFLINE_V86 calibration=BLOCKED physical_neutral=UNVERIFIED input_to_game=NONE learning_from_volunteers=NONE\n");
    report.push_str(&format!(
        "NEUTRAL_L={} NEUTRAL_R={} (stability_observation_only)\n",
        if review_idle(&phases[0], false) {
            "STABLE"
        } else {
            "UNSTABLE"
        },
        if review_idle(&phases[3], true) {
            "STABLE"
        } else {
            "UNSTABLE"
        }
    ));
    for (phase, rows) in phases.iter().enumerate() {
        let mut l = Stats::default();
        let mut r = Stats::default();
        for item in rows {
            let x = left.process(item.left, InputMode::Live);
            let y = right.process(item.right, InputMode::Live);
            l.push(item.left, fixed_deadzone(item.left, FIXED_DZ), x);
            r.push(item.right, fixed_deadzone(item.right, FIXED_DZ), y);
        }
        let chosen = if phase < 3 { &l } else { &r };
        report.push_str(&format!(
            "PHASE={} rows={} target_stick={} raw_rms_pct={:.3} fixed8_rms_pct={:.3} rust_untrained_rms_pct={:.3} fixed8_erased_nonzero={}/{} \n",
            phase,rows.len(),if phase<3{"LEFT"}else{"RIGHT"},
            chosen.rms(chosen.raw),chosen.rms(chosen.fixed),chosen.rms(chosen.shadow),
            chosen.erasures,chosen.total));
    }
    report.push_str(&format!(
        "RUST_SHADOW_CENTER_UPDATES left={} right={} (expect_zero)\n",
        left.telemetry().auto_updates,
        right.telemetry().auto_updates
    ));
    report
}
fn parse(lines: impl BufRead) -> Result<[Vec<Sample>; 6], String> {
    let mut by_phase: [Vec<Sample>; 6] = std::array::from_fn(|_| Vec::new());
    let mut last_time: Option<u64> = None;
    let mut last_phase = 0_usize;
    let mut count = 0_usize;
    for line in lines.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.trim().split(',').collect();
        if cols.len() != 6 {
            return Err("Expected phase,time,lx,ly,rx,ry".to_owned());
        }
        let phase: usize = cols[0].parse().map_err(|_| "Invalid phase")?;
        let when: u64 = cols[1].parse().map_err(|_| "Invalid time")?;
        if phase > 5 || phase < last_phase || phase > last_phase + 1 {
            return Err("Bad phase order".into());
        }
        if last_time.is_some_and(|t| when <= t) {
            return Err("Time not increasing".into());
        }
        if when > 180000 {
            return Err("Duration limit exceeded".into());
        }
        let mut x = [0.0_f64; 4];
        for (i, v) in x.iter_mut().enumerate() {
            *v = cols[i + 2].parse().map_err(|_| "Invalid axis")?;
            if !v.is_finite() || v.abs() > 1.0 {
                return Err("Axis outside [-1,1]".into());
            }
        }
        by_phase[phase].push(Sample {
            left: Vec2::new(x[0], x[1]),
            right: Vec2::new(x[2], x[3]),
        });
        last_phase = phase;
        last_time = Some(when);
        count += 1;
        if count > 9000 {
            return Err("Too many samples".into());
        }
    }
    if by_phase.iter().any(|v| v.len() < 80) {
        return Err("Insufficient six-phase sample data".into());
    }
    Ok(by_phase)
}
fn main() {
    match parse(io::stdin().lock()) {
        Ok(samples) => print!("{}", analyze(&samples)),
        Err(e) => {
            eprintln!("OFFLINE_REPLAY_REJECTED: {e}");
            std::process::exit(2);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_phases_and_nonfinite_axes() {
        assert!(parse("0,1,0,0,0,NaN\n".as_bytes()).is_err());
        assert!(parse("0,1,0,0,0,0\n2,2,0,0,0,0\n".as_bytes()).is_err());
        assert!(parse("0,1,0,0,0,0\n0,1,0,0,0,0\n".as_bytes()).is_err());
    }
    #[test]
    fn unverified_replay_has_zero_learning_and_fixed_deadzone_erases_micro() {
        let record = Sample {
            left: Vec2::new(0.025, 0.0),
            right: Vec2::new(0.0372, 0.0),
        };
        let rows: [Vec<Sample>; 6] = std::array::from_fn(|_| vec![record; 100]);
        let out = analyze(&rows);
        assert!(out.contains("calibration=BLOCKED"));
        assert!(out.contains("fixed8_erased_nonzero=100/100"));
        assert!(out.contains("RUST_SHADOW_CENTER_UPDATES left=0 right=0"));
    }
    #[test]
    fn clean_static_neutral_can_look_stable_but_intent_unknown() {
        let sample = Sample {
            left: Vec2::new(0.1, 0.),
            right: Vec2::ZERO,
        };
        assert!(review_idle(&vec![sample; 100], false));
    }
    #[test]
    fn neutral_spikes_do_not_pass_quality_check() {
        let s = Sample {
            left: Vec2::new(0.1, 0.),
            right: Vec2::ZERO,
        };
        let mut window = vec![s; 100];
        window[45].left = Vec2::new(0.9, 0.4);
        assert!(!review_idle(&window, false));
    }
}
