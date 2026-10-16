//! v0.39 controller-free, repeatable long-run stress of the entire
//! offline Frame -> Core -> XboxPacket -> memory-only Supervisor pipeline.
//! Software wall-clock timing is measured with Instant on this host. It is NOT physical
//! controller polling delay, Windows driver latency, or game response time.
use crate::{
    output_adapter::{DisabledBackend, VirtualGamepad, XboxPacket},
    output_supervisor::{fixture_topology, Phase, Stop, Supervisor},
    profile_store::Stick,
    reacquire::VerifiedCenter,
    runtime_core::{Core, Frame, Reason, State, Timing},
    Vec2,
};
use std::{io, path::Path, time::Instant};
const SEGMENT: usize = 4096;
const MIN_FRAMES: usize = SEGMENT * 5;
const MAX_FRAMES: usize = 2_097_152;
#[derive(Clone, Copy)]
pub struct Summary {
    pub total: u64,
    pub preview: u64,
    pub scripted_rearms: u64,
    pub stops: u64,
    pub stop_collision: u64,
    pub stop_epoch: u64,
    pub stop_timeout: u64,
    pub stop_rollback: u64,
    pub stop_emergency: u64,
    pub non_target_mismatches: u64,
    pub successfully_sent_to_game: u64,
    pub end_to_end_cpu: Timing,
    pub total_wall_ns: u128,
}
impl Summary {
    pub fn json(&self) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v39_synthetic_memory_pipeline_cpu_stress\",",
                "\"frames\":{},\"memory_preview_frames\":{},",
                "\"scripted_rearms_not_real_user_consents\":{},\"stop_events\":{},",
                "\"collision_stops\":{},\"epoch_stops\":{},\"watchdog_stops\":{},",
                "\"packet_rollback_stops\":{},\"emergency_stops\":{},",
                "\"non_target_mismatches\":{},\"game_writes\":{},",
                "\"total_software_loop_wall_ns\":{},\"measured_software_hotpath_wall_ns\":{},",
                "\"end_to_end_latency_measured\":false,",
                "\"synthetic_input_not_actual_controller\":true,",
                "\"virtual_driver_present\":false,",
                "\"automatic_restart_allowed\":false,",
                "\"physical_qualification\":\"BLOCKED\"}}\n"
            ),
            self.total,
            self.preview,
            self.scripted_rearms,
            self.stops,
            self.stop_collision,
            self.stop_epoch,
            self.stop_timeout,
            self.stop_rollback,
            self.stop_emergency,
            self.non_target_mismatches,
            self.successfully_sent_to_game,
            self.total_wall_ns,
            self.end_to_end_cpu.json()
        )
    }
}
fn center() -> VerifiedCenter {
    VerifiedCenter::restore_checked(Vec2::new(0.10, -0.05), Vec2::new(0.0001, 0.0001), 0.0001)
        .expect("bounded synthetic center")
}
fn input(global: usize, local: usize) -> Frame {
    let (x, y) = match (local / 256) % 4 {
        0 => (0.10, -0.05),
        1 => (0.1002, -0.0501),
        2 => (if local % 32 < 16 { 0.60 } else { -0.30 }, 0.12),
        _ => (0.10 + 0.30 * (local % 256) as f64 / 256.0, -0.20),
    };
    Frame {
        monotonic_us: local as u64 * 8_000,
        packet: (local / 2) as u32,
        left: Vec2::new(x, y),
        right: Vec2::new(-y, x),
        buttons: (global as u16) ^ 0xA35C,
        left_trigger: (global % 256) as u8,
        right_trigger: (255 - global % 256) as u8,
    }
}
pub fn evaluate(count: usize, seed: u64) -> io::Result<Summary> {
    if !(MIN_FRAMES..=MAX_FRAMES).contains(&count) || !count.is_multiple_of(SEGMENT) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frames must be 20480..2097152 and divisible by 4096",
        ));
    }
    let topology = fixture_topology();
    let mut summary = Summary {
        total: count as u64,
        preview: 0,
        scripted_rearms: 0,
        stops: 0,
        stop_collision: 0,
        stop_epoch: 0,
        stop_timeout: 0,
        stop_rollback: 0,
        stop_emergency: 0,
        non_target_mismatches: 0,
        successfully_sent_to_game: 0,
        end_to_end_cpu: Timing::default(),
        total_wall_ns: 0,
    };
    let mut guard = Supervisor::default();
    let mut core = Core::default();
    let mut token = seed.wrapping_add(1);
    if token == 0 {
        token = 1;
    }
    let begin = Instant::now();
    for i in 0..count {
        let local = i % SEGMENT;
        let segment = i / SEGMENT;
        let epoch = segment as u64 + 1;
        let stick = if segment.is_multiple_of(2) {
            Stick::Left
        } else {
            Stick::Right
        };
        if local == 0 {
            if segment > 0 {
                if guard.phase() != Phase::LatchedFault
                    || !guard.reset_by_operator(true)
                    || guard.held_packet().is_some()
                {
                    return Err(io::Error::other("fault reset not acknowledged"));
                }
                // A new TEST SCRIPT acknowledgment is explicit in the harness
                // but does not represent a live person's consent.
                token = token
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("synthetic token overflow"))?;
            }
            if !core.arm_stick(center(), epoch, true, stick) {
                return Err(io::Error::other("explicit synthetic calibration refused"));
            }
            guard
                .arm_memory_only(token, epoch, true, topology)
                .map_err(|e| io::Error::other(e.name()))?;
            summary.scripted_rearms += 1;
        }
        let raw = input(i, local);
        let tick = Instant::now();
        let shadow = core.step(raw, epoch);
        if shadow.reason != Reason::PreviewOnly
            || shadow.delivered_to_game
            || core.state() != State::Preview
        {
            return Err(io::Error::other("unexpected offline preview state"));
        }
        let packet = XboxPacket::from_shadow(shadow)?;
        // All non-target controls are losslessly copied. Axes are
        // deliberately i16-quantized by XboxPacket::encode.
        let baseline = XboxPacket::encode(raw)?;
        if packet.buttons != baseline.buttons
            || packet.left_trigger != baseline.left_trigger
            || packet.right_trigger != baseline.right_trigger
            || (stick == Stick::Left
                && (packet.right_x, packet.right_y) != (baseline.right_x, baseline.right_y))
            || (stick == Stick::Right
                && (packet.left_x, packet.left_y) != (baseline.left_x, baseline.left_y))
        {
            return Err(io::Error::other("non-target controls were modified"));
        }
        guard
            .accept_shadow(packet, epoch, raw.monotonic_us, raw.packet, topology)
            .map_err(|e| io::Error::other(e.name()))?;
        summary
            .end_to_end_cpu
            .observe(tick.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        summary.preview += 1;
        if local == SEGMENT - 1 {
            let event = (segment + (seed as usize % 5)) % 5;
            match event {
                0 => {
                    let duplicate = crate::routing_guard::Topology {
                        physical_visible_to_game: Some(true),
                        ..topology
                    };
                    if guard.accept_shadow(
                        packet,
                        epoch,
                        raw.monotonic_us + 8000,
                        raw.packet + 1,
                        duplicate,
                    ) != Err(Stop::DuplicateVisibility)
                    {
                        return Err(io::Error::other("duplicate controller not blocked"));
                    }
                    summary.stop_collision += 1;
                }
                1 => {
                    if guard.accept_shadow(
                        packet,
                        epoch + 1,
                        raw.monotonic_us + 8000,
                        raw.packet + 1,
                        topology,
                    ) != Err(Stop::SessionChanged)
                    {
                        return Err(io::Error::other("epoch switch not blocked"));
                    }
                    summary.stop_epoch += 1;
                }
                2 => {
                    if !guard.watchdog(raw.monotonic_us + 50_001) {
                        return Err(io::Error::other("watchdog timeout not detected"));
                    }
                    summary.stop_timeout += 1;
                }
                3 => {
                    if guard.accept_shadow(
                        packet,
                        epoch,
                        raw.monotonic_us + 8000,
                        raw.packet.saturating_sub(1),
                        topology,
                    ) != Err(Stop::PacketRollback)
                    {
                        return Err(io::Error::other("packet rollback not blocked"));
                    }
                    summary.stop_rollback += 1;
                }
                _ => {
                    guard.emergency_stop();
                    summary.stop_emergency += 1;
                }
            }
            core.emergency_off();
            if guard.phase() != Phase::LatchedFault || guard.held_packet().is_some() {
                return Err(io::Error::other("stale packet survived fault"));
            }
            if guard.arm_memory_only(token, epoch, true, topology) != Err(Stop::ResetRequired)
                || guard.held_packet().is_some()
            {
                return Err(io::Error::other("implicit rearm allowed"));
            }
            summary.stops += 1;
        }
    }
    summary.total_wall_ns = begin.elapsed().as_nanos();
    if summary.preview != summary.total
        || summary.stops != summary.scripted_rearms
        || [
            summary.stop_collision,
            summary.stop_epoch,
            summary.stop_timeout,
            summary.stop_rollback,
            summary.stop_emergency,
        ]
        .contains(&0)
    {
        return Err(io::Error::other("incomplete stress coverage"));
    }
    // Explicitly verify the *shipping* disabled backend still refuses output,
    // regardless of fully passing synthetic topology and offline preview.
    let mut shipping = DisabledBackend;
    match shipping.send(XboxPacket::encode(input(0, 0))?) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
        _ => {
            return Err(io::Error::other(
                "shipping driver unexpectedly accepted output",
            ))
        }
    }
    shipping.deactivate()?;
    Ok(summary)
}
pub fn run(frames: usize, seed: u64, out: &Path) -> io::Result<()> {
    let report = evaluate(frames, seed)?;
    crate::truth_lab::save_new(out, &report.json())?;
    println!("V039  frames={} offline_preview={} injected_fail_stops={} SOFTWARE-p99-upper-bucket-ns={} NO_DRIVER NO_GAME_WRITES production=BLOCKED",
        report.total,report.preview,report.stops,
        report.end_to_end_cpu.bucket_quantile_upper_ns(99));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stress_smoke_has_all_faults_and_no_data_leak() {
        let v = evaluate(SEGMENT * 10, 11).unwrap();
        assert_eq!(v.total, v.preview);
        assert_eq!(v.stops, 10);
        assert_eq!(v.non_target_mismatches, 0);
        assert_eq!(v.successfully_sent_to_game, 0);
        assert!(v.json().contains("\"end_to_end_latency_measured\":false"));
        assert!(v.json().contains("\"virtual_driver_present\":false"));
    }
    #[test]
    fn different_seed_runs_cover_all_discontinuities() {
        for seed in [0, 1, 2, 20261013] {
            let v = evaluate(SEGMENT * 5, seed).unwrap();
            assert_eq!(v.stops, 5);
            assert!(v.stop_collision > 0 && v.stop_epoch > 0 && v.stop_timeout > 0);
            assert!(v.stop_rollback > 0 && v.stop_emergency > 0);
        }
    }
    #[test]
    fn workload_refuses_unbalanced_or_large_runs() {
        for count in [0, 4096, 8192, 10000, 100000, MAX_FRAMES + 1] {
            assert!(evaluate(count, 1).is_err(), "{count}");
        }
    }
    #[test]
    fn report_is_non_overwriting() {
        let path = std::env::temp_dir().join(format!("ga-v39-{}.json", std::process::id()));
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        run(SEGMENT * 5, 1, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(run(SEGMENT * 5, 2, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_file(&path);
    }
}
