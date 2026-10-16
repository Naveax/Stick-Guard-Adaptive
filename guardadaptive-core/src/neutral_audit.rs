//! Operator-labeled offline neutral-window statistics for raw XInput captures.
//!
//! This code NEVER infers user intent, authenticates a device ID, re-centers
//! a controller, or loads an automatic profile. Two explicitly acknowledged
//! neutral CSV windows are only *evidence candidates*, not physical truth.
use crate::{capture, profile_store::Stick, Vec2};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static TEMP_NO: AtomicU64 = AtomicU64::new(0);
const HEADER:&str="sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source";
const SAMPLES: usize = 128;
const MAX_MAD: f64 = 0.0015;
const MAX_DISAGREEMENT: f64 = 0.003;
const MAX_OUTLIER_P99: f64 = 0.012;
const MAX_INDIVIDUAL_AXIS_DEVIATION: f64 = 0.03;
const MAX_MISSING_STEP_INTERVAL_US: u64 = 2_000_000;

#[derive(Debug, Clone)]
pub struct Window {
    pub median: Vec2,
    pub mad: Vec2,
    pub deviation_p95: f64,
    pub deviation_p99: f64,
    pub max_axis_deviation: f64,
    pub observed_range_x: f64,
    pub observed_range_y: f64,
    pub duplicated_packets: u64,
    pub changed_packets: u64,
    pub max_observed_interval_us: u64,
}
#[derive(Debug, Clone)]
pub struct NeutralAudit {
    pub first: Window,
    pub second: Window,
    pub center_candidate: Vec2,
    pub disagreement: f64,
    pub max_mad: f64,
    pub max_outlier_p99: f64,
    pub observational_policy_pass: bool,
    pub slot_hardware_identity_verified: bool,
    pub physical_release_verified: bool,
}
fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    (values[values.len() / 2 - 1] + values[values.len() / 2]) * 0.5
}
fn quantile_sorted(values: &[f64], percent: usize) -> f64 {
    let n = values.len();
    values[(n * percent).div_ceil(100).saturating_sub(1)]
}
fn summarize(
    samples: &[Vec2; SAMPLES],
    packets: &[u32; SAMPLES],
    times: &[u64; SAMPLES],
) -> Window {
    let mut x = samples.iter().map(|v| v.x).collect::<Vec<_>>();
    let mut y = samples.iter().map(|v| v.y).collect::<Vec<_>>();
    let mx = median(&mut x);
    let my = median(&mut y);
    let mut dx = samples.iter().map(|v| (v.x - mx).abs()).collect::<Vec<_>>();
    let mut dy = samples.iter().map(|v| (v.y - my).abs()).collect::<Vec<_>>();
    let axis_max = dx
        .iter()
        .zip(&dy)
        .map(|(a, b)| a.max(*b))
        .fold(0.0, f64::max);
    let mut radial = samples
        .iter()
        .map(|v| (*v - Vec2::new(mx, my)).norm())
        .collect::<Vec<_>>();
    let mad = Vec2::new(median(&mut dx) * 1.4826, median(&mut dy) * 1.4826);
    radial.sort_by(f64::total_cmp);
    let mut repeated = 0;
    let mut changed = 0;
    let mut max_interval = 0;
    for i in 1..SAMPLES {
        if packets[i] == packets[i - 1] {
            repeated += 1
        } else {
            changed += 1
        }
        max_interval = max_interval.max(times[i] - times[i - 1]);
    }
    Window {
        median: Vec2::new(mx, my),
        mad,
        deviation_p95: quantile_sorted(&radial, 95),
        deviation_p99: quantile_sorted(&radial, 99),
        max_axis_deviation: axis_max,
        observed_range_x: x[SAMPLES - 1] - x[0],
        observed_range_y: y[SAMPLES - 1] - y[0],
        duplicated_packets: repeated,
        changed_packets: changed,
        max_observed_interval_us: max_interval,
    }
}
fn parse_window(path: &Path, stick: Stick) -> io::Result<Window> {
    // This is a bounded, strict parser for our own read-only capture CSV.
    let f = fs::File::open(path)?;
    if f.metadata()?.len() > 100_000 {
        return Err(invalid(
            "neutral capture must be exactly 128 rows, <=100 KB",
        ));
    }
    let mut lines = BufReader::new(f).lines();
    let header = lines
        .next()
        .ok_or_else(|| invalid("missing XInput capture header"))??;
    if header != HEADER {
        return Err(invalid("unrecognized raw XInput capture schema"));
    }
    let mut samples = [Vec2::ZERO; SAMPLES];
    let mut packets = [0_u32; SAMPLES];
    let mut times = [0_u64; SAMPLES];
    let mut count = 0_usize;
    for line in lines {
        if count >= SAMPLES {
            return Err(invalid("too many neutral window samples"));
        }
        let line = line?;
        let cols = line.split(',').collect::<Vec<_>>();
        if cols.len() != 11 || cols[10] != "xinput_readonly" {
            return Err(invalid("invalid XInput capture row or source"));
        }
        if cols[0].parse::<usize>().ok() != Some(count) {
            return Err(invalid("sample index must be sequential and zero-based"));
        }
        let elapsed = cols[1]
            .parse::<u64>()
            .map_err(|_| invalid("invalid elapsed_us"))?;
        if count > 0 {
            if elapsed < times[count - 1] {
                return Err(invalid("nonmonotonic elapsed time"));
            }
            if elapsed - times[count - 1] > MAX_MISSING_STEP_INTERVAL_US {
                return Err(invalid("capture contains >2 second discontinuity"));
            }
        }
        let packet = cols[2]
            .parse::<u32>()
            .map_err(|_| invalid("invalid packet"))?;
        // Validate BOTH sticks, even if only one is selected.
        let axis = cols[3..7]
            .iter()
            .map(|value| {
                let n = value
                    .parse::<f64>()
                    .map_err(|_| invalid("invalid analog coordinate"))?;
                if !n.is_finite() || !(-1.0..=1.0).contains(&n) {
                    return Err(invalid("nonfinite or out-of-range analog coordinate"));
                }
                Ok(n)
            })
            .collect::<io::Result<Vec<_>>>()?;
        cols[7]
            .parse::<u16>()
            .map_err(|_| invalid("invalid buttons"))?;
        cols[8]
            .parse::<u8>()
            .map_err(|_| invalid("invalid left trigger"))?;
        cols[9]
            .parse::<u8>()
            .map_err(|_| invalid("invalid right trigger"))?;
        samples[count] = match stick {
            Stick::Left => Vec2::new(axis[0], axis[1]),
            Stick::Right => Vec2::new(axis[2], axis[3]),
        };
        packets[count] = packet;
        times[count] = elapsed;
        count += 1;
    }
    if count != SAMPLES {
        return Err(invalid(
            "neutral window requires exactly 128 XInput observations",
        ));
    }
    Ok(summarize(&samples, &packets, &times))
}
impl NeutralAudit {
    pub fn json(&self, stick: Stick) -> String {
        fn window(w: &Window) -> String {
            format!(
                concat!(
                    "{{\"center\":[{:.10},{:.10}],\"mad\":[{:.10},{:.10}],",
                    "\"deviation_p95\":{:.10},\"deviation_p99\":{:.10},",
                    "\"max_axis_deviation\":{:.10},\"range\":[{:.10},{:.10}],",
                    "\"repeated_packets\":{},\"changed_packets\":{},",
                    "\"max_interval_us\":{}}}"
                ),
                w.median.x,
                w.median.y,
                w.mad.x,
                w.mad.y,
                w.deviation_p95,
                w.deviation_p99,
                w.max_axis_deviation,
                w.observed_range_x,
                w.observed_range_y,
                w.duplicated_packets,
                w.changed_packets,
                w.max_observed_interval_us
            )
        }
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"two_operator_labeled_xinput_csv_windows\",",
                "\"stick\":\"{}\",\"samples_per_window\":128,",
                "\"first\":{},\"second\":{},",
                "\"center_candidate\":[{:.10},{:.10}],",
                "\"window_disagreement\":{:.10},\"max_axis_mad\":{:.10},",
                "\"max_window_p99_deviation\":{:.10},",
                "\"observational_policy_pass\":{},",
                "\"operator_claimed_two_neutral_releases\":true,",
                "\"physical_release_verified\":false,",
                "\"stable_hardware_identity_verified\":false,",
                "\"controller_profile_created\":false,",
                "\"input_to_game_latency_measured\":false,",
                "\"production_qualified\":false}}\n"
            ),
            stick.name(),
            window(&self.first),
            window(&self.second),
            self.center_candidate.x,
            self.center_candidate.y,
            self.disagreement,
            self.max_mad,
            self.max_outlier_p99,
            self.observational_policy_pass
        )
    }
}
pub fn analyze(first: &Path, second: &Path, stick: Stick) -> io::Result<NeutralAudit> {
    // Reject the same file twice; two independently captured windows needed.
    if fs::canonicalize(first)? == fs::canonicalize(second)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "two different neutral capture files required",
        ));
    }
    // Separate paths are insufficient: reject copied identical recordings.
    // Inputs are already size-limited to 100 KB each by parse_window.
    if fs::metadata(first)?.len() <= 100_000
        && fs::metadata(second)?.len() <= 100_000
        && fs::read(first)? == fs::read(second)?
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "identical capture bytes do not constitute two separate observations",
        ));
    }
    let first = parse_window(first, stick)?;
    let second = parse_window(second, stick)?;
    let disagreement = (first.median - second.median).norm();
    let max_mad = first
        .mad
        .x
        .max(first.mad.y)
        .max(second.mad.x)
        .max(second.mad.y);
    let p99 = first.deviation_p99.max(second.deviation_p99);
    let pass = max_mad <= MAX_MAD
        && disagreement <= MAX_DISAGREEMENT
        && p99 <= MAX_OUTLIER_P99
        && first.max_axis_deviation <= MAX_INDIVIDUAL_AXIS_DEVIATION
        && second.max_axis_deviation <= MAX_INDIVIDUAL_AXIS_DEVIATION;
    let center_candidate = (first.median + second.median) * 0.5;
    Ok(NeutralAudit {
        first,
        second,
        center_candidate,
        disagreement,
        max_mad,
        max_outlier_p99: p99,
        observational_policy_pass: pass,
        slot_hardware_identity_verified: false,
        physical_release_verified: false,
    })
}
pub fn save(report: &NeutralAudit, stick: Stick, out: &Path) -> io::Result<()> {
    let file = out
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output file name missing"))?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = out.with_file_name(format!(
        ".{}.{}.{}.neutral-audit-tmp",
        file.to_string_lossy(),
        std::process::id(),
        TEMP_NO.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created = false;
    let result = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        created = true;
        f.write_all(report.json(stick).as_bytes())?;
        f.sync_all()?;
        drop(f);
        capture::publish_new(&temp, out)
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn run(first: &Path, second: &Path, stick: Stick, ack: &str, out: &Path) -> io::Result<()> {
    if ack != "TWO_RELEASES" {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied,
            "requires explicit --acknowledge TWO_RELEASES, meaning operator states two separate released windows"));
    }
    let r = analyze(first, second, stick)?;
    save(&r, stick, out)?;
    println!(
        "NEUTRAL OBSERVATION: center=({:+.7},{:+.7}) disagreement={:.7} max MAD={:.7} p99={:.7}",
        r.center_candidate.x, r.center_candidate.y, r.disagreement, r.max_mad, r.max_outlier_p99
    );
    println!(
        "OBSERVATIONAL_POLICY_PASS={} BUT USER RELEASE AND PHYSICAL ID ARE NOT VERIFIED",
        r.observational_policy_pass
    );
    println!(
        "NO DEVICE PROFILE ACTIVATED. NO GAME INPUT MODIFIED. JSON={}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, center: Vec2, spike: Option<(usize, f64)>) {
        let mut rows = vec![HEADER.to_owned()];
        for i in 0..SAMPLES {
            let noise = if i % 2 == 0 { 0.00006 } else { -0.00006 };
            let outlier = if spike.is_some_and(|s| s.0 == i) {
                spike.unwrap().1
            } else {
                0.0
            };
            rows.push(format!(
                "{},{},{},{:.10},{:.10},0.0000000000,0.0000000000,0,0,0,xinput_readonly",
                i,
                i * 8000,
                7 + i / 2,
                center.x + noise + outlier,
                center.y - noise
            ));
        }
        fs::write(path, rows.join("\n") + "\n").unwrap();
    }
    fn paths(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "guardadaptive-v18-{}-{}-{}",
            tag,
            std::process::id(),
            TEMP_NO.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        (
            dir.join("first.csv"),
            dir.join("second.csv"),
            dir.join("audit.json"),
        )
    }
    #[test]
    fn repeatable_clean_windows_are_observational_only() {
        let (a, b, out) = paths("clean");
        fixture(&a, Vec2::new(0.4, -0.2), None);
        fixture(&b, Vec2::new(0.4005, -0.2001), None);
        let x = analyze(&a, &b, Stick::Left).unwrap();
        assert!(x.observational_policy_pass);
        assert!((x.center_candidate.x - 0.40025).abs() < 1e-8);
        assert_eq!(x.first.duplicated_packets, 64);
        assert_eq!(x.first.changed_packets, 63);
        assert!(x.max_mad > 0.0 && x.max_mad < 0.0015);
        let j = x.json(Stick::Left);
        assert!(j.contains("\"observational_policy_pass\":true"));
        assert!(j.contains("\"physical_release_verified\":false"));
        assert!(j.contains("\"stable_hardware_identity_verified\":false"));
        assert!(j.contains("\"controller_profile_created\":false"));
        run(&a, &b, Stick::Left, "TWO_RELEASES", &out).unwrap();
        let original = fs::read(&out).unwrap();
        assert!(save(&x, Stick::Left, &out).is_err());
        assert_eq!(fs::read(&out).unwrap(), original);
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }
    #[test]
    fn high_consistent_bias_is_not_proof_of_physical_release() {
        let (a, b, out) = paths("bias");
        fixture(&a, Vec2::new(0.96, -0.91), None);
        fixture(&b, Vec2::new(0.9601, -0.9101), None);
        let r = analyze(&a, &b, Stick::Left).unwrap();
        assert!(
            r.observational_policy_pass,
            "low temporal variance alone cannot infer stick intent"
        );
        assert!(!r.physical_release_verified);
        assert!(!r.slot_hardware_identity_verified);
        assert!(!out.exists());
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }
    #[test]
    fn unstable_noise_and_window_disagreement_fail_policy() {
        let (a, b, _) = paths("unstable");
        fixture(&a, Vec2::new(0.3, -0.2), Some((120, 0.15)));
        fixture(&b, Vec2::new(0.35, -0.2), None);
        let r = analyze(&a, &b, Stick::Left).unwrap();
        assert!(!r.observational_policy_pass);
        assert!(r.disagreement > 0.003);
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }
    #[test]
    fn malformed_csv_or_missing_user_ack_do_not_publish() {
        let (a, b, out) = paths("invalid");
        fixture(&a, Vec2::new(0.2, 0.0), None);
        fixture(&b, Vec2::new(0.2, 0.0), None);
        assert_eq!(
            run(&a, &b, Stick::Left, "no", &out).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!out.exists());
        assert!(analyze(&a, &a, Stick::Left).is_err());
        // Different paths do not excuse duplicate capture contents.
        assert!(analyze(&a, &b, Stick::Left).is_err());
        let original = fs::read_to_string(&b).unwrap();
        fs::write(&b, original.replace(",xinput_readonly", ",spoofed")).unwrap();
        assert!(analyze(&a, &b, Stick::Left).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }
    #[test]
    fn extreme_single_frame_is_reported_even_if_mad_small() {
        let (a, b, _) = paths("spike");
        fixture(&a, Vec2::new(0.3, -0.2), Some((120, 0.15)));
        fixture(&b, Vec2::new(0.3, -0.2), None);
        let r = analyze(&a, &b, Stick::Left).unwrap();
        assert!(r.first.max_axis_deviation > 0.14);
        assert!(
            !r.observational_policy_pass,
            "even a single spike must fail the conservative observational policy"
        );
        let _ = fs::remove_dir_all(a.parent().unwrap());
    }
}
