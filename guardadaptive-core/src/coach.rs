//! User-guided calibration protocol. A test oracle is NEVER used in the live hot path.
//! Coach is a scripted onboarding / future capture importer, not a virtual controller.
use crate::Vec2;
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub id: u8,
    pub label: &'static str,
    pub instruction: &'static str,
    pub target: Vec2,
    pub neutral: bool,
    pub length_ms: u32,
}
const fn step(
    id: u8,
    label: &'static str,
    instruction: &'static str,
    x: f64,
    y: f64,
    neutral: bool,
    duration: u32,
) -> Step {
    Step {
        id,
        label,
        instruction,
        target: Vec2::new(x, y),
        neutral,
        length_ms: duration,
    }
}
/// Directions are from the operator perspective, not a game-specific camera transform.
/// Grounded movement targets are instructional, not high-accuracy physical ground truth.
pub const STEPS: [Step; 21] = [
    step(
        0,
        "neutral_start",
        "CONTROLCUYE DOKUNMA. Her iki analog tamamen serbest; 3 saniye bekle.",
        0.,
        0.,
        true,
        3000,
    ),
    step(
        1,
        "right_full",
        "SOL analogu tamamen SAGA gotur, 2 saniye sabit tut.",
        1.,
        0.,
        false,
        2000,
    ),
    step(
        2,
        "release_right",
        "Analogu ANIDEN BIRAK. Elle merkezleme yapma; 3 saniye bekle.",
        0.,
        0.,
        true,
        3000,
    ),
    step(
        3,
        "left_full",
        "SOL analogu tamamen SOLA gotur, 2 saniye sabit tut.",
        -1.,
        0.,
        false,
        2000,
    ),
    step(
        4,
        "release_left",
        "Analogu birak, 3 saniye hic dokunma.",
        0.,
        0.,
        true,
        3000,
    ),
    step(
        5,
        "up_full",
        "SOL analogu tamamen YUKARI gotur, 2 saniye sabit tut.",
        0.,
        1.,
        false,
        2000,
    ),
    step(
        6,
        "release_up",
        "Analogu birak, 3 saniye hic dokunma.",
        0.,
        0.,
        true,
        3000,
    ),
    step(
        7,
        "down_full",
        "SOL analogu tamamen ASAGI gotur, 2 saniye sabit tut.",
        0.,
        -1.,
        false,
        2000,
    ),
    step(
        8,
        "release_down",
        "Analogu birak, 3 saniye hic dokunma.",
        0.,
        0.,
        true,
        3000,
    ),
    step(
        9,
        "diagonal_ne",
        "SAG-YUKARI caprazinda yaklasik %70 hareket et, 2 saniye tut.",
        0.7,
        0.7,
        false,
        2000,
    ),
    step(
        10,
        "diagonal_sw",
        "SOL-ASAGI caprazinda yaklasik %70 hareket et, 2 saniye tut.",
        -0.7,
        -0.7,
        false,
        2000,
    ),
    step(
        11,
        "neutral_mid",
        "Analogu birak ve 4 saniye bekle.",
        0.,
        0.,
        true,
        4000,
    ),
    step(
        12,
        "micro_right",
        "SAGA cok kucuk, yaklasik %1 hareket et ve sabit tut.",
        0.01,
        0.,
        false,
        3000,
    ),
    step(
        13,
        "micro_left",
        "SOLA cok kucuk, yaklasik %1 hareket et ve sabit tut.",
        -0.01,
        0.,
        false,
        3000,
    ),
    step(
        14,
        "micro_up",
        "YUKARI cok kucuk, yaklasik %1 hareket et ve sabit tut.",
        0.,
        0.01,
        false,
        3000,
    ),
    step(
        15,
        "micro_down",
        "ASAGI cok kucuk, yaklasik %1 hareket et ve sabit tut.",
        0.,
        -0.01,
        false,
        3000,
    ),
    step(
        16,
        "fine_right",
        "SAGA cok yavas bir sekilde %0'dan %100'e kadar kaydir.",
        0.5,
        0.,
        false,
        5000,
    ),
    step(
        17,
        "fine_left",
        "SOLA cok yavas bir sekilde %0'dan %100'e kadar kaydir.",
        -0.5,
        0.,
        false,
        5000,
    ),
    step(
        18,
        "neutral_late",
        "Analogu serbest birak ve 4 saniye bekle.",
        0.,
        0.,
        true,
        4000,
    ),
    step(
        19,
        "rapid_taps",
        "Arka arkaya saga-sola kisa, hizli ve keskin hareketler yap.",
        0.,
        0.,
        false,
        4000,
    ),
    step(
        20,
        "neutral_finish",
        "Son 5 saniye hic dokunma, bagimsiz notr dogrulamasi.",
        0.,
        0.,
        true,
        5000,
    ),
];
pub fn plan(out: &Path) -> io::Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(out)?;
    writeln!(
        f,
        "step_id,label,length_ms,target_x,target_y,trusted_neutral,instruction"
    )?;
    println!("GUARDADAPTIVE COACH | 21 guided stages | left stick | Turkish");
    println!("IMPORTANT: This is a protocol. Real capture is NOT connected yet.");
    for s in STEPS {
        println!("#{:02} ({:>4}ms): {}", s.id, s.length_ms, s.instruction);
        writeln!(
            f,
            "{},{},{},{:.6},{:.6},{},{}",
            s.id, s.label, s.length_ms, s.target.x, s.target.y, s.neutral, s.instruction
        )?;
    }
    println!("Protocol CSV: {}", out.display());
    Ok(())
}
/// Import a recorded external stream with CSV records: step_id,x,y.
/// The operator marks step transitions; neutral samples require actual released stick.
/// Never treat directional commands as precise intent labels.
/// Controller-free synthetic capture; used only to regression-test operator
/// instructions and assess parser. This is NOT real human/controller evidence.
pub fn demo(out: &Path) -> io::Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut csv = io::BufWriter::new(fs::File::create(out)?);
    writeln!(csv, "step_id,x,y")?;
    for s in STEPS {
        for t in 0..125_u64 {
            let u = t as f64 / 124.0;
            let desired = if s.id == 16 || s.id == 17 {
                s.target * (2.0 * u)
            } else if s.id == 19 {
                Vec2::new(0.5 * (u * 40.0).sin(), 0.)
            } else {
                s.target
            };
            // Synthetic sensor bias plus bounded deterministic perturbation.
            let offset = Vec2::new(0.020 + 0.005 * (s.id as f64 / 20.0), -0.012);
            let noise = (t as f64 * 1.17 + s.id as f64 * 3.0).sin() * 0.0003;
            let raw = (desired + offset + Vec2::new(noise, -noise)).clamp();
            writeln!(csv, "{},{:.8},{:.8}", s.id, raw.x, raw.y)?;
        }
    }
    csv.flush()?;
    println!(
        "SYNTHETIC COACH CAPTURE: {} stages * 125 samples; {}",
        STEPS.len(),
        out.display()
    );
    Ok(())
}
pub fn assess(input: &Path) -> io::Result<()> {
    let file = fs::File::open(input)?;
    let reader = io::BufReader::new(file);
    let mut count = [0_u64; 21];
    let mut sum = [Vec2::ZERO; 21];
    let mut min = [Vec2::new(1., 1.); 21];
    let mut max = [Vec2::new(-1., -1.); 21];
    let mut dropped = 0_u64;
    for (line_number, line) in reader.lines().enumerate() {
        let line = line?;
        if line_number == 0 && line.starts_with("step_id") {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split(',').collect();
        if cells.len() < 3 {
            dropped += 1;
            continue;
        }
        let Ok(id) = cells[0].parse::<usize>() else {
            dropped += 1;
            continue;
        };
        let Ok(x) = cells[1].parse::<f64>() else {
            dropped += 1;
            continue;
        };
        let Ok(y) = cells[2].parse::<f64>() else {
            dropped += 1;
            continue;
        };
        if id >= STEPS.len() || !x.is_finite() || !y.is_finite() || x.abs() > 1.0 || y.abs() > 1.0 {
            dropped += 1;
            continue;
        }
        let v = Vec2::new(x, y);
        count[id] += 1;
        sum[id] = sum[id] + v;
        min[id].x = min[id].x.min(x);
        min[id].y = min[id].y.min(y);
        max[id].x = max[id].x.max(x);
        max[id].y = max[id].y.max(y);
    }
    println!("GUIDED CAPTURE ASSESSMENT: missing samples are NOT treated as completed stages.");
    let mut min_neutral_x = f64::INFINITY;
    let mut max_neutral_x = f64::NEG_INFINITY;
    let mut min_neutral_y = f64::INFINITY;
    let mut max_neutral_y = f64::NEG_INFINITY;
    let mut neutral_ok = 0;
    for s in STEPS {
        let n = count[s.id as usize];
        if n == 0 {
            println!("#{:02} {:<20} MISSING | {}", s.id, s.label, s.instruction);
            continue;
        }
        let avg = sum[s.id as usize] * (1.0 / n as f64);
        if s.neutral {
            min_neutral_x = min_neutral_x.min(avg.x);
            max_neutral_x = max_neutral_x.max(avg.x);
            min_neutral_y = min_neutral_y.min(avg.y);
            max_neutral_y = max_neutral_y.max(avg.y);
            neutral_ok += 1;
        }
        println!(
            "#{:02} {:<20} samples={:<6} avg=({:+.5},{:+.5}) rangeX=[{:+.4},{:+.4}]",
            s.id, s.label, n, avg.x, avg.y, min[s.id as usize].x, max[s.id as usize].x
        );
        if !s.neutral && s.target.norm() >= 0.5 {
            let projected = avg.x * s.target.x + avg.y * s.target.y;
            if projected < 0.0 {
                println!("  ALERT: measured movement opposes requested direction. Check orientation/input mapping.");
            }
        }
    }
    println!("Ignored malformed/out-of-range rows={dropped}");
    if neutral_ok >= 3 {
        println!(
            "NEUTRAL CENTER DRIFT ACROSS {} STAGES: dx={:.6}, dy={:.6}",
            neutral_ok,
            max_neutral_x - min_neutral_x,
            max_neutral_y - min_neutral_y
        );
    } else {
        println!("NEED >=3 observed neutral steps before any neutral drift claim.");
    }
    println!("No measured instruction equals guaranteed user intention; micro-movement fidelity requires independent checks.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn step_ids_unique_and_trusted_only_release() {
        assert_eq!(STEPS.len(), 21);
        for (id, s) in STEPS.iter().enumerate() {
            assert_eq!(id, s.id as usize);
            if s.neutral {
                assert_eq!(s.target, Vec2::ZERO);
            }
        }
    }
}
