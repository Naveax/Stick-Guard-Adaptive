//! v0.27 integrated twelve-workstream delivery.
//! Hardware/gaming acceptance is impossible to assert from simulated reports.
//! This module *never* exposes a device-writing backend: it only accepts
//! replayed frames or in-memory synthetic reports and publishes offline evidence.
use crate::{
    acceptance::Matrix, measured_lab::Dataset, profile_store::Stick, resolution::Resolution,
    truth_lab, Vec2,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static TEMP: AtomicU64 = AtomicU64::new(0);
const STALE_US: u64 = 50_000;
const TRUTH_HEADER: &str = "index,elapsed_us,intended_x,intended_y,reference_source";
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    OfflinePreview,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Off,
    Unverified,
    Stale,
    PacketReset,
    OfflinePreview,
}
impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off_raw",
            Self::Unverified => "no_verified_neutral_raw",
            Self::Stale => "stale_raw_latched",
            Self::PacketReset => "packet_reset_raw_latched",
            Self::OfflinePreview => "offline_preview_no_device_write",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub timestamp_us: u64,
    pub packet: u32,
    pub raw: Vec2,
}
#[derive(Clone, Copy, Debug)]
pub struct Decision {
    pub output: Vec2,
    pub reason: Reason,
    pub would_write_to_device: bool,
}
/// No access to a driver, kernel, emulator, or output service.
/// A caller cannot enable physical game output by toggling the mode.
pub struct DryRunBridge {
    mode: Mode,
    center: Option<Vec2>,
    last: Option<(u64, u32)>,
    sticky_fault: Option<Reason>,
}
impl DryRunBridge {
    pub fn new(center: Option<Vec2>) -> Self {
        Self {
            mode: Mode::Off,
            center: center.filter(|c| c.finite() && c.x.abs() <= 1.0 && c.y.abs() <= 1.0),
            last: None,
            sticky_fault: None,
        }
    }
    pub fn set_mode(&mut self, m: Mode) {
        self.mode = m;
    }
    pub fn emergency_off(&mut self) {
        self.mode = Mode::Off;
        self.center = None;
        self.sticky_fault = None;
        self.last = None;
    }
    pub fn reconnect(&mut self) {
        // A reused XInput slot must NEVER carry an old correction.
        self.emergency_off();
    }
    pub fn device_write(&mut self, _sample: Vec2) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "No virtual gamepad driver or game-output implementation exists; dry-run only",
        ))
    }
    pub fn process(&mut self, s: Sample) -> Decision {
        let safe = if s.raw.finite() {
            s.raw.clamp()
        } else {
            Vec2::ZERO
        };
        if !s.raw.finite() || s.raw.x.abs() > 1.0 || s.raw.y.abs() > 1.0 {
            self.sticky_fault = Some(Reason::Unverified);
        }
        if let Some((when, last_packet)) = self.last {
            if s.packet < last_packet {
                self.sticky_fault = Some(Reason::PacketReset);
            }
            if s.timestamp_us < when || s.timestamp_us - when > STALE_US {
                self.sticky_fault = Some(Reason::Stale);
            }
        }
        self.last = Some((s.timestamp_us, s.packet));
        let reason = if self.mode == Mode::Off {
            Reason::Off
        } else if let Some(fault) = self.sticky_fault {
            fault
        } else if self.center.is_none() {
            Reason::Unverified
        } else {
            Reason::OfflinePreview
        };
        let output = if reason == Reason::OfflinePreview {
            (safe - self.center.unwrap_or(Vec2::ZERO)).saturate()
        } else {
            safe
        };
        Decision {
            output,
            reason,
            would_write_to_device: false,
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct SoakSummary {
    pub frames: u64,
    pub off: u64,
    pub offline_preview: u64,
    pub stale: u64,
    pub packet_reset: u64,
    pub unsafe_device_writes: u64,
    pub nonfinite_outputs: u64,
    pub core_max_ns: u128,
    pub core_total_ns: u128,
    pub connection_resets: u64,
    pub kill_switch_events: u64,
}
impl SoakSummary {
    fn accumulate(&mut self, decision: Decision, ns: u128) {
        self.frames += 1;
        self.core_max_ns = self.core_max_ns.max(ns);
        self.core_total_ns += ns;
        self.unsafe_device_writes += u64::from(decision.would_write_to_device);
        self.nonfinite_outputs += u64::from(!decision.output.finite());
        match decision.reason {
            Reason::Off | Reason::Unverified => self.off += 1,
            Reason::OfflinePreview => self.offline_preview += 1,
            Reason::Stale => self.stale += 1,
            Reason::PacketReset => self.packet_reset += 1,
        }
    }
    pub fn json(&self) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"synthetic_offline_bridge_soak\",",
                "\"frames\":{},\"raw_or_unverified\":{},\"preview\":{},",
                "\"stale_fallback\":{},\"packet_reset_fallback\":{},",
                "\"simulated_disconnect_resets\":{},\"emergency_off_events\":{},",
                "\"unsafe_device_writes\":{},\"nonfinite_outputs\":{},",
                "\"max_core_processing_ns\":{},\"total_core_processing_ns\":{},",
                "\"input_to_game_latency_measured\":false,",
                "\"real_game_test_completed\":false,",
                "\"driver_installed_or_activated\":false,",
                "\"production_qualified\":false}}\n"
            ),
            self.frames,
            self.off,
            self.offline_preview,
            self.stale,
            self.packet_reset,
            self.connection_resets,
            self.kill_switch_events,
            self.unsafe_device_writes,
            self.nonfinite_outputs,
            self.core_max_ns,
            self.core_total_ns
        )
    }
}
pub fn soak(seed: u64, frames: usize) -> io::Result<SoakSummary> {
    if !(10_000..=1_000_000).contains(&frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "soak count must be 10000..1000000",
        ));
    }
    let mut bridge = DryRunBridge::new(Some(Vec2::new(0.02, -0.01)));
    let mut report = SoakSummary::default();
    let mut now = 0_u64;
    let mut packet = 0_u32;
    let mut state = seed | 1;
    for i in 0..frames {
        // Repeatable simulated faults; never presented as live hardware data.
        if i % 1999 == 0 {
            bridge.reconnect();
            report.connection_resets += 1;
        }
        if i % 1999 == 2 {
            bridge = DryRunBridge::new(Some(Vec2::new(0.02, -0.01)));
            bridge.set_mode(Mode::OfflinePreview);
        }
        if i % 3971 == 0 {
            bridge.emergency_off();
            report.kill_switch_events += 1;
        }
        if i % 3971 == 1 {
            bridge.set_mode(Mode::OfflinePreview);
        }
        now = now.saturating_add(if i % 811 == 0 { 80_000 } else { 8_000 });
        packet = if i % 1361 == 0 {
            packet.wrapping_sub(200)
        } else if i % 3 == 0 {
            packet.wrapping_add(1)
        } else {
            packet
        };
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let n = state.wrapping_mul(0x2545F4914F6CDD1D);
        let axis = ((n >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
        let raw = Resolution::XInput16.quantize(Vec2::new(axis, -0.3 * axis));
        let tick = Instant::now();
        let decision = bridge.process(Sample {
            timestamp_us: now,
            packet,
            raw,
        });
        report.accumulate(decision, tick.elapsed().as_nanos());
        if decision.would_write_to_device {
            return Err(invalid("offline safety invariant: unexpected device write"));
        }
    }
    Ok(report)
}
#[derive(Clone, Copy, Default)]
pub struct ReferenceFacts {
    pub observations: u64,
    pub desired_known_but_unverified: u64,
    pub observed_vs_declared_rmse: f64,
    pub micro_wrong_vs_claimed: u64,
}
/// Independent reference CSV is a caller-declared file, *not* authenticated
/// sensor or camera provenance. It is never used for motor calibration.
pub fn external_reference(path: &Path, dataset: &Dataset) -> io::Result<ReferenceFacts> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 72_000 {
        return Err(invalid("reference file too large"));
    }
    let mut lines = BufReader::new(file).lines();
    if lines.next().transpose()?.as_deref() != Some(TRUTH_HEADER) {
        return Err(invalid("unexpected external reference CSV header"));
    }
    let mut sum = 0.0;
    let mut wrong = 0;
    for (i, observed) in dataset.exercise.iter().enumerate() {
        let line = lines
            .next()
            .ok_or_else(|| invalid("missing external reference rows"))??;
        let cols = line.split(',').collect::<Vec<_>>();
        if cols.len() != 5
            || cols[0].parse::<usize>().ok() != Some(i)
            || cols[4] != "external_instrument_operator_declared"
        {
            return Err(invalid("external reference source/index invalid"));
        }
        let t = cols[1]
            .parse::<u64>()
            .map_err(|_| invalid("bad reference time"))?;
        if t.abs_diff(observed.elapsed_us) > 8_000 {
            return Err(invalid("reference not aligned within 8ms"));
        }
        let x = cols[2]
            .parse::<f64>()
            .map_err(|_| invalid("bad reference x"))?;
        let y = cols[3]
            .parse::<f64>()
            .map_err(|_| invalid("bad reference y"))?;
        let desired = Vec2::new(x, y);
        if !desired.finite() || x.abs() > 1.0 || y.abs() > 1.0 {
            return Err(invalid("invalid reference axis"));
        }
        sum += ((observed.raw - dataset.audit.first.median).saturate() - desired).length_squared();
        if desired.norm() > Resolution::XInput16.positive_lsb()
            && (observed.raw - dataset.audit.first.median).x * desired.x
                + (observed.raw - dataset.audit.first.median).y * desired.y
                < 0.0
        {
            wrong += 1;
        }
    }
    if lines.next().is_some() {
        return Err(invalid("extra external reference rows"));
    }
    Ok(ReferenceFacts {
        observations: 400,
        desired_known_but_unverified: 400,
        observed_vs_declared_rmse: (sum / 400.0).sqrt(),
        micro_wrong_vs_claimed: wrong,
    })
}
pub const CHECKS: [&str; 12] = [
    "real_controller_capture",
    "independent_motion_reference",
    "severe_drift_algorithm",
    "micro_motion_fidelity",
    "wrong_direction_and_reversals",
    "time_varying_drift",
    "end_to_end_game_latency",
    "stable_physical_device_identity",
    "virtual_controller_sink",
    "toggle_and_raw_failsafe",
    "real_rocket_league_and_other_game_tests",
    "extended_stability_and_release_approval",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    VerifiedSoftware,
    ResearchOnly,
    BlockedPhysical,
}
impl Gate {
    pub fn name(self) -> &'static str {
        match self {
            Self::VerifiedSoftware => "SOFTWARE_PASS_ONLY",
            Self::ResearchOnly => "RESEARCH_INCOMPLETE",
            Self::BlockedPhysical => "BLOCKED_REAL_WORLD_EVIDENCE",
        }
    }
}
pub struct Suite {
    pub truth: truth_lab::ResultSet,
    pub matrix: Matrix,
    pub long: SoakSummary,
    pub session: Option<Dataset>,
    pub reference: Option<ReferenceFacts>,
    pub gates: [Gate; 12],
}
impl Suite {
    pub fn evaluate(
        seed: u64,
        samples: usize,
        soak_frames: usize,
        input: Option<&Path>,
        reference: Option<&Path>,
        stick: Stick,
    ) -> io::Result<Self> {
        if reference.is_some() && input.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reference CSV requires an already verified five-file XInput session",
            ));
        }
        let truth = truth_lab::evaluate(seed, samples)?;
        let matrix = Matrix::evaluate(seed.wrapping_add(1), samples)?;
        let long = soak(seed, soak_frames)?;
        let session = input.map(|path| Dataset::read(path, stick)).transpose()?;
        let ref_facts = match (reference, session.as_ref()) {
            (Some(path), Some(dataset)) => Some(external_reference(path, dataset)?),
            _ => None,
        };
        let gates = [
            Gate::BlockedPhysical, // 1: real acquisition *not* proven by file alone
            Gate::BlockedPhysical, // 2: caller labels not authenticated physical truth
            Gate::ResearchOnly,
            Gate::ResearchOnly,
            Gate::ResearchOnly,
            Gate::ResearchOnly,
            Gate::BlockedPhysical,
            Gate::BlockedPhysical,
            Gate::ResearchOnly,
            if long.unsafe_device_writes == 0 && long.nonfinite_outputs == 0 {
                Gate::VerifiedSoftware
            } else {
                Gate::ResearchOnly
            },
            Gate::BlockedPhysical,
            Gate::BlockedPhysical,
        ];
        Ok(Self {
            truth,
            matrix,
            long,
            session,
            reference: ref_facts,
            gates,
        })
    }
    pub fn manifest(&self) -> String {
        let physical = if self.session.is_some() {
            "manual_five_file_session_supplied_unverified_device_identity"
        } else {
            "no_physical_session_supplied"
        };
        let ref_status = if self.reference.is_some() {
            "operator_declared_external_source_not_authenticated"
        } else {
            "missing"
        };
        let checks = self
            .gates
            .iter()
            .zip(CHECKS.iter())
            .enumerate()
            .map(|(i, (gate, name))| {
                format!(
                    "{{\"id\":{},\"name\":\"{}\",\"status\":\"{}\"}}",
                    i + 1,
                    name,
                    gate.name()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"release\":\"0.27.0\",",
                "\"source\":\"integrated_twelve_workstream_research_suite\",",
                "\"checks\":[{}],\"workstream_count\":12,",
                "\"real_session\":\"{}\",\"external_reference\":\"{}\",",
                "\"software_safety_checks_pass\":{},",
                "\"real_virtual_driver_available_or_verified\":false,",
                "\"game_output_is_supported\":false,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            checks,
            physical,
            ref_status,
            self.long.unsafe_device_writes == 0 && self.long.nonfinite_outputs == 0
        )
    }
    fn reference_json(&self) -> String {
        match self.reference {
            Some(r) => format!(
                concat!(
                    "{{\"source\":\"operator_claimed_external_reference\",",
                    "\"observations\":{},\"declared_reference_rmse\":{:.10},",
                    "\"observed_wrong_vs_claimed\":{},",
                    "\"instrument_provenance_authenticated\":false,",
                    "\"hardware_identity_verified\":false,",
                    "\"production_qualified\":false}}\n"
                ),
                r.observations, r.observed_vs_declared_rmse, r.micro_wrong_vs_claimed
            ),
            None => String::from(
                "{\"source\":\"NO_EXTERNAL_REFERENCE\",\"ground_truth_achieved\":false}\n",
            ),
        }
    }
}
const GAME_ACCEPTANCE: &str = r#"# GuardAdaptive v0.27 Physical Acceptance Checklist

STATUS: NOT COMPLETE. This file is an operator protocol, NOT test evidence.

1. Attach intended XInput device; record VID/PID/serial from authoritative
   device APIs, if exposed; test unplug, reconnect and change of slot.
2. Perform four explicit local confirmations with neutral-session and save
   two independent, repeated sessions after exercising each intended stick.
3. Acquire independent intended movement using an instrumented jig or
   synchronized external reference; record time alignment and provenance.
4. Compare micro motion, severe drift, wrong sign, reversal timing and abrupt
   drift shifts against truly independent intended input.
5. Instrument upstream report timestamp and downstream on-screen/physical
   response independently: XInput polling time alone is not game latency.
6. Test any future virtual gamepad adapter OFF by default, explicit enable
   and emergency stop, raw fallback, unplug and driver-restart behavior.
7. Perform extended game tests with explicit user oversight, including
   Rocket League, on a separate test configuration. Anti-cheat/game rules
   and per-game compatibility must be assessed before attempting injection.
8. Run a 24h supervised soak with controller plugged in; confirm no
   duplicate gamepads, leaks, unintended clicks, freezes or focus theft.
9. Have an operator sign off results and retain device/release manifests.

This build contains NO ViGEm driver, virtual controller output or game
injection. Do not claim game compatibility or production qualification.
"#;
pub fn run(
    seed: u64,
    samples: usize,
    soak_frames: usize,
    input: Option<&Path>,
    reference: Option<&Path>,
    stick: Stick,
    dest: &Path,
) -> io::Result<()> {
    if dest.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "release output already exists",
        ));
    }
    let suite = Suite::evaluate(seed, samples, soak_frames, input, reference, stick)?;
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stem = dest
        .file_name()
        .ok_or_else(|| invalid("missing output name"))?;
    let stage = parent.join(format!(
        ".{}.{}.{}.v027-tmp",
        stem.to_string_lossy(),
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage)?;
    let result = (|| -> io::Result<()> {
        let files = [
            ("release_manifest.json", suite.manifest()),
            ("synthetic_oracle.json", suite.truth.json()),
            ("acceptance_matrix.json", suite.matrix.json()),
            ("soak_synthetic.json", suite.long.json()),
            ("independent_reference_status.json", suite.reference_json()),
            ("physical_test_protocol.md", String::from(GAME_ACCEPTANCE)),
        ];
        for (name, text) in files {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(stage.join(name))?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        if let Some(ref dataset) = suite.session {
            let f = stage.join("observational_dataset.csv");
            let internal = stage.join("observed_session");
            dataset.publish(stick, &internal)?;
            fs::copy(internal.join("observations.csv"), f)?;
        }
        #[cfg(windows)]
        {
            crate::capture::publish_new(&stage, dest)?;
        }
        #[cfg(not(windows))]
        {
            if dest.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination appeared",
                ));
            }
            fs::rename(&stage, dest)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result?;
    println!(
        "V0.27 INTEGRATED SUITE: 12 workstreams, synthetic samples={}, offline soak={}, session={}",
        samples,
        soak_frames,
        suite.session.is_some()
    );
    for (name, gate) in CHECKS.iter().zip(suite.gates.iter()) {
        println!("{name}={}", gate.name());
    }
    println!(
        "PRODUCTION_GATE=BLOCKED NO_GAME_DRIVER_INSTALLED NO_GAME_WRITES. {}",
        dest.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emergency_stop_and_disconnect_invalidate_calibration() {
        let mut b = DryRunBridge::new(Some(Vec2::new(0.2, 0.0)));
        b.set_mode(Mode::OfflinePreview);
        let frame = Sample {
            timestamp_us: 0,
            packet: 1,
            raw: Vec2::new(0.3, 0.0),
        };
        let p = b.process(frame);
        assert_eq!(p.reason, Reason::OfflinePreview);
        assert!((p.output.x - 0.1).abs() < 1e-12);
        b.emergency_off();
        assert_eq!(
            b.process(Sample {
                timestamp_us: 8000,
                packet: 2,
                raw: frame.raw
            })
            .output,
            frame.raw
        );
        b.set_mode(Mode::OfflinePreview);
        assert_eq!(
            b.process(Sample {
                timestamp_us: 16000,
                packet: 3,
                raw: frame.raw
            })
            .reason,
            Reason::Unverified
        );
        b.reconnect();
        assert_eq!(
            b.process(Sample {
                timestamp_us: 0,
                packet: 0,
                raw: frame.raw
            })
            .reason,
            Reason::Off
        );
        assert_eq!(
            b.device_write(frame.raw).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
    #[test]
    fn packet_reset_and_stale_report_latch_passthrough() {
        let raw = Vec2::new(0.7, 0.0);
        let mut b = DryRunBridge::new(Some(Vec2::new(0.1, 0.0)));
        b.set_mode(Mode::OfflinePreview);
        b.process(Sample {
            timestamp_us: 0,
            packet: 10,
            raw,
        });
        assert_eq!(
            b.process(Sample {
                timestamp_us: 8000,
                packet: 9,
                raw
            })
            .reason,
            Reason::PacketReset
        );
        assert_eq!(
            b.process(Sample {
                timestamp_us: 16000,
                packet: 11,
                raw
            })
            .output,
            raw
        );
        let mut c = DryRunBridge::new(Some(Vec2::new(0.1, 0.0)));
        c.set_mode(Mode::OfflinePreview);
        c.process(Sample {
            timestamp_us: 0,
            packet: 10,
            raw,
        });
        assert_eq!(
            c.process(Sample {
                timestamp_us: 80_000,
                packet: 11,
                raw
            })
            .reason,
            Reason::Stale
        );
        assert_eq!(
            c.process(Sample {
                timestamp_us: 88_000,
                packet: 12,
                raw
            })
            .reason,
            Reason::Stale
        );
    }
    #[test]
    fn synthetic_soak_covers_faults_with_zero_device_writes() {
        let r = soak(20261027, 30_000).unwrap();
        assert_eq!(r.frames, 30_000);
        assert_eq!(r.unsafe_device_writes, 0);
        assert_eq!(r.nonfinite_outputs, 0);
        assert!(r.stale > 0 && r.packet_reset > 0);
        assert!(r.connection_resets > 0 && r.kill_switch_events > 0);
        assert!(r.json().contains("\"real_game_test_completed\":false"));
    }
    #[test]
    fn integrated_manifest_has_exactly_twelve_nonapproved_checks() {
        let m = Suite::evaluate(20261027, 7200, 10000, None, None, Stick::Left).unwrap();
        assert_eq!(m.gates.len(), 12);
        assert!(
            m.gates
                .iter()
                .filter(|g| **g == Gate::BlockedPhysical)
                .count()
                >= 5
        );
        assert!(m.manifest().contains("\"production_gate\":\"BLOCKED\""));
        assert!(m.manifest().contains("\"game_output_is_supported\":false"));
        assert!(m.reference.is_none());
    }
    #[test]
    fn external_reference_rejects_unverified_source_and_misalignment() {
        let src =
            std::env::temp_dir().join(format!("guardadaptive-v27-ref-{}.csv", std::process::id()));
        let mut s = String::from(TRUTH_HEADER);
        s.push('\n');
        for i in 0..400 {
            s.push_str(&format!(
                "{i},{},0.00000,0.00000,synthetic_not_external\n",
                i * 8000
            ));
        }
        fs::write(&src, s).unwrap();
        // Full dataset is only constructible from five-file verified session;
        // the parser's source check is reached in integration tests when
        // a valid operator session is present.
        assert_eq!(fs::read_to_string(&src).unwrap().lines().count(), 401);
        assert!(Suite::evaluate(1, 7200, 10000, None, Some(&src), Stick::Left).is_err());
        let _ = fs::remove_file(src);
    }
    struct FakeReader {
        step: usize,
    }
    impl crate::capture::Reader for FakeReader {
        fn read(&mut self) -> io::Result<Option<crate::xinput::Report>> {
            let i = self.step;
            self.step += 1;
            if i >= 658 {
                return Ok(None);
            }
            let noise = if i.is_multiple_of(2) {
                0.00002
            } else {
                -0.00002
            };
            let (x, y) = if i <= 128 {
                (0.1 + noise, -0.2 - noise)
            } else if i <= 528 {
                (0.34, -0.2)
            } else {
                (0.1004 + noise, -0.2001 - noise)
            };
            Ok(Some(crate::xinput::Report {
                packet: (i / 2) as u32,
                left: Vec2::new(x, y),
                right: Vec2::ZERO,
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
            }))
        }
    }
    #[test]
    fn independent_reference_parser_checks_alignment_and_declared_provenance() {
        let id = TEMP.fetch_add(1, Ordering::Relaxed);
        let src = std::env::temp_dir().join(format!(
            "guardadaptive-v27-full-{}-{id}",
            std::process::id()
        ));
        crate::neutral_session::run_with(
            &mut FakeReader { step: 0 },
            0,
            Stick::Left,
            8,
            &src,
            false,
            &mut |_, _| Ok(()),
        )
        .unwrap();
        let dataset = Dataset::read(&src, Stick::Left).unwrap();
        let refpath = src.join("external_declared.csv");
        let mut text = String::from(TRUTH_HEADER);
        text.push('\n');
        for (i, o) in dataset.exercise.iter().enumerate() {
            text.push_str(&format!(
                "{i},{},{:.10},{:.10},external_instrument_operator_declared\n",
                o.elapsed_us,
                o.raw.x - dataset.audit.first.median.x,
                o.raw.y - dataset.audit.first.median.y
            ));
        }
        fs::write(&refpath, &text).unwrap();
        let r = external_reference(&refpath, &dataset).unwrap();
        assert_eq!(r.observations, 400);
        assert!(r.observed_vs_declared_rmse < 0.000001);
        let suite =
            Suite::evaluate(9, 7200, 10000, Some(&src), Some(&refpath), Stick::Left).unwrap();
        assert!(suite.reference.is_some());
        assert!(suite.manifest().contains("\"production_gate\":\"BLOCKED\""));
        let tampered = text.replacen(
            "external_instrument_operator_declared",
            "no_external_instrument",
            1,
        );
        fs::write(&refpath, tampered).unwrap();
        assert!(external_reference(&refpath, &dataset).is_err());
        let invalid = text
            .lines()
            .enumerate()
            .map(|(i, l)| {
                if i == 2 {
                    let mut fields = l.split(',').map(ToOwned::to_owned).collect::<Vec<_>>();
                    fields[1] = "99999999".into();
                    fields.join(",")
                } else {
                    l.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&refpath, invalid).unwrap();
        assert!(external_reference(&refpath, &dataset).is_err());
        let _ = fs::remove_dir_all(src);
    }
    #[test]
    fn integrated_bundle_is_nonreplacing_and_contains_all_twelve_statuses() {
        let id = TEMP.fetch_add(1, Ordering::Relaxed);
        let out =
            std::env::temp_dir().join(format!("guardadaptive-v27-out-{}-{id}", std::process::id()));
        run(7, 7200, 10000, None, None, Stick::Left, &out).unwrap();
        let manifest = fs::read_to_string(out.join("release_manifest.json")).unwrap();
        assert_eq!(manifest.matches("\"status\":").count(), 12);
        assert!(fs::read_to_string(out.join("physical_test_protocol.md"))
            .unwrap()
            .contains("Rocket League"));
        assert!(run(7, 7200, 10000, None, None, Stick::Left, &out).is_err());
        let _ = fs::remove_dir_all(out);
    }
    #[test]
    fn malformed_limits_are_rejected() {
        assert!(soak(2, 0).is_err());
        assert!(Suite::evaluate(2, 100, 10000, None, None, Stick::Left).is_err());
    }
}
