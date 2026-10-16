//! Interactive read-only neutral evidence collection; no profile writes.
//! Two operator-confirmed released windows with a witnessed raw excursion
//! between them, using the existing guarded calibration state machine.
//! Results are published as a whole new directory only after successful
//! collection and offline audit. XInput slot is NOT a physical identity.
use crate::{
    capture::{self, Reader, XInputReader},
    guided::{GuidedWizard, Screen},
    neutral_audit,
    profile_store::Stick,
    xinput::Report,
};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static UNIQUE: AtomicU64 = AtomicU64::new(0);
const NEUTRAL_SAMPLES: usize = 128;
const EXERCISE_SAMPLES: usize = 400;
fn invalid(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}
fn stage_error(e: crate::guided::WizardError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("operator-guided neutral session rejected: {e:?}"),
    )
}
fn lost() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "XInput connection lost; entire neutral session discarded",
    )
}
/// A testable confirmation callback. In native mode the callback reads the
/// local console and refuses absent, partial, or incorrect declarations.
pub fn run_with<R: Reader, F: FnMut(&str, &str) -> io::Result<()>>(
    reader: &mut R,
    slot: u32,
    stick: Stick,
    period_ms: u64,
    output_dir: &Path,
    sleep: bool,
    confirm: &mut F,
) -> io::Result<neutral_audit::NeutralAudit> {
    if slot > 3 || !(1..=100).contains(&period_ms) {
        return Err(invalid("slot must be 0..3, period 1..100ms"));
    }
    // Probe before creating anything or requesting operator responses.
    let probe = reader.read()?.ok_or_else(lost)?;
    let mut observed = ContinuityReader {
        upstream: reader,
        previous: probe.packet,
    };
    if output_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "session output directory already exists; no overwrite permitted",
        ));
    }
    let stem = output_dir
        .file_name()
        .ok_or_else(|| invalid("output directory name required"))?;
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    let mut wizard = GuidedWizard::new();
    confirm("BASLAT", wizard.instruction())?;
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(
        ".{}.{}.{}.neutral-session-tmp",
        stem.to_string_lossy(),
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage)?;
    let result = (|| -> io::Result<neutral_audit::NeutralAudit> {
        wizard.confirm_release().map_err(stage_error)?;
        {
            let mut tee = GuidedReader {
                upstream: &mut observed,
                wizard: &mut wizard,
                stick,
            };
            capture::record(
                &mut tee,
                NEUTRAL_SAMPLES,
                period_ms,
                &stage.join("first.csv"),
                sleep,
            )?;
        }
        if wizard.screen() != Screen::AskMovement {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "first neutral window incomplete",
            ));
        }
        confirm("HAREKET", wizard.instruction())?;
        {
            let mut tee = GuidedReader {
                upstream: &mut observed,
                wizard: &mut wizard,
                stick,
            };
            capture::record(
                &mut tee,
                EXERCISE_SAMPLES,
                period_ms,
                &stage.join("exercise.csv"),
                sleep,
            )?;
        }
        wizard.confirm_movement_finished().map_err(stage_error)?;
        confirm("BIRAK", wizard.instruction())?;
        wizard.confirm_release().map_err(stage_error)?;
        {
            let mut tee = GuidedReader {
                upstream: &mut observed,
                wizard: &mut wizard,
                stick,
            };
            capture::record(
                &mut tee,
                NEUTRAL_SAMPLES,
                period_ms,
                &stage.join("second.csv"),
                sleep,
            )?;
        }
        if wizard.screen() != Screen::Review {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "second neutral window incomplete",
            ));
        }
        // Consent is separate from observational state. No code can infer it.
        confirm(
            "RAPOR",
            "Iki pencere tamamlandi. Salt okunur tanilama dosyalarini yayimla?",
        )?;
        // The state machine confirms that the raw excursion was present and
        // the user's two released windows passed its basic consistency gate.
        let _verified_candidate = wizard.commit().map_err(stage_error)?;
        let result =
            neutral_audit::analyze(&stage.join("first.csv"), &stage.join("second.csv"), stick)?;
        // Check final device presence before publishing, but do not claim
        // that this proves a stable hardware identity.
        let _ = observed.read()?.ok_or_else(lost)?;
        let movement =
            ExerciseEvidence::analyze(&stage.join("exercise.csv"), stick, result.first.median)?;
        if movement.max_excursion < 0.075 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recorded movement did not reach 0.075 excursion",
            ));
        }
        neutral_audit::save(&result, stick, &stage.join("neutral_audit.json"))?;
        fs::write(
            stage.join("exercise_audit.json"),
            movement.json(stick).as_bytes(),
        )?;
        // Atomic directory publication, nonreplacing on Windows.
        // Physical acceptance remains false even for a passing report.
        #[cfg(windows)]
        capture::publish_new(&stage, output_dir)?;
        #[cfg(not(windows))]
        {
            if output_dir.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "session output already exists",
                ));
            }
            fs::rename(&stage, output_dir)?;
        }
        Ok(result)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
struct GuidedReader<'a, R: Reader> {
    upstream: &'a mut R,
    wizard: &'a mut GuidedWizard,
    stick: Stick,
}
impl<R: Reader> Reader for GuidedReader<'_, R> {
    fn read(&mut self) -> io::Result<Option<Report>> {
        let report = self.upstream.read()?;
        if let Some(report) = report {
            self.wizard
                .sample(report.stick(self.stick))
                .map_err(stage_error)?;
        }
        Ok(report)
    }
}
/// XInput's packet sequence can reveal some reconnects but CANNOT prove
/// that the physical controller stayed the same. Refuse a decreasing counter
/// as an ambiguous reset (including extremely rare u32 wrap).
struct ContinuityReader<'a, R: Reader> {
    upstream: &'a mut R,
    previous: u32,
}
impl<R: Reader> Reader for ContinuityReader<'_, R> {
    fn read(&mut self) -> io::Result<Option<Report>> {
        let report = self.upstream.read()?;
        if let Some(report) = report {
            if report.packet < self.previous {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "XInput packet reset/wrap during session: physical continuity unverified",
                ));
            }
            self.previous = report.packet;
        }
        Ok(report)
    }
}
const EXERCISE_HEADER:&str="sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source";
#[derive(Debug)]
pub(crate) struct ExerciseEvidence {
    pub(crate) max_excursion: f64,
    pub(crate) min_x: f64,
    pub(crate) max_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_y: f64,
    pub(crate) repeated_packets: usize,
    pub(crate) changed_packets: usize,
    pub(crate) max_interval_us: u64,
}
impl ExerciseEvidence {
    pub(crate) fn analyze(path: &Path, stick: Stick, origin: crate::Vec2) -> io::Result<Self> {
        let file = fs::File::open(path)?;
        if file.metadata()?.len() > 130_000 {
            return Err(invalid("exercise capture too large"));
        }
        let mut lines = BufReader::new(file).lines();
        if lines.next().transpose()?.as_deref() != Some(EXERCISE_HEADER) {
            return Err(invalid("exercise capture header mismatch"));
        }
        let mut result = Self {
            max_excursion: 0.0,
            min_x: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            min_y: f64::INFINITY,
            max_y: f64::NEG_INFINITY,
            repeated_packets: 0,
            changed_packets: 0,
            max_interval_us: 0,
        };
        let mut prev_packet = None;
        let mut prev_time = None;
        for index in 0..EXERCISE_SAMPLES {
            let line = lines
                .next()
                .ok_or_else(|| invalid("exercise missing samples"))??;
            let c = line.split(',').collect::<Vec<_>>();
            if c.len() != 11
                || c[10] != "xinput_readonly"
                || c[0].parse::<usize>().ok() != Some(index)
            {
                return Err(invalid("exercise row schema/index mismatch"));
            }
            let time = c[1]
                .parse::<u64>()
                .map_err(|_| invalid("bad exercise time"))?;
            let packet = c[2]
                .parse::<u32>()
                .map_err(|_| invalid("bad exercise packet"))?;
            if let Some(prev) = prev_time {
                if time < prev || time - prev > 2_000_000 {
                    return Err(invalid("exercise timestamp discontinuity"));
                }
                result.max_interval_us = result.max_interval_us.max(time - prev);
            }
            if let Some(last) = prev_packet {
                if packet < last {
                    return Err(invalid("exercise packet reset"));
                }
                if packet == last {
                    result.repeated_packets += 1
                } else {
                    result.changed_packets += 1;
                }
            }
            let values = c[3..7]
                .iter()
                .map(|v| {
                    let v = v.parse::<f64>().map_err(|_| invalid("bad exercise axis"))?;
                    if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                        return Err(invalid("nonfinite/out-of-range exercise axis"));
                    }
                    Ok(v)
                })
                .collect::<io::Result<Vec<_>>>()?;
            c[7].parse::<u16>()
                .map_err(|_| invalid("bad exercise buttons"))?;
            c[8].parse::<u8>()
                .map_err(|_| invalid("bad exercise left trigger"))?;
            c[9].parse::<u8>()
                .map_err(|_| invalid("bad exercise right trigger"))?;
            let (x, y) = match stick {
                Stick::Left => (values[0], values[1]),
                Stick::Right => (values[2], values[3]),
            };
            result.min_x = result.min_x.min(x);
            result.max_x = result.max_x.max(x);
            result.min_y = result.min_y.min(y);
            result.max_y = result.max_y.max(y);
            result.max_excursion = result
                .max_excursion
                .max((crate::Vec2::new(x, y) - origin).norm());
            prev_packet = Some(packet);
            prev_time = Some(time);
        }
        if lines.next().is_some() {
            return Err(invalid("extra exercise samples"));
        }
        Ok(result)
    }
    pub(crate) fn json(&self, stick: Stick) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"read_only_exercise_window\",",
                "\"stick\":\"{}\",\"samples\":400,\"max_observed_excursion\":{:.10},",
                "\"observed_min\":[{:.10},{:.10}],\"observed_max\":[{:.10},{:.10}],",
                "\"repeated_packets\":{},\"changed_packets\":{},\"max_interval_us\":{},",
                "\"movement_is_operator_labeled\":true,",
                "\"physical_intent_verified\":false,\"hardware_identity_verified\":false,",
                "\"game_input_modified\":false,\"profile_created\":false,",
                "\"production_qualified\":false}}\n"
            ),
            stick.name(),
            self.max_excursion,
            self.min_x,
            self.min_y,
            self.max_x,
            self.max_y,
            self.repeated_packets,
            self.changed_packets,
            self.max_interval_us
        )
    }
}
pub fn native(slot: u32, stick: Stick, period_ms: u64, output_dir: &Path) -> io::Result<()> {
    if slot > 3 || !(1..=100).contains(&period_ms) {
        return Err(invalid("invalid slot or period"));
    }
    let mut device = XInputReader { slot };
    println!(
        "GuardAdaptive read-only neutral session: slot={slot} stick={}; NO profile creation.",
        stick.name()
    );
    println!("XInput slot is not a stable physical device identity. The two release declarations cannot be verified by software.");
    let mut accept = |token: &str, message: &str| -> io::Result<()> {
        print!("{message} Onay icin tam olarak {token} yaz: ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "local operator confirmation absent",
            ));
        }
        if line.trim() != token {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "operator did not approve current stage",
            ));
        }
        Ok(())
    };
    let result = run_with(
        &mut device,
        slot,
        stick,
        period_ms,
        output_dir,
        true,
        &mut accept,
    )?;
    println!(
        "NEUTRAL SESSION PUBLISHED: {} / first.csv, exercise.csv, second.csv, neutral_audit.json, exercise_audit.json",
        output_dir.display()
    );
    println!("OBSERVATIONAL_POLICY_PASS={} | PHYSICAL_RELEASE_VERIFIED=false | HARDWARE_IDENTITY_VERIFIED=false | PRODUCTION=BLOCKED",
        result.observational_policy_pass);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;
    struct Fake {
        reports: Vec<Option<Report>>,
        index: usize,
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            let result = self.reports.get(self.index).copied().flatten();
            self.index += 1;
            Ok(result)
        }
    }
    fn fixture(disconnect: Option<usize>, movement: bool, bad_second: bool) -> Fake {
        let mut values = Vec::new();
        let mut push = |x: f64, y: f64| {
            let i = values.len();
            values.push(if Some(i) == disconnect {
                None
            } else {
                Some(Report {
                    packet: (i / 2) as u32,
                    left: Vec2::new(x, y),
                    right: Vec2::new(0.0, 0.0),
                    buttons: 0,
                    left_trigger: 0,
                    right_trigger: 0,
                })
            });
        };
        push(0.1, -0.2); // initial non-consuming connectivity probe
        for i in 0..NEUTRAL_SAMPLES {
            let noise = if i % 2 == 0 { 0.00003 } else { -0.00003 };
            push(0.1 + noise, -0.2 - noise);
        }
        for _ in 0..EXERCISE_SAMPLES {
            push(if movement { 0.34 } else { 0.1 }, -0.2);
        }
        for i in 0..NEUTRAL_SAMPLES {
            let noise = if i % 2 == 0 { 0.00003 } else { -0.00003 };
            push(
                0.1004 + noise + if bad_second { 0.2 } else { 0.0 },
                -0.2001 - noise,
            );
        }
        push(0.1004, -0.2001); // final liveness probe
        Fake {
            reports: values,
            index: 0,
        }
    }
    fn dir(tag: &str) -> std::path::PathBuf {
        let id = UNIQUE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "guardadaptive-session-{tag}-{}-{id}",
            std::process::id()
        ))
    }
    fn approved() -> impl FnMut(&str, &str) -> io::Result<()> {
        |_, _| Ok(())
    }
    #[test]
    fn two_release_movement_consent_publishes_read_only_triplet() {
        let p = dir("success");
        let r = run_with(
            &mut fixture(None, true, false),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut approved(),
        )
        .unwrap();
        assert!(r.observational_policy_pass);
        for file in [
            "first.csv",
            "exercise.csv",
            "second.csv",
            "neutral_audit.json",
            "exercise_audit.json",
        ] {
            assert!(p.join(file).is_file());
        }
        assert_eq!(
            fs::read_to_string(p.join("exercise.csv"))
                .unwrap()
                .lines()
                .count(),
            401
        );
        let exercise = fs::read_to_string(p.join("exercise_audit.json")).unwrap();
        assert!(exercise.contains(r#""movement_is_operator_labeled":true"#));
        let json = fs::read_to_string(p.join("neutral_audit.json")).unwrap();
        assert!(json.contains("\"controller_profile_created\":false"));
        assert!(json.contains("\"physical_release_verified\":false"));
        assert!(json.contains("\"stable_hardware_identity_verified\":false"));
        assert_eq!(
            fs::read_to_string(p.join("first.csv"))
                .unwrap()
                .lines()
                .count(),
            129
        );
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn unavailable_device_has_no_prompt_and_no_files() {
        let p = dir("none");
        let mut called = 0;
        let e = run_with(
            &mut Fake {
                reports: Vec::new(),
                index: 0,
            },
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut |_, _| {
                called += 1;
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotConnected);
        assert_eq!(called, 0);
        assert!(!p.exists());
    }
    #[test]
    fn mid_capture_disconnect_discards_all_evidence() {
        let p = dir("disconnect");
        let e = run_with(
            &mut fixture(
                Some(1 + NEUTRAL_SAMPLES + EXERCISE_SAMPLES + 9),
                true,
                false,
            ),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut approved(),
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotConnected);
        assert!(!p.exists());
    }
    #[test]
    fn no_intentional_movement_is_rejected() {
        let p = dir("noexcursion");
        assert!(run_with(
            &mut fixture(None, false, false),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut approved()
        )
        .is_err());
        assert!(!p.exists());
    }
    #[test]
    fn inconsistent_second_window_is_rejected() {
        let p = dir("disagree");
        assert!(run_with(
            &mut fixture(None, true, true),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut approved()
        )
        .is_err());
        assert!(!p.exists());
    }
    #[test]
    fn missing_stage_approval_discards_entire_session() {
        let p = dir("noapproval");
        let mut tokens = Vec::new();
        let e = run_with(
            &mut fixture(None, true, false),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut |token, _| {
                tokens.push(token.to_owned());
                if token == "BIRAK" {
                    Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(tokens, ["BASLAT", "HAREKET", "BIRAK"]);
        assert!(!p.exists());
    }
    #[test]
    fn existing_report_is_not_replaced() {
        let p = dir("existing");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("keep.txt"), b"old evidence").unwrap();
        assert_eq!(
            run_with(
                &mut fixture(None, true, false),
                0,
                Stick::Left,
                8,
                &p,
                false,
                &mut approved()
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(p.join("keep.txt")).unwrap(), b"old evidence");
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn selected_stick_cannot_borrow_excursion_from_other_stick() {
        let p = dir("wrongstick");
        let e = run_with(
            &mut fixture(None, true, false),
            0,
            Stick::Right,
            8,
            &p,
            false,
            &mut approved(),
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(!p.exists());
    }
    #[test]
    fn operator_declining_report_publication_keeps_all_files_private() {
        let p = dir("declinepublish");
        let e = run_with(
            &mut fixture(None, true, false),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut |token, _| {
                if token == "RAPOR" {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "user cancelled",
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(!p.exists());
    }
    #[test]
    fn packet_counter_reset_cancels_without_publishing() {
        let p = dir("packetreset");
        let mut fake = fixture(None, true, false);
        fake.reports[1 + NEUTRAL_SAMPLES + 12]
            .as_mut()
            .unwrap()
            .packet = 0;
        let err = run_with(&mut fake, 0, Stick::Left, 8, &p, false, &mut approved()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("packet reset"));
        assert!(!p.exists());
    }
    #[test]
    fn exercise_evidence_parser_rejects_tampered_rows() {
        let p = dir("tamperedexercise");
        let _ = run_with(
            &mut fixture(None, true, false),
            0,
            Stick::Left,
            8,
            &p,
            false,
            &mut approved(),
        )
        .unwrap();
        let exercise = p.join("exercise.csv");
        let baseline = fs::read_to_string(&exercise).unwrap();
        let median = crate::Vec2::new(0.1, -0.2);
        let ok = ExerciseEvidence::analyze(&exercise, Stick::Left, median).unwrap();
        assert!(ok.max_excursion > 0.20);
        assert_eq!(
            ok.repeated_packets + ok.changed_packets,
            EXERCISE_SAMPLES - 1
        );
        fs::write(&exercise, baseline.replace("xinput_readonly", "spoofed")).unwrap();
        assert!(ExerciseEvidence::analyze(&exercise, Stick::Left, median).is_err());
        fs::write(&exercise, baseline.replace(",0.3400000000,", ",NaN,")).unwrap();
        assert!(ExerciseEvidence::analyze(&exercise, Stick::Left, median).is_err());
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn final_disconnect_discards_all_evidence() {
        let p = dir("finaldisconnect");
        assert_eq!(
            run_with(
                &mut fixture(
                    Some(1 + NEUTRAL_SAMPLES + EXERCISE_SAMPLES + NEUTRAL_SAMPLES),
                    true,
                    false
                ),
                0,
                Stick::Left,
                8,
                &p,
                false,
                &mut approved()
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotConnected
        );
        assert!(!p.exists());
    }
}
