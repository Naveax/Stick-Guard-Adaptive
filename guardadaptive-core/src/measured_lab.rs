//! v0.22 dataset staging + v0.23 paired offline model comparison.
//! Observed reports are not truth-labeled player intent. Nothing here can
//! qualify a controller or influence live/game input.
use crate::{
    capture,
    causal_guard::{CausalGuard, GuardConfig},
    confidence_direction::ConfidenceDirection,
    direction_guard::DirectionGuard,
    hybrid::{HybridConfig, HybridGuard},
    motion_first::{MotionFirst, MotionFirstConfig},
    neutral_audit::{self, NeutralAudit},
    profile_store::Stick,
    reacquire::VerifiedCenter,
    resolution::Resolution,
    session_replay::SessionFacts,
    InputMode, Vec2,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const RAW_HEADER:&str="sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source";
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub elapsed_us: u64,
    pub packet: u32,
    pub raw: Vec2,
}
pub struct Dataset {
    pub first: Vec<Observation>,
    pub exercise: Vec<Observation>,
    pub second: Vec<Observation>,
    pub audit: NeutralAudit,
    pub facts: SessionFacts,
}
fn window(path: &Path, stick: Stick, n: usize) -> io::Result<Vec<Observation>> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 150_000 {
        return Err(invalid("stage oversized"));
    }
    let mut lines = BufReader::new(file).lines();
    if lines.next().transpose()?.as_deref() != Some(RAW_HEADER) {
        return Err(invalid("raw schema mismatch"));
    }
    let mut rows = Vec::with_capacity(n);
    let mut previous_time = None;
    let mut previous_packet = None;
    for index in 0..n {
        let line = lines.next().ok_or_else(|| invalid("short raw stage"))??;
        let c = line.split(',').collect::<Vec<_>>();
        if c.len() != 11 || c[0].parse::<usize>().ok() != Some(index) || c[10] != "xinput_readonly"
        {
            return Err(invalid("invalid raw stage row"));
        }
        let t = c[1]
            .parse::<u64>()
            .map_err(|_| invalid("invalid raw timestamp"))?;
        let packet = c[2].parse::<u32>().map_err(|_| invalid("invalid packet"))?;
        if let Some(prev) = previous_time {
            if t < prev || t - prev > 2_000_000 {
                return Err(invalid("timing reset/gap"));
            }
        }
        if let Some(prev) = previous_packet {
            if packet < prev {
                return Err(invalid("packet reset"));
            }
        }
        let axes = c[3..7]
            .iter()
            .map(|v| {
                let x = v.parse::<f64>().map_err(|_| invalid("invalid axis"))?;
                if !x.is_finite() || !(-1.0..=1.0).contains(&x) {
                    return Err(invalid("axis outside normalized range"));
                }
                Ok(x)
            })
            .collect::<io::Result<Vec<_>>>()?;
        c[7].parse::<u16>()
            .map_err(|_| invalid("invalid buttons"))?;
        c[8].parse::<u8>().map_err(|_| invalid("invalid trigger"))?;
        c[9].parse::<u8>().map_err(|_| invalid("invalid trigger"))?;
        let raw = match stick {
            Stick::Left => Vec2::new(axes[0], axes[1]),
            Stick::Right => Vec2::new(axes[2], axes[3]),
        };
        rows.push(Observation {
            elapsed_us: t,
            packet,
            raw,
        });
        previous_time = Some(t);
        previous_packet = Some(packet);
    }
    if lines.next().is_some() {
        return Err(invalid("extra raw records"));
    }
    Ok(rows)
}
impl Dataset {
    pub fn read(dir: &Path, stick: Stick) -> io::Result<Self> {
        // v0.21 independently verifies all 5 raw/audit files first.
        let facts = SessionFacts::from_dir(dir, stick)?;
        let audit = neutral_audit::analyze(&dir.join("first.csv"), &dir.join("second.csv"), stick)?;
        let first = window(&dir.join("first.csv"), stick, 128)?;
        let exercise = window(&dir.join("exercise.csv"), stick, 400)?;
        let second = window(&dir.join("second.csv"), stick, 128)?;
        if first[127].packet > exercise[0].packet || exercise[399].packet > second[0].packet {
            return Err(invalid("cross-stage packet continuity broken"));
        }
        Ok(Self {
            first,
            exercise,
            second,
            audit,
            facts,
        })
    }
    fn csv(&self) -> String {
        let mut s=String::from("split,label_source,index,elapsed_us,packet,raw_x,raw_y,ground_truth_x,ground_truth_y\n");
        for (label, rows) in [
            ("calibration_operator_declared_neutral", &self.first),
            ("exercise_intent_unknown", &self.exercise),
            ("holdout_operator_declared_neutral", &self.second),
        ] {
            for (i, o) in rows.iter().enumerate() {
                s.push_str(&format!("{label},operator_label_not_ground_truth,{i},{},{},{:.10},{:.10},UNKNOWN,UNKNOWN\n",
                    o.elapsed_us,o.packet,o.raw.x,o.raw.y));
            }
        }
        s
    }
    fn manifest(&self, stick: Stick) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"verified_five_file_xinput_session\",",
                "\"stick\":\"{}\",\"calibration_count\":128,\"exercise_count\":400,",
                "\"holdout_count\":128,\"cross_session_holdout_available\":false,",
                "\"holdout_is_within_same_session\":true,",
                "\"calibration_uses_only_first_window\":true,",
                "\"observational_policy_pass\":{},",
                "\"operator_labeled_neutral_not_truth\":true,",
                "\"exercise_desired_motion_known\":false,",
                "\"ground_truth_motion_known\":false,",
                "\"hardware_identity_verified\":false,\"production_qualified\":false}}\n"
            ),
            stick.name(),
            self.audit.observational_policy_pass
        )
    }
    pub fn publish(&self, stick: Stick, dest: &Path) -> io::Result<()> {
        if dest.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "dataset exists",
            ));
        }
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let name = dest
            .file_name()
            .ok_or_else(|| invalid("destination name missing"))?;
        let stage = parent.join(format!(
            ".{}.{}.{}.dataset-tmp",
            name.to_string_lossy(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&stage)?;
        let result = (|| -> io::Result<()> {
            for (file, text) in [
                ("observations.csv", self.csv()),
                ("manifest.json", self.manifest(stick)),
            ] {
                let path = stage.join(file);
                let mut f = OpenOptions::new().create_new(true).write(true).open(path)?;
                f.write_all(text.as_bytes())?;
                f.sync_all()?;
            }
            // No partial dataset visible under final name.
            #[cfg(windows)]
            {
                capture::publish_new(&stage, dest)?;
            }
            #[cfg(not(windows))]
            {
                if dest.exists() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "dataset exists",
                    ));
                }
                fs::rename(&stage, dest)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }
}
#[derive(Default, Debug)]
pub struct ProxyScore {
    pub holdout_neutral_rmse: f64,
    pub exercise_delta_rms_vs_center_only: f64,
    pub exercise_micro_suppressed: u64,
    pub exercise_opposite_vs_center_only: u64,
    pub exercise_samples: u64,
    pub holdout_samples: u64,
    pub core_compute_ns: u128,
}
impl ProxyScore {
    fn json(&self) -> String {
        format!(
            concat!(
                "{{\"holdout_neutral_rmse\":{:.10},",
                "\"exercise_delta_rms_vs_center_only\":{:.10},",
                "\"exercise_micro_suppressed_vs_observed\":{},",
                "\"exercise_opposite_vs_center_only\":{},",
                "\"exercise_samples\":{},\"holdout_samples\":{},",
                "\"core_only_processing_ns\":{}}}"
            ),
            self.holdout_neutral_rmse,
            self.exercise_delta_rms_vs_center_only,
            self.exercise_micro_suppressed,
            self.exercise_opposite_vs_center_only,
            self.exercise_samples,
            self.holdout_samples,
            self.core_compute_ns
        )
    }
}
enum Engine {
    CenterOnly,
    MotionFirst(MotionFirst),
    Hybrid(HybridGuard),
    Direction(DirectionGuard),
    Confidence(ConfidenceDirection),
    Causal(CausalGuard),
}
impl Engine {
    fn process(&mut self, raw: Vec2, center: Vec2) -> Vec2 {
        match self {
            Engine::CenterOnly => (raw - center).saturate(),
            Engine::MotionFirst(g) => g.process(raw, InputMode::Live),
            Engine::Hybrid(g) => g.process(raw),
            Engine::Direction(g) => g.process(raw),
            Engine::Confidence(g) => g.process(raw),
            Engine::Causal(g) => g.process(raw),
        }
    }
}
fn candidates(verified: VerifiedCenter) -> Vec<(&'static str, Engine)> {
    let lsb = Resolution::XInput16.positive_lsb();
    let mut motion = MotionFirst::new(MotionFirstConfig {
        input_lsb: lsb,
        softness_lsb: 0.5,
    });
    motion.accept_reacquisition(verified);
    let cfg = HybridConfig {
        report_lsb: lsb,
        ..HybridConfig::default()
    };
    let mut hybrid = HybridGuard::new(cfg);
    hybrid.accept_verified(verified);
    let mut direction = DirectionGuard::new(cfg);
    direction.accept_verified(verified);
    let mut confidence = ConfidenceDirection::new(cfg);
    confidence.accept_verified(verified);
    let mut causal = CausalGuard::new(GuardConfig::default());
    causal.accept_verified(verified);
    vec![
        ("center_only", Engine::CenterOnly),
        ("motion_first", Engine::MotionFirst(motion)),
        ("hybrid", Engine::Hybrid(hybrid)),
        ("direction_guard", Engine::Direction(direction)),
        (
            "confidence_direction_rejected_v015",
            Engine::Confidence(confidence),
        ),
        ("causal_bounded_v028_research_only", Engine::Causal(causal)),
    ]
}
fn score(dataset: &Dataset, engine: &mut Engine) -> ProxyScore {
    let center = dataset.audit.first.median;
    let mut result = ProxyScore::default();
    let mut neutral_error = 0.0;
    let mut motion_deviation = 0.0;
    // Warming from only first labeled neutral; second NEVER calibrates.
    for x in &dataset.first {
        engine.process(x.raw, center);
    }
    for x in &dataset.exercise {
        let start = Instant::now();
        let out = engine.process(x.raw, center);
        result.core_compute_ns += start.elapsed().as_nanos();
        let baseline = (x.raw - center).saturate();
        motion_deviation += (out - baseline).length_squared();
        let radius = baseline.norm();
        if radius >= Resolution::XInput16.positive_lsb()
            && radius <= 0.03
            && out.norm() < 0.5 * radius
        {
            result.exercise_micro_suppressed += 1;
        }
        if radius > 0.0 && out.norm() > 0.0 && (out.x * baseline.x + out.y * baseline.y) < 0.0 {
            result.exercise_opposite_vs_center_only += 1;
        }
        result.exercise_samples += 1;
    }
    for x in &dataset.second {
        let start = Instant::now();
        let out = engine.process(x.raw, center);
        result.core_compute_ns += start.elapsed().as_nanos();
        neutral_error += out.length_squared();
        result.holdout_samples += 1;
    }
    result.holdout_neutral_rmse = (neutral_error / result.holdout_samples as f64).sqrt();
    result.exercise_delta_rms_vs_center_only =
        (motion_deviation / result.exercise_samples as f64).sqrt();
    result
}
pub struct Benchmark {
    scores: Vec<(&'static str, ProxyScore)>,
    pub eligible: bool,
    first_center: Vec2,
    second_center: Vec2,
}
impl Benchmark {
    pub fn evaluate(data: &Dataset) -> Self {
        // Quality fail-closed: an operator-labeled recording can still be
        // physically inaccurate; a bad one MUST NOT train comparison models.
        let verified = if data.audit.observational_policy_pass {
            VerifiedCenter::restore_checked(
                data.audit.first.median,
                data.audit.first.mad,
                data.audit.disagreement,
            )
        } else {
            None
        };
        let Some(v) = verified else {
            return Self {
                scores: Vec::new(),
                eligible: false,
                first_center: data.audit.first.median,
                second_center: data.audit.second.median,
            };
        };
        let scores = candidates(v)
            .into_iter()
            .map(|(name, mut e)| (name, score(data, &mut e)))
            .collect();
        Self {
            scores,
            eligible: true,
            first_center: data.audit.first.median,
            second_center: data.audit.second.median,
        }
    }
    pub fn json(&self, stick: Stick) -> String {
        let data = self
            .scores
            .iter()
            .map(|(n, s)| format!("\"{n}\":{}", s.json()))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"stick\":\"{}\",",
                "\"source\":\"paired_operator_labeled_offline_xinput_proxies\",",
                "\"eligible_for_offline_comparison\":{},",
                "\"model_count\":{},",
                "\"first_window_training_center\":[{:.10},{:.10}],",
                "\"second_window_holdout_median\":[{:.10},{:.10}],",
                "\"models\":{{{}}},",
                "\"holdout_labels_are_operator_declarations\":true,",
                "\"exercise_player_intent_is_unknown\":true,",
                "\"true_micro_fidelity_measurable\":false,",
                "\"true_wrong_direction_measurable\":false,",
                "\"core_time_is_not_game_latency\":true,",
                "\"auto_model_selection_allowed\":false,",
                "\"production_quality\":\"BLOCKED\"}}\n"
            ),
            stick.name(),
            self.eligible,
            self.scores.len(),
            self.first_center.x,
            self.first_center.y,
            self.second_center.x,
            self.second_center.y,
            data
        )
    }
}
fn save_new(path: &Path, text: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("missing output basename"))?;
    let temp = parent.join(format!(
        ".{}.{}.{}.lab-tmp",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created = false;
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        created = true;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        capture::publish_new(&temp, path)
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn dataset_command(source: &Path, stick: Stick, dest: &Path) -> io::Result<()> {
    let dataset = Dataset::read(source, stick)?;
    dataset.publish(stick, dest)?;
    println!("DATASET EXPORTED: {} rows, first=128 training / exercise=400 unlabeled / second=128 operator-labeled holdout",656);
    println!(
        "WARNING: no true intent, stable device identity or physical acceptance. {}",
        dest.display()
    );
    Ok(())
}
pub fn benchmark_command(
    source: &Path,
    stick: Stick,
    reference: Option<&Path>,
    dest: &Path,
) -> io::Result<()> {
    let dataset = Dataset::read(source, stick)?;
    let result = Benchmark::evaluate(&dataset);
    let output = if let Some(other) = reference {
        if fs::canonicalize(source)? == fs::canonicalize(other)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot compare a session with itself",
            ));
        }
        let other_data = Dataset::read(other, stick)?;
        let baseline = Benchmark::evaluate(&other_data);
        let deltas = result
            .scores
            .iter()
            .filter_map(|(name, target)| {
                baseline
                    .scores
                    .iter()
                    .find(|(old_name, _)| old_name == name)
                    .map(|(_, old)| {
                        format!(
                            concat!(
                                "\"{}\":{{\"neutral_proxy_rmse_delta\":{:.10},",
                                "\"exercise_alteration_rms_delta\":{:.10},",
                                "\"micro_suppression_proxy_delta\":{}}}"
                            ),
                            name,
                            target.holdout_neutral_rmse - old.holdout_neutral_rmse,
                            target.exercise_delta_rms_vs_center_only
                                - old.exercise_delta_rms_vs_center_only,
                            target.exercise_micro_suppressed as i64
                                - old.exercise_micro_suppressed as i64
                        )
                    })
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"comparison_type\":\"two_operator_labeled_sessions\",",
                "\"target\":{},\"reference\":{},\"candidate_deltas\":{{{}}},",
                "\"physical_device_match_unverified\":true,",
                "\"independent_ground_truth_motion_available\":false,",
                "\"automatic_promotion_allowed\":false,\"production_quality\":\"BLOCKED\"}}\n"
            ),
            result.json(stick).trim(),
            baseline.json(stick).trim(),
            deltas
        )
    } else {
        result.json(stick)
    };
    save_new(dest, &output)?;
    println!("BENCHMARK: compared={} candidates={} (first window only calibration, second operator holdout)",
        result.eligible,result.scores.len());
    for (name, s) in &result.scores {
        println!("{}: holdout_neutral_proxy_rmse={:.8} exercise_change_rms={:.8} micro_suppression_proxy={} opposite_to_observed_centered={}",
            name,s.holdout_neutral_rmse,s.exercise_delta_rms_vs_center_only,
            s.exercise_micro_suppressed,s.exercise_opposite_vs_center_only);
    }
    println!(
        "NO GROUND TRUTH PLAYER INTENT: auto-selection disabled, production BLOCKED. {}",
        dest.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{capture::Reader, neutral_session, xinput::Report};
    struct Fake {
        index: usize,
        bias: f64,
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
            let x = if i <= 128 {
                0.1 + noise
            } else if i <= 168 {
                0.1002 + noise
            } else if i <= 220 {
                0.0995 + noise
            } else if i <= 528 {
                0.34
            } else {
                0.1004 + noise
            };
            let y = if i <= 128 {
                -0.2 - noise
            } else if i <= 528 {
                -0.2
            } else {
                -0.2001 - noise
            };
            Ok(Some(Report {
                packet: (i / 2) as u32,
                left: Vec2::new(x + self.bias, y),
                right: Vec2::ZERO,
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
            }))
        }
    }
    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "guardadaptive-v22-v23-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn session() -> std::path::PathBuf {
        let p = path("source");
        neutral_session::run_with(
            &mut Fake {
                index: 0,
                bias: 0.0,
            },
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
    fn synthetic_session_dataset_and_model_comparison_are_isolated() {
        let src = session();
        let data_path = path("dataset");
        let result_path = path("compare.json");
        let d = Dataset::read(&src, Stick::Left).unwrap();
        assert_eq!(
            (d.first.len(), d.exercise.len(), d.second.len()),
            (128, 400, 128)
        );
        assert!(d.audit.observational_policy_pass);
        d.publish(Stick::Left, &data_path).unwrap();
        assert_eq!(
            fs::read_to_string(data_path.join("observations.csv"))
                .unwrap()
                .lines()
                .count(),
            657
        );
        assert!(fs::read_to_string(data_path.join("manifest.json"))
            .unwrap()
            .contains("\"ground_truth_motion_known\":false"));
        let r = Benchmark::evaluate(&d);
        assert!(r.eligible);
        assert_eq!(r.scores.len(), 6);
        let j = r.json(Stick::Left);
        assert!(j.contains("\"auto_model_selection_allowed\":false"));
        assert!(j.contains("\"production_quality\":\"BLOCKED\""));
        assert!(r
            .scores
            .iter()
            .all(|(_, s)| s.holdout_samples == 128 && s.exercise_samples == 400));
        save_new(&result_path, &j).unwrap();
        assert_eq!(fs::read(&result_path).unwrap(), j.as_bytes());
        assert!(save_new(&result_path, &j).is_err());
        assert!(d.publish(Stick::Left, &data_path).is_err());
        let _ = fs::remove_file(result_path);
        let _ = fs::remove_dir_all(data_path);
        let _ = fs::remove_dir_all(src);
    }
    #[test]
    fn cross_session_comparison_requires_distinct_valid_sources() {
        let target = session();
        let reference = session();
        let report = path("two-session.json");
        benchmark_command(&target, Stick::Left, Some(&reference), &report).unwrap();
        let payload = fs::read_to_string(&report).unwrap();
        assert!(payload.contains("\"comparison_type\":\"two_operator_labeled_sessions\""));
        assert!(payload.contains("\"physical_device_match_unverified\":true"));
        assert!(payload.contains("\"automatic_promotion_allowed\":false"));
        assert!(payload.contains("\"candidate_deltas\":{"));
        let duplicate = path("invalid-ref.json");
        assert!(benchmark_command(&target, Stick::Left, Some(&target), &duplicate).is_err());
        assert!(!duplicate.exists());
        fs::write(reference.join("exercise_audit.json"), b"altered").unwrap();
        assert!(benchmark_command(&target, Stick::Left, Some(&reference), &duplicate).is_err());
        assert!(!duplicate.exists());
        let _ = fs::remove_file(report);
        let _ = fs::remove_dir_all(target);
        let _ = fs::remove_dir_all(reference);
    }
    #[test]
    fn bad_second_window_fails_replay_and_publishes_nothing() {
        let src = session();
        let out = path("unpublished");
        fs::write(src.join("neutral_audit.json"), b"{}").unwrap();
        assert!(dataset_command(&src, Stick::Left, &out).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(src);
    }
    #[test]
    fn invalid_observational_quality_blocks_candidate_evaluation() {
        let src = session();
        let mut d = Dataset::read(&src, Stick::Left).unwrap();
        d.audit.observational_policy_pass = false;
        let b = Benchmark::evaluate(&d);
        assert!(!b.eligible);
        assert!(b.scores.is_empty());
        assert!(b.json(Stick::Left).contains("\"model_count\":0"));
        let _ = fs::remove_dir_all(src);
    }
    #[test]
    fn second_window_is_not_used_for_model_training() {
        let src = session();
        let mut d = Dataset::read(&src, Stick::Left).unwrap();
        let first = Benchmark::evaluate(&d);
        let old_center = first.first_center;
        for sample in &mut d.second {
            sample.raw.x += 0.0001;
        }
        d.audit.second.median.x += 0.0001;
        let changed = Benchmark::evaluate(&d);
        assert_eq!(old_center, changed.first_center);
        assert_eq!(
            first.scores[0].1.exercise_delta_rms_vs_center_only,
            changed.scores[0].1.exercise_delta_rms_vs_center_only
        );
        let _ = fs::remove_dir_all(src);
    }
}
