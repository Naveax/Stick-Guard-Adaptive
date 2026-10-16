//! Learned controller response mapping from EXPLICIT guided movement captures.
//! Human instructions do not supply exact intent. Model quality needs real recordings.
use crate::{coach::STEPS, Vec2};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};
#[derive(Clone, Copy, Debug)]
pub struct MotionProfile {
    pub center: Vec2,
    pub right: f64,
    pub left: f64,
    pub up: f64,
    pub down: f64,
    pub neutral_span: Vec2,
    pub micro_right_response: f64,
    pub micro_left_response: f64,
}
impl MotionProfile {
    /// Precision mode: do not suppress micro-motion. This necessarily preserves
    /// some residual sensor jitter; it is NOT the "zero output when untouched" mode.
    pub fn precision(&self, raw: Vec2) -> Vec2 {
        let delta = raw.clamp() - self.center;
        Vec2::new(
            delta.x / if delta.x >= 0. { self.right } else { self.left },
            delta.y / if delta.y >= 0. { self.up } else { self.down },
        )
        .clamp()
    }
    /// Balanced mode: measured neutral variability guards against small jitter,
    /// but can suppress genuine movement whose amplitude overlaps the noise floor.
    pub fn balanced(&self, raw: Vec2) -> Vec2 {
        let v = self.precision(raw);
        let dz = ((self.neutral_span.x.max(self.neutral_span.y) * 2.0)
            / self.right.min(self.left).min(self.up).min(self.down))
        .clamp(0.00005, 0.12);
        let n = v.norm();
        if n <= dz {
            Vec2::ZERO
        } else {
            (v * (((n - dz) / (1.0 - dz)) / n)).saturate()
        }
    }
}
#[derive(Default, Clone, Copy)]
struct Stat {
    sum: Vec2,
    lo: Vec2,
    hi: Vec2,
    n: u64,
}
impl Stat {
    fn add(&mut self, v: Vec2) {
        if self.n == 0 {
            self.lo = v;
            self.hi = v;
        }
        self.n += 1;
        self.sum = self.sum + v;
        self.lo.x = self.lo.x.min(v.x);
        self.lo.y = self.lo.y.min(v.y);
        self.hi.x = self.hi.x.max(v.x);
        self.hi.y = self.hi.y.max(v.y);
    }
    fn mean(self) -> Vec2 {
        self.sum * (1.0 / self.n.max(1) as f64)
    }
}
fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    if v.len().is_multiple_of(2) {
        (v[v.len() / 2 - 1] + v[v.len() / 2]) * 0.5
    } else {
        v[v.len() / 2]
    }
}
fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}
/// Learning is triggered only by deliberately marked stages, never by live
/// untrusted raw input. Source format: step_id,x,y as exported by capture client.
pub fn learn(input: &Path) -> io::Result<MotionProfile> {
    let f = fs::File::open(input)?;
    let mut stats = [Stat::default(); 21];
    let mut invalid_rows = 0_u64;
    for (line_no, line) in io::BufReader::new(f).lines().enumerate() {
        let s = line?;
        if line_no == 0 && s.starts_with("step_id") {
            continue;
        }
        if s.trim().is_empty() {
            continue;
        }
        let columns: Vec<&str> = s.split(',').collect();
        if columns.len() != 3 {
            invalid_rows += 1;
            continue;
        }
        let (Ok(id), Ok(x), Ok(y)) = (
            columns[0].parse::<usize>(),
            columns[1].parse::<f64>(),
            columns[2].parse::<f64>(),
        ) else {
            invalid_rows += 1;
            continue;
        };
        if id >= STEPS.len() || !x.is_finite() || !y.is_finite() || x.abs() > 1.0 || y.abs() > 1.0 {
            invalid_rows += 1;
            continue;
        }
        stats[id].add(Vec2::new(x, y));
    }
    let neutral: Vec<_> = STEPS
        .iter()
        .filter(|s| s.neutral)
        .map(|s| stats[s.id as usize])
        .filter(|v| v.n >= 16)
        .collect();
    if neutral.len() < 3 {
        return Err(invalid(
            "need >=3 completed neutral steps with >=16 samples",
        ));
    }
    for id in [1, 3, 5, 7] {
        if stats[id].n < 16 {
            return Err(invalid(
                "need >=16 samples for each +/- X and Y full-throw step",
            ));
        }
    }
    let mut xs: Vec<f64> = neutral.iter().map(|s| s.mean().x).collect();
    let mut ys: Vec<f64> = neutral.iter().map(|s| s.mean().y).collect();
    let center = Vec2::new(median(&mut xs), median(&mut ys));
    let neutral_span = Vec2::new(
        neutral
            .iter()
            .map(|s| (s.hi.x - s.lo.x).abs())
            .fold(0.0_f64, f64::max),
        neutral
            .iter()
            .map(|s| (s.hi.y - s.lo.y).abs())
            .fold(0.0_f64, f64::max),
    );
    let drift_span = Vec2::new(
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - ys.iter().copied().fold(f64::INFINITY, f64::min),
    );
    if drift_span.x > 0.08 || drift_span.y > 0.08 {
        return Err(invalid(
            "neutral center unstable across phases >8%; recalibration needed",
        ));
    }
    let right = stats[1].hi.x - center.x;
    let left = center.x - stats[3].lo.x;
    let up = stats[5].hi.y - center.y;
    let down = center.y - stats[7].lo.y;
    if [right, left, up, down]
        .iter()
        .any(|v| *v < 0.2 || *v > 2.0 || !v.is_finite())
    {
        return Err(invalid(
            "incomplete travel / inverted axis; repeat directional full-throw guided stages",
        ));
    }
    println!(
        "TRAIN from {} (discarded malformed rows={})",
        input.display(),
        invalid_rows
    );
    println!(
        "Neutral range over time dx={:.6},dy={:.6}; noise span dx={:.6},dy={:.6}",
        drift_span.x, drift_span.y, neutral_span.x, neutral_span.y
    );
    let micro_right_response = if stats[12].n >= 16 {
        stats[12].mean().x - center.x
    } else {
        0.0
    };
    let micro_left_response = if stats[13].n >= 16 {
        center.x - stats[13].mean().x
    } else {
        0.0
    };
    if micro_right_response.abs() < neutral_span.x || micro_left_response.abs() < neutral_span.x {
        println!("CAUTION: measured micro-movement overlaps neutral signal uncertainty. Do not promise 100% retention.");
    }
    Ok(MotionProfile {
        center,
        right,
        left,
        up,
        down,
        neutral_span,
        micro_right_response,
        micro_left_response,
    })
}
pub fn train(input: &Path, out: &Path) -> io::Result<()> {
    let p = learn(input)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(out)?;
    writeln!(
        f,
        "# GuardAdaptive guided motion calibration; cannot prove user intent or physics."
    )?;
    writeln!(f, "format_version = 1")?;
    writeln!(
        f,
        "center_x = {:.10}\ncenter_y = {:.10}",
        p.center.x, p.center.y
    )?;
    writeln!(
        f,
        "right_throw = {:.10}\nleft_throw = {:.10}",
        p.right, p.left
    )?;
    writeln!(f, "up_throw = {:.10}\ndown_throw = {:.10}", p.up, p.down)?;
    writeln!(
        f,
        "neutral_span_x = {:.10}\nneutral_span_y = {:.10}",
        p.neutral_span.x, p.neutral_span.y
    )?;
    writeln!(
        f,
        "micro_right_raw_response = {:.10}\nmicro_left_raw_response = {:.10}",
        p.micro_right_response, p.micro_left_response
    )?;
    println!(
        "PROFILE: {} center=({:.6},{:.6}) response range [+x {:.6},-x {:.6},+y {:.6},-y {:.6}]",
        out.display(),
        p.center.x,
        p.center.y,
        p.right,
        p.left,
        p.up,
        p.down
    );
    println!("PRECISION: no deadzone, keeps residual jitter. BALANCED: protects neutral, sacrifices below noise floor.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precision_preserves_micro_motion_with_perfect_calibration() {
        let p = MotionProfile {
            center: Vec2::new(0.20, -0.05),
            right: 0.8,
            left: 1.2,
            up: 1.05,
            down: 0.95,
            neutral_span: Vec2::new(0.001, 0.001),
            micro_right_response: 0.001,
            micro_left_response: 0.001,
        };
        assert!(p.precision(Vec2::new(0.2002, -0.05)).x > 0.0002);
        assert!(p.precision(p.center).norm() < 1e-10);
    }
    #[test]
    fn precision_and_balanced_are_distinct_tradeoffs() {
        let p = MotionProfile {
            center: Vec2::ZERO,
            right: 1.,
            left: 1.,
            up: 1.,
            down: 1.,
            neutral_span: Vec2::new(0.003, 0.003),
            micro_right_response: 0.001,
            micro_left_response: 0.001,
        };
        assert!(p.precision(Vec2::new(0.001, 0.)).x > 0.0);
        assert_eq!(p.balanced(Vec2::new(0.001, 0.)), Vec2::ZERO);
    }
}
