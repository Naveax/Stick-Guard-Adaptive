//! v0.41: fail-closed dataset reference pairing, for future real-controller
//! sessions. A reference CSV's origin cannot be authenticated merely because
//! it exists. Absent provenance means NO accuracy conclusion.
use crate::{profile_store::Stick, Vec2};
use std::{fs, io, path::Path};
const RAW_HEADER:&str="sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source";
const REFERENCE_HEADER: &str = "sample,elapsed_us,intended_x,intended_y,reference_source";
const MIN_ROWS: usize = 256;
const MAX_ROWS: usize = 100_000;
const MAX_FILE_BYTES: u64 = 20_000_000;
const MAX_TIME_SKEW_US: u64 = 4_000;
const MAX_POLL_GAP_US: u64 = 50_000;
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    SyntheticFixture,
    ReportedDevice,
    ExternallyDeclared,
}
impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::SyntheticFixture => "synthetic_fixture",
            Self::ReportedDevice => "xinput_readonly_not_identity_authenticated",
            Self::ExternallyDeclared => "external_reference_declared_unverified",
        }
    }
}
#[derive(Clone, Copy)]
struct Raw {
    time: u64,
    packet: u32,
    stick: Vec2,
}
#[derive(Clone, Copy)]
struct Reference {
    time: u64,
    intent: Vec2,
}
fn strict_read(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_FILE_BYTES {
        return Err(invalid("not an ordinary bounded file"));
    }
    fs::read_to_string(path)
}
fn float(s: &str) -> io::Result<f64> {
    let v = s.parse::<f64>().map_err(|_| invalid("invalid float"))?;
    if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
        return Err(invalid("nonfinite or out-of-range axis"));
    }
    Ok(v)
}
fn time(s: &str) -> io::Result<u64> {
    s.parse::<u64>().map_err(|_| invalid("timestamp invalid"))
}
fn row_count(n: usize) -> io::Result<()> {
    if !(MIN_ROWS..=MAX_ROWS).contains(&n) {
        return Err(invalid("invalid row count 256..100000"));
    }
    Ok(())
}
fn ordering(prev_t: Option<u64>, now: u64, limit: u64) -> io::Result<()> {
    if let Some(p) = prev_t {
        if now <= p || now - p > limit {
            return Err(invalid("clock rollback, duplicate time, or read gap"));
        }
    }
    Ok(())
}
fn raw_rows(path: &Path, stick: Stick) -> io::Result<(Vec<Raw>, Source)> {
    let text = strict_read(path)?;
    let mut lines = text.lines();
    if lines.next() != Some(RAW_HEADER) {
        return Err(invalid("raw CSV header mismatch"));
    }
    let mut rows = Vec::new();
    let mut origin = None;
    let mut prev_t = None;
    let mut prev_packet = None;
    for (i, line) in lines.enumerate() {
        if i >= MAX_ROWS {
            return Err(invalid("raw capture exceeds bounded row count"));
        }
        let cols = line.split(',').collect::<Vec<_>>();
        if cols.len() != 11 || cols[0].parse::<usize>().ok() != Some(i) {
            return Err(invalid("raw capture index or field count wrong"));
        }
        let source = match cols[10] {
            "synthetic_fixture" => Source::SyntheticFixture,
            "xinput_readonly" => Source::ReportedDevice,
            _ => return Err(invalid("raw provenance unrecognized")),
        };
        if origin.is_some_and(|old| old != source) {
            return Err(invalid("mixed raw provenance"));
        }
        origin = Some(source);
        let t = time(cols[1])?;
        ordering(prev_t, t, MAX_POLL_GAP_US)?;
        let packet = cols[2]
            .parse::<u32>()
            .map_err(|_| invalid("packet not u32"))?;
        if prev_packet.is_some_and(|p| packet < p) {
            return Err(invalid("XInput packet rollback"));
        }
        let axes = cols[3..7]
            .iter()
            .map(|s| float(s))
            .collect::<io::Result<Vec<_>>>()?;
        cols[7]
            .parse::<u16>()
            .map_err(|_| invalid("buttons not u16"))?;
        cols[8]
            .parse::<u8>()
            .map_err(|_| invalid("left trigger not u8"))?;
        cols[9]
            .parse::<u8>()
            .map_err(|_| invalid("right trigger not u8"))?;
        let stick = if stick == Stick::Left {
            Vec2::new(axes[0], axes[1])
        } else {
            Vec2::new(axes[2], axes[3])
        };
        rows.push(Raw {
            time: t,
            packet,
            stick,
        });
        prev_t = Some(t);
        prev_packet = Some(packet);
    }
    row_count(rows.len())?;
    Ok((rows, origin.ok_or_else(|| invalid("raw capture empty"))?))
}
fn reference_rows(path: &Path) -> io::Result<(Vec<Reference>, Source)> {
    let text = strict_read(path)?;
    let mut lines = text.lines();
    if lines.next() != Some(REFERENCE_HEADER) {
        return Err(invalid("independent-reference schema mismatch"));
    }
    let mut rows = Vec::new();
    let mut origin = None;
    let mut prev_t = None;
    for (i, line) in lines.enumerate() {
        if i >= MAX_ROWS {
            return Err(invalid("reference exceeds bounded row count"));
        }
        let cols = line.split(',').collect::<Vec<_>>();
        if cols.len() != 5 || cols[0].parse::<usize>().ok() != Some(i) {
            return Err(invalid("reference index or fields mismatch"));
        }
        let source = match cols[4] {
            "synthetic_fixture" => Source::SyntheticFixture,
            "external_declared_unverified" => Source::ExternallyDeclared,
            _ => return Err(invalid("unrecognized or claimed-verified reference source")),
        };
        if origin.is_some_and(|old| old != source) {
            return Err(invalid("mixed reference origins"));
        }
        origin = Some(source);
        let t = time(cols[1])?;
        ordering(prev_t, t, MAX_POLL_GAP_US)?;
        rows.push(Reference {
            time: t,
            intent: Vec2::new(float(cols[2])?, float(cols[3])?),
        });
        prev_t = Some(t);
    }
    row_count(rows.len())?;
    Ok((rows, origin.ok_or_else(|| invalid("reference empty"))?))
}
#[derive(Clone, Copy, Default)]
struct Metrics {
    n: u64,
    baseline_sse: f64,
    centered_sse: f64,
    micro_frames: u64,
    micro_lost_baseline: u64,
    micro_lost_centered: u64,
    wrong_baseline: u64,
    wrong_centered: u64,
}
impl Metrics {
    fn add(&mut self, truth: Vec2, raw: Vec2, centered: Vec2) {
        self.n += 1;
        self.baseline_sse += (raw - truth).length_squared();
        self.centered_sse += (centered - truth).length_squared();
        if truth.norm() > 0.0 && truth.norm() <= 0.01 {
            self.micro_frames += 1;
            if raw.norm() < truth.norm() * 0.5 {
                self.micro_lost_baseline += 1;
            }
            if centered.norm() < truth.norm() * 0.5 {
                self.micro_lost_centered += 1;
            }
        }
        if truth.norm() > 0.0001 {
            if raw.x * truth.x + raw.y * truth.y < 0.0 {
                self.wrong_baseline += 1;
            }
            if centered.x * truth.x + centered.y * truth.y < 0.0 {
                self.wrong_centered += 1;
            }
        }
    }
    fn rmse(sse: f64, n: u64) -> f64 {
        (sse / n.max(1) as f64).sqrt()
    }
}
fn neutral_calibration(rows: &[Raw], refs: &[Reference]) -> io::Result<Vec2> {
    // Reference dataset must explicitly claim the first 128 frames are zero
    // intended input; this is STILL an unverified external declaration.
    if refs[0..128].iter().any(|r| r.intent.norm() > 0.0001) {
        return Err(invalid("first 128 must be externally declared neutral"));
    }
    let mean = rows[0..128].iter().fold(Vec2::ZERO, |acc, x| acc + x.stick) * (1.0 / 128.0);
    let half1 = rows[0..64].iter().fold(Vec2::ZERO, |a, x| a + x.stick) * (1.0 / 64.0);
    let half2 = rows[64..128].iter().fold(Vec2::ZERO, |a, x| a + x.stick) * (1.0 / 64.0);
    let spread = rows[0..128]
        .iter()
        .map(|r| (r.stick - mean).norm())
        .fold(0.0, f64::max);
    if (half1 - half2).norm() > 0.003 || spread > 0.005 {
        return Err(invalid("unstable declared-neutral calibration"));
    }
    Ok(mean)
}
pub fn audit(raw: &Path, reference: Option<&Path>, stick: Stick, out: &Path) -> io::Result<()> {
    if reference.is_some_and(|r| r == raw || fs::canonicalize(r).ok() == fs::canonicalize(raw).ok())
    {
        return Err(invalid(
            "same file cannot serve as raw and independent reference",
        ));
    }
    let (rows, raw_source) = raw_rows(raw, stick)?;
    let mut result = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v41_reference_pairing_audit\",",
            "\"stick\":\"{}\",\"raw_source\":\"{}\",\"rows\":{},",
            "\"first_packet\":{},\"last_packet\":{},",
            "\"reference_present\":{}"
        ),
        stick.name(),
        raw_source.as_str(),
        rows.len(),
        rows[0].packet,
        rows[rows.len() - 1].packet,
        reference.is_some()
    );
    if let Some(path) = reference {
        let (truth, ref_source) = reference_rows(path)?;
        if rows.len() != truth.len() {
            return Err(invalid("reference and raw frame count differ"));
        }
        for (raw, reference) in rows.iter().zip(truth.iter()) {
            if raw.time.abs_diff(reference.time) > MAX_TIME_SKEW_US {
                return Err(invalid("timestamps disagree beyond 4ms"));
            }
        }
        let center = neutral_calibration(&rows, &truth)?;
        let mut metrics = Metrics::default();
        for (raw, r) in rows.iter().zip(truth.iter()).skip(128) {
            metrics.add(r.intent, raw.stick, (raw.stick - center).saturate());
        }
        result.push_str(&format!(
            concat!(
                ",\"reference_source\":\"{}\",\"reference_count\":{},",
                "\"declared_neutral_calibration_frames\":128,",
                "\"measured_frames\":{},\"baseline_unadjusted_rmse\":{:.10},",
                "\"fixed_center_rmse\":{:.10},",
                "\"reference_declared_micro_frames\":{},",
                "\"raw_micro_lost\":{},\"centered_micro_lost\":{},",
                "\"raw_wrong_direction\":{},\"centered_wrong_direction\":{}"
            ),
            ref_source.as_str(),
            truth.len(),
            metrics.n,
            Metrics::rmse(metrics.baseline_sse, metrics.n),
            Metrics::rmse(metrics.centered_sse, metrics.n),
            metrics.micro_frames,
            metrics.micro_lost_baseline,
            metrics.micro_lost_centered,
            metrics.wrong_baseline,
            metrics.wrong_centered
        ));
    } else {
        // No oracle => no accuracy, RMSE, micro-loss or wrong-direction claims.
        result.push_str(",\"reference_source\":\"absent\",\"accuracy_scoring_available\":false");
    }
    result.push_str(concat!(
        ",\"external_reference_independence_verified\":false,",
        "\"physical_device_identity_verified\":false,",
        "\"physical_intent_truth_verified\":false,",
        "\"automatic_drift_learning\":false,",
        "\"correction_promoted\":false,\"game_writes\":0,",
        "\"production_gate\":\"BLOCKED\"}\n"
    ));
    crate::truth_lab::save_new(out, &result)?;
    println!("V041 pair-audit: raw_frames={} reference={} PROVENANCE_UNVERIFIED, GAME_WRITES=0, PROD=BLOCKED",
        rows.len(),if reference.is_some(){"DECLARED"}else{"ABSENT"});
    Ok(())
}
/// Reuse the established strict v0.41 parser for v0.67 dataset gates.
/// Validation says nothing about physical truth or authentic provenance.
pub(crate) struct CheckedSplit {
    pub(crate) rows: usize,
    pub(crate) raw_origin: &'static str,
    pub(crate) reference_origin: &'static str,
}
pub(crate) fn checked_split(
    raw: &Path,
    reference: Option<&Path>,
    stick: Stick,
) -> io::Result<CheckedSplit> {
    if reference.is_some_and(|r| r == raw || fs::canonicalize(r).ok() == fs::canonicalize(raw).ok())
    {
        return Err(invalid("raw and reference cannot be the same file"));
    }
    let (raw_data, origin) = raw_rows(raw, stick)?;
    let mut reference_origin = "absent";
    if let Some(path) = reference {
        let (truth, src) = reference_rows(path)?;
        if truth.len() != raw_data.len() {
            return Err(invalid("reference count mismatch"));
        }
        for (a, b) in raw_data.iter().zip(truth.iter()) {
            if a.time.abs_diff(b.time) > MAX_TIME_SKEW_US {
                return Err(invalid("raw/reference timestamp skew"));
            }
        }
        neutral_calibration(&raw_data, &truth)?;
        reference_origin = src.as_str();
    }
    Ok(CheckedSplit {
        rows: raw_data.len(),
        raw_origin: origin.as_str(),
        reference_origin,
    })
}

/// v0.68 exposes already-strictly validated *in-memory* samples for
/// training-only fixed-center comparison. It does NOT authenticate reference
/// intent, hardware origin or user consent.
pub(crate) struct ComparisonFrames {
    pub(crate) axes: Vec<Vec2>,
    pub(crate) declared_intentions: Option<Vec<Vec2>>,
    pub(crate) declared_neutral_center: Option<Vec2>,
}
pub(crate) fn comparison_frames(
    raw: &Path,
    reference: Option<&Path>,
    stick: Stick,
) -> io::Result<ComparisonFrames> {
    if reference.is_some_and(|r| r == raw || fs::canonicalize(r).ok() == fs::canonicalize(raw).ok())
    {
        return Err(invalid("raw/reference canonical path alias"));
    }
    let (raw_rows, _) = raw_rows(raw, stick)?;
    let (intents, center) = if let Some(path) = reference {
        let (ref_rows, _) = reference_rows(path)?;
        if ref_rows.len() != raw_rows.len() {
            return Err(invalid("reference row count mismatch"));
        }
        for (a, b) in raw_rows.iter().zip(ref_rows.iter()) {
            if a.time.abs_diff(b.time) > MAX_TIME_SKEW_US {
                return Err(invalid("raw/reference timestamp mismatch"));
            }
        }
        let c = neutral_calibration(&raw_rows, &ref_rows)?;
        (Some(ref_rows.iter().map(|r| r.intent).collect()), Some(c))
    } else {
        (None, None)
    };
    Ok(ComparisonFrames {
        axes: raw_rows.iter().map(|r| r.stick).collect(),
        declared_intentions: intents,
        declared_neutral_center: center,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ga-v41-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn fixture(
        dir: &Path,
        n: usize,
        raw_source: &str,
        ref_source: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        fs::create_dir_all(dir).unwrap();
        let mut raw = String::from(RAW_HEADER);
        raw.push('\n');
        let mut reference = String::from(REFERENCE_HEADER);
        reference.push('\n');
        for i in 0..n {
            let intention = if i < 128 {
                0.0
            } else if i % 2 == 0 {
                0.002
            } else {
                -0.002
            };
            raw.push_str(&format!(
                "{i},{},{},{:.8},-0.10000000,0.0,0.0,0,10,20,{raw_source}\n",
                (i + 1) * 8000,
                i / 2,
                0.20 + intention
            ));
            reference.push_str(&format!(
                "{i},{},{intention:.8},0.0,{ref_source}\n",
                (i + 1) * 8000
            ));
        }
        let a = dir.join("raw.csv");
        let b = dir.join("intent.csv");
        fs::write(&a, raw).unwrap();
        fs::write(&b, reference).unwrap();
        (a, b)
    }
    #[test]
    fn missing_reference_omits_accuracy_claims() {
        let dir = temp();
        let (raw, _) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let out = dir.join("raw-only.json");
        audit(&raw, None, Stick::Left, &out).unwrap();
        let s = fs::read_to_string(&out).unwrap();
        assert!(s.contains("\"accuracy_scoring_available\":false"));
        assert!(!s.contains("fixed_center_rmse"));
        assert!(s.contains("\"production_gate\":\"BLOCKED\""));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn synthetic_truth_is_scored_but_never_verified() {
        let dir = temp();
        let (raw, reference) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let out = dir.join("paired.json");
        audit(&raw, Some(&reference), Stick::Left, &out).unwrap();
        let s = fs::read_to_string(&out).unwrap();
        assert!(s.contains("\"reference_source\":\"synthetic_fixture\""));
        assert!(s.contains("\"physical_intent_truth_verified\":false"));
        assert!(s.contains("\"fixed_center_rmse\":"));
        assert!(audit(&raw, Some(&reference), Stick::Left, &out).is_err());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn reject_dishonest_and_mismatched_references_before_publishing() {
        let dir = temp();
        let (raw, reference) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let original = fs::read_to_string(&reference).unwrap();
        let out = dir.join("no-report.json");
        fs::write(
            &reference,
            original.replace("synthetic_fixture", "physically_verified"),
        )
        .unwrap();
        assert!(audit(&raw, Some(&reference), Stick::Left, &out).is_err());
        assert!(!out.exists());
        fs::write(&reference, original.replace("4096000,", "4105000,")).unwrap();
        assert!(audit(&raw, Some(&reference), Stick::Left, &out).is_err());
        assert!(!out.exists());
        fs::write(&reference, original.replace("0.00200000", "NaN")).unwrap();
        assert!(audit(&raw, Some(&reference), Stick::Left, &out).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn fail_on_clock_rollback_and_changed_packet() {
        let dir = temp();
        let (raw, reference) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let baseline = fs::read_to_string(&raw).unwrap();
        fs::write(&raw, baseline.replacen("1,16000,0,", "1,8000,0,", 1)).unwrap();
        assert!(audit(&raw, Some(&reference), Stick::Left, &dir.join("bad.json")).is_err());
        // Packet repeats are fine (counter equality); a rollback is not.
        let old = baseline.replacen("4,40000,2,", "4,40000,0,", 1);
        fs::write(&raw, old).unwrap();
        assert!(audit(&raw, Some(&reference), Stick::Left, &dir.join("bad2.json")).is_err());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn externally_declared_truth_is_still_not_physical_certification() {
        let dir = temp();
        let (raw, reference) =
            fixture(&dir, 512, "xinput_readonly", "external_declared_unverified");
        let out = dir.join("operator-reference.json");
        audit(&raw, Some(&reference), Stick::Left, &out).unwrap();
        let content = fs::read_to_string(out).unwrap();
        assert!(content.contains("\"reference_source\":\"external_reference_declared_unverified\""));
        assert!(content.contains("\"raw_source\":\"xinput_readonly_not_identity_authenticated\""));
        assert!(content.contains("\"external_reference_independence_verified\":false"));
        assert!(content.contains("\"physical_device_identity_verified\":false"));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn mismatch_counts_or_false_neutral_window_never_publish_report() {
        let dir = temp();
        let (raw, reference) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let original = fs::read_to_string(&reference).unwrap();
        let shortened = original.lines().take(500).collect::<Vec<_>>().join("\n") + "\n";
        fs::write(&reference, shortened).unwrap();
        assert!(audit(
            &raw,
            Some(&reference),
            Stick::Left,
            &dir.join("missing.json")
        )
        .is_err());
        assert!(!dir.join("missing.json").exists());
        fs::write(
            &reference,
            original.replacen(
                "0.00000000,0.0,synthetic_fixture",
                "0.2,0.0,synthetic_fixture",
                1,
            ),
        )
        .unwrap();
        assert!(audit(
            &raw,
            Some(&reference),
            Stick::Left,
            &dir.join("false-neutral.json")
        )
        .is_err());
        assert!(!dir.join("false-neutral.json").exists());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn cannot_self_reference_or_reuse_report() {
        let dir = temp();
        let (raw, _) = fixture(&dir, 512, "synthetic_fixture", "synthetic_fixture");
        let out = dir.join("failed.json");
        assert!(audit(&raw, Some(&raw), Stick::Left, &out).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(dir);
    }
}
