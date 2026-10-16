//! Read-only, deterministic replay of manually acquired XInput evidence.
//! Recomputes both existing reports from raw captures and checks cross-stage
//! packet monotonicity. Not proof of user intent or hardware provenance.
use crate::{
    capture,
    neutral_audit::{self, NeutralAudit},
    neutral_session::ExerciseEvidence,
    profile_store::Stick,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static TEMP: AtomicU64 = AtomicU64::new(0);
const RAW_HEADER:&str="sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source";

fn bad(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
/// Read only well-defined captures. Each stage has its own time origin; only
/// packet monotonicity can be compared across capture windows.
fn boundaries(path: &Path, rows: usize) -> io::Result<(u32, u32)> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 150_000 {
        return Err(bad("raw stage exceeds 150 KB"));
    }
    let mut lines = BufReader::new(file).lines();
    if lines.next().transpose()?.as_deref() != Some(RAW_HEADER) {
        return Err(bad("bad raw capture header"));
    }
    let mut first = None;
    let mut last = None;
    for index in 0..rows {
        let line = lines.next().ok_or_else(|| bad("raw stage too short"))??;
        let parts = line.split(',').collect::<Vec<_>>();
        if parts.len() != 11
            || parts[0].parse::<usize>().ok() != Some(index)
            || parts[10] != "xinput_readonly"
        {
            return Err(bad("raw stage field/index mismatch"));
        }
        let packet = parts[2].parse::<u32>().map_err(|_| bad("bad packet"))?;
        if let Some(prev) = last {
            if packet < prev {
                return Err(bad("raw packet reset inside stage"));
            }
        }
        first.get_or_insert(packet);
        last = Some(packet);
    }
    if lines.next().is_some() {
        return Err(bad("raw stage too long"));
    }
    Ok((
        first.ok_or_else(|| bad("empty raw stage"))?,
        last.ok_or_else(|| bad("empty raw stage"))?,
    ))
}
fn verify_report(path: &Path, expected: &str) -> io::Result<()> {
    let file = fs::read(path)?;
    if file.len() > 16_384 || file != expected.as_bytes() {
        return Err(bad(
            "persisted audit differs from deterministic replay of raw files",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone)]
pub struct SessionFacts {
    pub first_center: crate::Vec2,
    pub second_center: crate::Vec2,
    pub disagreement: f64,
    pub max_mad: f64,
    pub max_p99: f64,
    pub max_exercise_excursion: f64,
    pub observational_policy_pass: bool,
    pub first_packet: u32,
    pub final_packet: u32,
    pub repeated_observation_possible: bool,
}
impl SessionFacts {
    pub fn from_dir(dir: &Path, stick: Stick) -> io::Result<Self> {
        // Never silently interpret synthetic lab reports as physical evidence.
        if !dir.is_dir() {
            return Err(bad("session directory missing"));
        }
        let first = dir.join("first.csv");
        let exercise = dir.join("exercise.csv");
        let second = dir.join("second.csv");
        let audit: NeutralAudit = neutral_audit::analyze(&first, &second, stick)?;
        let movement = ExerciseEvidence::analyze(&exercise, stick, audit.first.median)?;
        verify_report(&dir.join("neutral_audit.json"), &audit.json(stick))?;
        verify_report(&dir.join("exercise_audit.json"), &movement.json(stick))?;
        let p1 = boundaries(&first, 128)?;
        let p2 = boundaries(&exercise, 400)?;
        let p3 = boundaries(&second, 128)?;
        if p1.1 > p2.0 || p2.1 > p3.0 {
            return Err(bad("packet sequence reset across capture stages"));
        }
        if movement.max_excursion < 0.075 {
            return Err(bad("exercise excursion too small"));
        }
        Ok(Self {
            first_center: audit.first.median,
            second_center: audit.second.median,
            disagreement: audit.disagreement,
            max_mad: audit.max_mad,
            max_p99: audit.max_outlier_p99,
            max_exercise_excursion: movement.max_excursion,
            observational_policy_pass: audit.observational_policy_pass,
            first_packet: p1.0,
            final_packet: p3.1,
            repeated_observation_possible: p1.0 == p1.1 || p2.0 == p2.1 || p3.0 == p3.1,
        })
    }
}
pub struct Comparison {
    pub target: SessionFacts,
    pub reference: Option<SessionFacts>,
}
impl Comparison {
    pub fn compute(path: &Path, stick: Stick, reference: Option<&Path>) -> io::Result<Self> {
        if let Some(other) = reference {
            if fs::canonicalize(path)? == fs::canonicalize(other)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "reference and target must be distinct session directories",
                ));
            }
        }
        let target = SessionFacts::from_dir(path, stick)?;
        let reference = match reference {
            Some(dir) => Some(SessionFacts::from_dir(dir, stick)?),
            None => None,
        };
        Ok(Self { target, reference })
    }
    fn flags() -> &'static str {
        "\"hardware_identity_verified\":false,\"user_intent_verified\":false,\"input_to_game_latency_measured\":false,\"virtual_controller_enabled\":false,\"production_qualified\":false"
    }
    fn facts(a: &SessionFacts) -> String {
        format!(
            concat!(
                "{{\"first_median\":[{:.10},{:.10}],\"second_median\":[{:.10},{:.10}],",
                "\"neutral_disagreement\":{:.10},\"max_neutral_mad\":{:.10},",
                "\"max_neutral_p99\":{:.10},\"max_exercise_excursion\":{:.10},",
                "\"observational_policy_pass\":{},\"first_packet\":{},\"last_packet\":{}}}"
            ),
            a.first_center.x,
            a.first_center.y,
            a.second_center.x,
            a.second_center.y,
            a.disagreement,
            a.max_mad,
            a.max_p99,
            a.max_exercise_excursion,
            a.observational_policy_pass,
            a.first_packet,
            a.final_packet
        )
    }
    pub fn json(&self, stick: Stick) -> String {
        let reference = self
            .reference
            .as_ref()
            .map_or_else(|| "null".to_owned(), Self::facts);
        let delta = self.reference.as_ref().map_or_else(
            || "null".to_owned(),
            |other| {
                format!(
                    concat!("{{\"center_shift\":{:.10},\"neutral_disagreement_delta\":{:.10},",
                "\"max_neutral_mad_delta\":{:.10},\"max_exercise_excursion_delta\":{:.10}}}"),
                    (self.target.first_center - other.first_center).norm(),
                    self.target.disagreement - other.disagreement,
                    self.target.max_mad - other.max_mad,
                    self.target.max_exercise_excursion - other.max_exercise_excursion
                )
            },
        );
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"offline_xinput_session_replay\",",
                "\"stick\":\"{}\",\"raw_stages_verified\":3,\"audits_recomputed_and_matched\":2,",
                "\"capture_sample_count\":656,",
                "\"target\":{},\"reference\":{},\"comparison\":{},",
                "{}",
                ",\"note\":\"Self-consistent evidence is not authenticated device provenance\"}}\n"
            ),
            stick.name(),
            Self::facts(&self.target),
            reference,
            delta,
            Self::flags()
        )
    }
}
/// Only write after all five evidence files and optional reference validated.
pub fn save(comparison: &Comparison, stick: Stick, out: &Path) -> io::Result<()> {
    if out.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "comparison exists",
        ));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = out
        .file_name()
        .ok_or_else(|| bad("missing output filename"))?;
    let temp = out.with_file_name(format!(
        ".{}.{}.{}.replay-tmp",
        name.to_string_lossy(),
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created = false;
    let result = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        created = true;
        f.write_all(comparison.json(stick).as_bytes())?;
        f.sync_all()?;
        drop(f);
        capture::publish_new(&temp, out)
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn run(dir: &Path, stick: Stick, reference: Option<&Path>, out: &Path) -> io::Result<()> {
    let comparison = Comparison::compute(dir, stick, reference)?;
    save(&comparison, stick, out)?;
    println!("SESSION REPLAY: raw stages=3, reports verified=2, samples=656");
    println!("TARGET: neutral disagreement={:.8}, max MAD={:.8}, exercise excursion={:.8}, observational policy={}",
        comparison.target.disagreement,comparison.target.max_mad,
        comparison.target.max_exercise_excursion,comparison.target.observational_policy_pass);
    if let Some(ref baseline) = comparison.reference {
        println!(
            "REFERENCE: disagreement={:.8}, max MAD={:.8}, exercise excursion={:.8}",
            baseline.disagreement, baseline.max_mad, baseline.max_exercise_excursion
        );
    }
    println!(
        "PHYSICAL ID / USER INTENT / GAME LATENCY: UNVERIFIED. PRODUCTION: BLOCKED. JSON={}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{capture::Reader, neutral_session, xinput::Report, Vec2};
    use std::path::PathBuf;
    struct Fake {
        index: usize,
        delta: f64,
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            let i = self.index;
            self.index += 1;
            if i >= 658 {
                return Ok(None);
            }
            let noise = if i.is_multiple_of(2) {
                0.00003
            } else {
                -0.00003
            };
            let (x, y) = if i <= 128 {
                (0.1 + noise + self.delta, -0.2 - noise)
            } else if i <= 528 {
                (0.34 + self.delta, -0.2)
            } else {
                (0.1004 + noise + self.delta, -0.2001 - noise)
            };
            Ok(Some(Report {
                packet: (i / 2) as u32,
                left: Vec2::new(x, y),
                right: Vec2::ZERO,
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
            }))
        }
    }
    fn newdir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "guardadaptive-v21-{name}-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn session(name: &str, delta: f64) -> PathBuf {
        let p = newdir(name);
        neutral_session::run_with(
            &mut Fake { index: 0, delta },
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut |_, _| Ok(()),
        )
        .unwrap();
        p
    }
    #[test]
    fn complete_session_replays_and_reports_are_still_non_authoritative() {
        let path = session("complete", 0.0);
        let baseline = session("baseline", 0.002);
        let p = newdir("replay.json");
        let cmp = Comparison::compute(&path, Stick::Left, Some(&baseline)).unwrap();
        assert!(cmp.target.max_exercise_excursion > 0.2);
        assert!((cmp.target.first_center.x - 0.1).abs() < 1e-8);
        let j = cmp.json(Stick::Left);
        assert!(j.contains("\"hardware_identity_verified\":false"));
        assert!(j.contains("\"user_intent_verified\":false"));
        assert!(j.contains("\"comparison\":{"));
        run(&path, Stick::Left, Some(&baseline), &p).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), j);
        assert_eq!(
            save(&cmp, Stick::Left, &p).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let _ = fs::remove_file(p);
        let _ = fs::remove_dir_all(path);
        let _ = fs::remove_dir_all(baseline);
    }
    #[test]
    fn tampered_neutral_or_exercise_evidence_is_rejected() {
        let dir = session("tamper", 0.0);
        let original = fs::read(dir.join("exercise.csv")).unwrap();
        let mut altered = String::from_utf8(original.clone()).unwrap();
        altered = altered.replacen("0.3400000000", "0.5000000000", 1);
        assert_ne!(altered.as_bytes(), original);
        fs::write(dir.join("exercise.csv"), altered).unwrap();
        assert!(SessionFacts::from_dir(&dir, Stick::Left).is_err());
        fs::write(dir.join("exercise.csv"), original).unwrap();
        assert!(SessionFacts::from_dir(&dir, Stick::Left).is_ok());
        fs::write(dir.join("neutral_audit.json"), b"{}").unwrap();
        assert!(SessionFacts::from_dir(&dir, Stick::Left).is_err());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn altered_reference_blocks_comparison_and_output() {
        let dir = session("target", 0.0);
        let base = session("ref", 0.001);
        let out = newdir("must-not-exist.json");
        fs::write(
            base.join("exercise_audit.json"),
            b"not a valid canonical audit",
        )
        .unwrap();
        assert!(run(&dir, Stick::Left, Some(&base), &out).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn internally_consistent_audits_still_reject_cross_stage_packet_reset() {
        let dir = session("crossphase", 0.0);
        let exercise = dir.join("exercise.csv");
        let contents = fs::read_to_string(&exercise).unwrap();
        let mut rewritten = Vec::new();
        for (i, line) in contents.lines().enumerate() {
            if i == 0 {
                rewritten.push(line.to_owned());
                continue;
            }
            let mut fields = line.split(',').map(ToOwned::to_owned).collect::<Vec<_>>();
            fields[2] = "0".to_owned();
            rewritten.push(fields.join(","));
        }
        fs::write(&exercise, rewritten.join("\n") + "\n").unwrap();
        let origin =
            neutral_audit::analyze(&dir.join("first.csv"), &dir.join("second.csv"), Stick::Left)
                .unwrap()
                .first
                .median;
        let new_audit = ExerciseEvidence::analyze(&exercise, Stick::Left, origin).unwrap();
        fs::write(dir.join("exercise_audit.json"), new_audit.json(Stick::Left)).unwrap();
        assert!(SessionFacts::from_dir(&dir, Stick::Left).is_err());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn same_directory_is_not_valid_as_independent_reference() {
        let dir = session("selfreference", 0.0);
        assert_eq!(
            Comparison::compute(&dir, Stick::Left, Some(&dir))
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn packet_reset_across_stages_is_rejected() {
        let dir = session("crosspacket", 0.0);
        let first = dir.join("first.csv");
        let s = fs::read_to_string(&first).unwrap();
        // Tamper a fully recorded stage; boundary logic must fail even if
        // recomputed reports themselves would otherwise agree.
        let raw = s.lines().collect::<Vec<_>>();
        let mut fixed = raw.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let last = fixed.last_mut().unwrap();
        let mut fields = last.split(',').map(str::to_owned).collect::<Vec<_>>();
        fields[2] = "99999".into();
        *last = fields.join(",");
        fs::write(&first, fixed.join("\n") + "\n").unwrap();
        assert!(SessionFacts::from_dir(&dir, Stick::Left).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
