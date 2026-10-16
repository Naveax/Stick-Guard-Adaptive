//! v0.26: completely offline shadow-output proof-of-interface.
//! This NEVER opens a device sink. It writes only a new trace file.
//! Conservative stale-report and uncertainty failsafes are simulated.
use crate::{
    measured_lab::{Dataset, Observation},
    profile_store::Stick,
    reacquire::VerifiedCenter,
    safety_preview::{PreviewMode, SafePreview},
};
use std::{fs, io, path::Path, time::Instant};
const MAX_ADJACENT_GAP_US: u64 = 50_000;
#[derive(Default)]
pub struct ShadowStats {
    pub total: u64,
    pub raw_passthrough: u64,
    pub simulated_corrected: u64,
    pub stale_gap_count: u64,
    pub total_processing_ns: u128,
    pub worst_processing_ns: u128,
}
impl ShadowStats {
    pub fn report_json(&self) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"offline_only_xinput_shadow\",",
                "\"total\":{},\"raw_passthrough\":{},",
                "\"simulated_corrected\":{},\"stale_gap_count\":{},",
                "\"total_core_processing_ns\":{},\"max_core_processing_ns\":{},",
                "\"no_real_time_scheduling_claim\":true,",
                "\"end_to_end_game_latency_measured\":false,",
                "\"physical_device_identity_verified\":false,",
                "\"writes_to_game_or_device\":false,",
                "\"hardware_acceptance\":\"UNVERIFIED\",",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.total,
            self.raw_passthrough,
            self.simulated_corrected,
            self.stale_gap_count,
            self.total_processing_ns,
            self.worst_processing_ns
        )
    }
}
pub struct ShadowTrace {
    pub csv: String,
    pub stats: ShadowStats,
}
fn append_phase(
    trace: &mut ShadowTrace,
    phase: &str,
    rows: &[Observation],
    processor: &mut Option<SafePreview>,
    force_raw: bool,
) {
    let mut previous = None;
    let mut stale = false;
    for (i, o) in rows.iter().enumerate() {
        if let Some(prev) = previous {
            if o.elapsed_us.saturating_sub(prev) > MAX_ADJACENT_GAP_US {
                stale = true;
                trace.stats.stale_gap_count += 1;
            }
        }
        previous = Some(o.elapsed_us);
        let start = Instant::now();
        let corrected = if force_raw || stale {
            o.raw
        } else if let Some(engine) = processor.as_mut() {
            engine.process(o.raw)
        } else {
            o.raw
        };
        let elapsed = start.elapsed().as_nanos();
        trace.stats.total_processing_ns += elapsed;
        trace.stats.worst_processing_ns = trace.stats.worst_processing_ns.max(elapsed);
        let mode = if force_raw {
            "first_window_raw"
        } else if stale {
            "stale_gap_raw_latched"
        } else if processor.is_none() {
            "quality_unverified_raw"
        } else {
            "offline_calibrated_preview"
        };
        if mode == "offline_calibrated_preview" {
            trace.stats.simulated_corrected += 1
        } else {
            trace.stats.raw_passthrough += 1;
        }
        trace.stats.total += 1;
        trace.csv.push_str(&format!(
            "{phase},{i},{},{},{:.10},{:.10},{:.10},{:.10},{mode},false\n",
            o.elapsed_us, o.packet, o.raw.x, o.raw.y, corrected.x, corrected.y
        ));
    }
}
pub fn simulate(data: &Dataset) -> ShadowTrace {
    let trusted = if data.audit.observational_policy_pass {
        VerifiedCenter::restore_checked(
            data.audit.first.median,
            data.audit.first.mad,
            data.audit.disagreement,
        )
    } else {
        None
    };
    let authorized = trusted
        .map(SafePreview::from_verified)
        .filter(|g| g.quality().mode == PreviewMode::Calibrated);
    let mut trace=ShadowTrace{
        csv:String::from("phase,index,elapsed_us,packet,raw_x,raw_y,shadow_x,shadow_y,mode,was_written_to_device\n"),
        stats:ShadowStats::default(),
    };
    let mut first = None;
    append_phase(&mut trace, "first_neutral", &data.first, &mut first, true);
    let mut exercise = trusted
        .map(SafePreview::from_verified)
        .filter(|g| g.quality().mode == PreviewMode::Calibrated);
    append_phase(&mut trace, "exercise", &data.exercise, &mut exercise, false);
    let mut holdout = authorized;
    append_phase(&mut trace, "holdout", &data.second, &mut holdout, false);
    trace
}
pub fn run(dir: &Path, stick: Stick, output_csv: &Path, output_json: &Path) -> io::Result<()> {
    if output_csv == output_json || output_csv.exists() || output_json.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "shadow output names must be distinct and both absent",
        ));
    }
    // Use a dedicated directory so CSV and report publication is atomic,
    // nonreplacing and does not leak partial finalized evidence.
    let parent = output_csv.parent().unwrap_or_else(|| Path::new("."));
    if parent != output_json.parent().unwrap_or_else(|| Path::new(".")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "shadow output CSV and JSON must share same parent",
        ));
    }
    // No candidate can be promoted by this non-live proof-of-interface.
    let data = Dataset::read(dir, stick)?;
    let trace = simulate(&data);
    if trace.stats.total != 656 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "shadow trace missing frames",
        ));
    }
    // Publish each file no-overwrite; rollback on second publication failure.
    // Source files are never changed.
    crate::truth_lab::save_new(output_csv, &trace.csv)?;
    if let Err(e) = crate::truth_lab::save_new(output_json, &trace.stats.report_json()) {
        let _ = fs::remove_file(output_csv);
        return Err(e);
    }
    println!(
        "OFFLINE SHADOW rows={} preview={} raw={} stale_gap={} core_max={}ns",
        trace.stats.total,
        trace.stats.simulated_corrected,
        trace.stats.raw_passthrough,
        trace.stats.stale_gap_count,
        trace.stats.worst_processing_ns
    );
    println!(
        "NO device writes, NO game injection, NO physical or end-to-end latency qualification."
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{capture::Reader, neutral_session, xinput::Report, Vec2};
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    struct Fake {
        index: usize,
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
                (0.1 + noise, -0.2 - noise)
            } else if i <= 528 {
                (0.34, -0.2)
            } else {
                (0.1004 + noise, -0.2001 - noise)
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
    fn source() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ga-v26-src-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        neutral_session::run_with(
            &mut Fake { index: 0 },
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
    fn offline_shadow_is_read_only_and_never_publishes_device_writes() {
        let p = source();
        let d = Dataset::read(&p, Stick::Left).unwrap();
        let t = simulate(&d);
        assert_eq!(t.stats.total, 656);
        assert_eq!(t.stats.raw_passthrough, 128);
        assert_eq!(t.stats.simulated_corrected, 528);
        assert!(t.csv.contains("first_neutral"));
        assert!(t.csv.contains(",false\n"));
        assert!(!t
            .stats
            .report_json()
            .contains("\"writes_to_game_or_device\":true"));
        assert_eq!(t.csv.lines().count(), 657);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn stale_input_latches_raw_within_phase_only() {
        let p = source();
        let mut d = Dataset::read(&p, Stick::Left).unwrap();
        // Input delay in the exercise phase cannot be silently corrected.
        for observation in d.exercise.iter_mut().skip(200) {
            observation.elapsed_us += 80_000;
        }
        let t = simulate(&d);
        assert!(t.stats.stale_gap_count >= 1);
        assert!(t.stats.raw_passthrough >= 328);
        assert!(t.csv.contains("stale_gap_raw_latched"));
        assert!(t.csv.contains("holdout,0,"));
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn uncertain_calibration_is_exact_raw_output() {
        let p = source();
        let mut d = Dataset::read(&p, Stick::Left).unwrap();
        d.audit.observational_policy_pass = false;
        let t = simulate(&d);
        assert_eq!(t.stats.simulated_corrected, 0);
        assert_eq!(t.stats.raw_passthrough, 656);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn shadow_publication_refuses_existing_evidence() {
        let p = source();
        let out = std::env::temp_dir().join(format!("ga-v26-{}.csv", std::process::id()));
        let json = out.with_extension("json");
        let _ = fs::remove_file(&out);
        let _ = fs::remove_file(&json);
        run(&p, Stick::Left, &out, &json).unwrap();
        assert!(run(&p, Stick::Left, &out, &json).is_err());
        let bytes = fs::read(&out).unwrap();
        assert!(bytes.len() > 1000);
        assert!(fs::read_to_string(&json)
            .unwrap()
            .contains("\"production_gate\":\"BLOCKED\""));
        let _ = fs::remove_file(out);
        let _ = fs::remove_file(json);
        let _ = fs::remove_dir_all(p);
    }
}
