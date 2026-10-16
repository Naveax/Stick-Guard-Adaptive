//! v0.38 fail-closed OUTPUT LIFECYCLE SIMULATOR.
//! In-memory Xbox packets only. There is intentionally no OS driver, device
//! handle or game-write mechanism in this module. Simulated telemetry and
//! operator-supplied topology MUST NEVER authorize real output.
use crate::{
    output_adapter::{DisabledBackend, VirtualGamepad, XboxPacket},
    routing_guard::{Topology, Verdict},
    runtime_core::Frame,
};
use std::{io, path::Path};

const MAX_GAP_US: u64 = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Disarmed,
    MemoryPreview,
    LatchedFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    None,
    NoUserConsent,
    UnverifiedTopology,
    DuplicateVisibility,
    SessionChanged,
    ClockRollback,
    Timeout,
    PacketRollback,
    TransportRefused,
    EmergencyStop,
    ResetRequired,
    ReplayToken,
}
impl Stop {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NoUserConsent => "no_user_consent",
            Self::UnverifiedTopology => "unverified_simulated_topology",
            Self::DuplicateVisibility => "duplicate_game_visible_controllers",
            Self::SessionChanged => "connection_epoch_mismatch",
            Self::ClockRollback => "monotonic_clock_rollback",
            Self::Timeout => "heartbeat_gap_over_50ms",
            Self::PacketRollback => "xinput_packet_counter_rollback",
            Self::TransportRefused => "disabled_virtual_backend_rejected_packet",
            Self::EmergencyStop => "user_emergency_stop",
            Self::ResetRequired => "operator_reset_required",
            Self::ReplayToken => "replayed_authorization_token",
        }
    }
}
#[derive(Default)]
struct MemoryPort {
    last: Option<XboxPacket>,
    samples: u64,
    clears: u64,
}
impl MemoryPort {
    fn push(&mut self, packet: XboxPacket) {
        self.last = Some(packet);
        self.samples += 1;
    }
    fn clear(&mut self) {
        self.last = None;
        self.clears += 1;
    }
}
pub struct Supervisor {
    phase: Phase,
    stop: Stop,
    port: MemoryPort,
    topology: Option<Topology>,
    epoch: Option<u64>,
    token_high_water: u64,
    last_sample: Option<(u64, u32)>,
    stops: u64,
}
impl Default for Supervisor {
    fn default() -> Self {
        Self {
            phase: Phase::Disarmed,
            stop: Stop::None,
            port: MemoryPort::default(),
            topology: None,
            epoch: None,
            token_high_water: 0,
            last_sample: None,
            stops: 0,
        }
    }
}
impl Supervisor {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn stop_reason(&self) -> Stop {
        self.stop
    }
    pub fn held_packet(&self) -> Option<XboxPacket> {
        self.port.last
    }
    pub fn captured(&self) -> u64 {
        self.port.samples
    }
    pub fn stops(&self) -> u64 {
        self.stops
    }
    pub fn memory_clears(&self) -> u64 {
        self.port.clears
    }
    fn latch(&mut self, why: Stop) {
        self.port.clear();
        self.phase = Phase::LatchedFault;
        self.stop = why;
        self.epoch = None;
        self.topology = None;
        self.last_sample = None;
        self.stops += 1;
    }
    pub fn emergency_stop(&mut self) {
        self.latch(Stop::EmergencyStop);
    }
    pub fn disconnect(&mut self) {
        self.latch(Stop::SessionChanged);
    }
    /// Physical topology and driver identity are NOT tested in this build.
    /// Arm is a purely synthetic MEMORY_PREVIEW request, not a game-output grant.
    pub fn arm_memory_only(
        &mut self,
        token: u64,
        epoch: u64,
        operator_consent: bool,
        topology: Topology,
    ) -> Result<(), Stop> {
        if self.phase != Phase::Disarmed {
            self.latch(Stop::ResetRequired);
            return Err(Stop::ResetRequired);
        }
        self.port.clear();
        if !operator_consent {
            return Err(Stop::NoUserConsent);
        }
        if token == 0 || token <= self.token_high_water {
            return Err(Stop::ReplayToken);
        }
        if topology.check() != Verdict::SimulatedTopologyEligible {
            return Err(Self::topology_reason(topology));
        }
        self.token_high_water = token;
        self.topology = Some(topology);
        self.epoch = Some(epoch);
        self.last_sample = None;
        self.phase = Phase::MemoryPreview;
        self.stop = Stop::None;
        Ok(())
    }
    fn topology_reason(topology: Topology) -> Stop {
        if topology.check() == Verdict::DuplicateGameVisibleController {
            Stop::DuplicateVisibility
        } else {
            Stop::UnverifiedTopology
        }
    }
    /// Reset does not grant consent, create a new session or re-arm.
    pub fn reset_by_operator(&mut self, operator_acknowledged: bool) -> bool {
        if !operator_acknowledged || self.phase != Phase::LatchedFault {
            return false;
        }
        self.port.clear();
        self.phase = Phase::Disarmed;
        self.stop = Stop::None;
        self.epoch = None;
        self.topology = None;
        self.last_sample = None;
        true
    }
    /// Record a simulated packet in memory ONLY. Never calls a virtual driver.
    pub fn accept_shadow(
        &mut self,
        packet: XboxPacket,
        epoch: u64,
        t_us: u64,
        packet_id: u32,
        topology: Topology,
    ) -> Result<(), Stop> {
        if self.phase != Phase::MemoryPreview {
            return Err(Stop::ResetRequired);
        }
        if self.epoch != Some(epoch) {
            self.latch(Stop::SessionChanged);
            return Err(Stop::SessionChanged);
        }
        if self.topology != Some(topology) || topology.check() != Verdict::SimulatedTopologyEligible
        {
            let why = Self::topology_reason(topology);
            self.latch(why);
            return Err(why);
        }
        if let Some((t, previous)) = self.last_sample {
            let why = if t_us < t {
                Some(Stop::ClockRollback)
            } else if t_us - t > MAX_GAP_US {
                Some(Stop::Timeout)
            } else if packet_id < previous {
                Some(Stop::PacketRollback)
            } else {
                None
            };
            if let Some(why) = why {
                self.latch(why);
                return Err(why);
            }
        }
        self.last_sample = Some((t_us, packet_id));
        self.port.push(packet);
        Ok(())
    }
    pub fn watchdog(&mut self, current_t_us: u64) -> bool {
        if self.phase != Phase::MemoryPreview {
            return false;
        }
        let Some((last, _)) = self.last_sample else {
            return false;
        };
        if current_t_us < last {
            self.latch(Stop::ClockRollback);
            return true;
        }
        if current_t_us - last > MAX_GAP_US {
            self.latch(Stop::Timeout);
            return true;
        }
        false
    }
    /// Assert the real shipping backend *still* rejects an otherwise valid
    /// packet, independently from simulated routing or a memory-only arm.
    pub fn verify_shipping_backend_denied(&mut self, packet: XboxPacket) -> io::Result<()> {
        let mut disabled = DisabledBackend;
        let result = disabled.send(packet);
        let _ = disabled.deactivate();
        if matches!(result,Err(ref e) if e.kind()==io::ErrorKind::PermissionDenied) {
            Ok(())
        } else {
            self.latch(Stop::TransportRefused);
            Err(io::Error::other(
                "virtual driver unexpectedly accepted a write",
            ))
        }
    }
    pub fn status(&self, frames: u64) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v38_synthetic_output_supervisor\",",
                "\"frames\":{},\"memory_packets\":{},\"stops\":{},",
                "\"memory_clears\":{},\"last_packet_retained\":{},",
                "\"phase\":\"{:?}\",\"last_reason\":\"{}\",",
                "\"driver_available\":false,\"gamepad_output_writes\":0,",
                "\"operator_topology_authenticated\":false,",
                "\"implicit_restart_permitted\":false,",
                "\"physical_acceptance\":\"BLOCKED\"}}\n"
            ),
            frames,
            self.port.samples,
            self.stops,
            self.port.clears,
            self.port.last.is_some(),
            self.phase,
            self.stop.name()
        )
    }
}
pub fn fixture_topology() -> Topology {
    Topology {
        explicit_user_consent: true,
        stable_hardware_identity_authenticated: true,
        physical_input_slot: Some(0),
        virtual_output_slot: Some(1),
        virtual_backend_verified: true,
        physical_visible_to_game: Some(false),
        virtual_visible_to_game: Some(true),
    }
}
pub fn fixture_frame(i: u64) -> Frame {
    Frame {
        monotonic_us: i * 8000,
        packet: (i / 2) as u32,
        left: crate::Vec2::new(0.07, -0.05),
        right: crate::Vec2::new(-0.04, 0.08),
        left_trigger: (i % 256) as u8,
        right_trigger: (255 - i % 256) as u8,
        buttons: (i as u16) ^ 0xA55A,
    }
}
pub fn run(count: usize, out: &Path) -> io::Result<()> {
    if !(4096..=1_000_000).contains(&count) || !count.is_multiple_of(4) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame count must be divisible by 4 and within 4096..1000000",
        ));
    }
    let good = fixture_topology();
    let bad = Topology {
        physical_visible_to_game: Some(true),
        ..good
    };
    let mut supervisor = Supervisor::default();
    supervisor
        .arm_memory_only(1, 1, true, good)
        .map_err(|why| io::Error::other(why.name()))?;
    let mut rejected = 0u64;
    let mut backend_denials = 0u64;
    let mut last_reason = Stop::None;
    for i in 0..count {
        let f = fixture_frame(i as u64);
        // Exercise collision, explicit recovery, packet reversal, watchdog,
        // and nonreplayable tokens. The sender exists in MEMORY only.
        if i == count / 4 {
            let p = XboxPacket::encode(f)?;
            if supervisor.accept_shadow(p, 1, f.monotonic_us, f.packet, bad)
                != Err(Stop::DuplicateVisibility)
            {
                return Err(io::Error::other("collision not rejected"));
            }
            last_reason = supervisor.stop_reason();
        }
        if i == count / 2 {
            if supervisor.arm_memory_only(2, 2, true, good) != Err(Stop::ResetRequired) {
                return Err(io::Error::other("fault unexpectedly auto-rearmed"));
            }
            if !supervisor.reset_by_operator(true) {
                return Err(io::Error::other("explicit reset failed"));
            }
            supervisor
                .arm_memory_only(2, 2, true, good)
                .map_err(|r| io::Error::other(r.name()))?;
        }
        if i == 3 * count / 4 {
            if !supervisor.watchdog(f.monotonic_us + 100_000) {
                return Err(io::Error::other("timeout not detected"));
            }
            last_reason = supervisor.stop_reason();
        }
        if supervisor.phase() != Phase::MemoryPreview {
            rejected += 1;
            continue;
        }
        let p = XboxPacket::encode(f)?;
        if i % 2048 == 0 {
            supervisor.verify_shipping_backend_denied(p)?;
            backend_denials += 1;
        }
        supervisor
            .accept_shadow(
                p,
                if i >= count / 2 { 2 } else { 1 },
                f.monotonic_us,
                f.packet,
                good,
            )
            .map_err(|r| io::Error::other(r.name()))?;
    }
    if supervisor.held_packet().is_some()
        || supervisor.phase() != Phase::LatchedFault
        || supervisor.captured() == 0
        || rejected == 0
        || last_reason != Stop::Timeout
    {
        return Err(io::Error::other("supervisor acceptance invariants failed"));
    }
    if supervisor.arm_memory_only(2, 2, true, good) != Err(Stop::ResetRequired) {
        return Err(io::Error::other("old session survived watchdog stop"));
    }
    if !supervisor.reset_by_operator(true) {
        return Err(io::Error::other("final reset failed"));
    }
    if supervisor.arm_memory_only(2, 2, true, good) != Err(Stop::ReplayToken) {
        return Err(io::Error::other("session token replay allowed"));
    }
    supervisor.emergency_stop();
    if supervisor.held_packet().is_some() {
        return Err(io::Error::other("emergency stop retained stale packet"));
    }
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v38_no_driver_output_lifecycle\",",
            "\"frames\":{},\"memory_captured\":{},\"blocked_frames\":{},",
            "\"stops\":{},\"memory_clears\":{},\"backend_denied_checks\":{},",
            "\"final_stale_packet\":false,\"replay_accepted\":false,",
            "\"auto_rearm\":false,\"real_hardware_identity_tested\":false,",
            "\"virtual_driver_installed\":false,\"game_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        count,
        supervisor.captured(),
        rejected,
        supervisor.stops(),
        supervisor.memory_clears(),
        backend_denials
    );
    crate::truth_lab::save_new(out, &report)?;
    println!("V038 output lifecycle: frames={count} captured={} blocked={} stops={} driver_writes=0 production=BLOCKED",
        supervisor.captured(),rejected,supervisor.stops());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> XboxPacket {
        XboxPacket::encode(fixture_frame(1)).unwrap()
    }
    #[test]
    fn no_permission_or_valid_topology_means_no_memory_arm() {
        let mut s = Supervisor::default();
        assert_eq!(
            s.arm_memory_only(1, 1, false, fixture_topology()),
            Err(Stop::NoUserConsent)
        );
        assert_eq!(s.phase(), Phase::Disarmed);
        assert_eq!(
            s.arm_memory_only(
                1,
                1,
                true,
                Topology {
                    physical_visible_to_game: None,
                    ..fixture_topology()
                }
            ),
            Err(Stop::UnverifiedTopology)
        );
        assert_eq!(s.held_packet(), None);
    }
    #[test]
    fn duplicate_visibility_latches_and_requires_explicit_reset() {
        let mut s = Supervisor::default();
        let g = fixture_topology();
        s.arm_memory_only(100, 1, true, g).unwrap();
        s.accept_shadow(packet(), 1, 8000, 1, g).unwrap();
        assert!(s.held_packet().is_some());
        assert_eq!(
            s.accept_shadow(
                packet(),
                1,
                16000,
                2,
                Topology {
                    physical_visible_to_game: Some(true),
                    ..g
                }
            ),
            Err(Stop::DuplicateVisibility)
        );
        assert_eq!(s.phase(), Phase::LatchedFault);
        assert_eq!(s.held_packet(), None);
        assert!(!s.reset_by_operator(false));
        assert_eq!(s.arm_memory_only(101, 1, true, g), Err(Stop::ResetRequired));
        assert!(s.reset_by_operator(true));
        assert_eq!(s.arm_memory_only(100, 1, true, g), Err(Stop::ReplayToken));
        s.arm_memory_only(101, 2, true, g).unwrap();
        assert_eq!(s.phase(), Phase::MemoryPreview);
    }
    #[test]
    fn live_memory_arm_cannot_silently_replace_operator_token() {
        let mut s = Supervisor::default();
        let g = fixture_topology();
        s.arm_memory_only(1, 1, true, g).unwrap();
        s.accept_shadow(packet(), 1, 8000, 1, g).unwrap();
        assert_eq!(s.arm_memory_only(2, 2, true, g), Err(Stop::ResetRequired));
        assert_eq!(s.phase(), Phase::LatchedFault);
        // Replacing the arm attempt latches a fault and drops any stale packet.
        assert!(s.held_packet().is_none());
        s.emergency_stop();
        assert!(!s.reset_by_operator(false));
    }
    #[test]
    fn epoch_rollback_gap_and_clock_fail_closed() {
        let g = fixture_topology();
        for (t, p, epoch, expected) in [
            (16000, 2, 3, Stop::SessionChanged),
            (7000, 2, 1, Stop::ClockRollback),
            (60001, 2, 1, Stop::Timeout),
            (16000, 0, 1, Stop::PacketRollback),
        ] {
            let mut s = Supervisor::default();
            s.arm_memory_only(1, 1, true, g).unwrap();
            s.accept_shadow(packet(), 1, 8000, 1, g).unwrap();
            assert_eq!(s.accept_shadow(packet(), epoch, t, p, g), Err(expected));
            assert_eq!(s.phase(), Phase::LatchedFault);
            assert!(s.held_packet().is_none());
        }
    }
    #[test]
    fn watchdog_and_emergency_clear_latest_packet() {
        let g = fixture_topology();
        let mut s = Supervisor::default();
        s.arm_memory_only(1, 1, true, g).unwrap();
        s.accept_shadow(packet(), 1, 10000, 1, g).unwrap();
        assert!(!s.watchdog(60000));
        assert!(s.held_packet().is_some());
        assert!(s.watchdog(60001));
        assert_eq!(s.stop_reason(), Stop::Timeout);
        assert!(s.held_packet().is_none());
        assert!(s.reset_by_operator(true));
        s.arm_memory_only(2, 2, true, g).unwrap();
        s.accept_shadow(packet(), 2, 2000, 3, g).unwrap();
        s.emergency_stop();
        assert!(s.held_packet().is_none());
        assert_eq!(s.stop_reason(), Stop::EmergencyStop);
    }
    #[test]
    fn disabled_backend_denies_even_when_simulated_topology_eligible() {
        let g = fixture_topology();
        let mut s = Supervisor::default();
        s.arm_memory_only(1, 1, true, g).unwrap();
        s.accept_shadow(packet(), 1, 8000, 1, g).unwrap();
        s.verify_shipping_backend_denied(packet()).unwrap();
        assert_eq!(s.captured(), 1);
        assert!(s.status(1).contains("\"gamepad_output_writes\":0"));
        s.disconnect();
        assert_eq!(s.phase(), Phase::LatchedFault);
        assert!(s.held_packet().is_none());
    }
    #[test]
    fn integrated_in_memory_preview_packet_obeys_supervisor() {
        use crate::{
            pipeline_v32::Pipeline, profile_store::Stick, reacquire::VerifiedCenter, Vec2,
        };
        let center = VerifiedCenter::restore_checked(
            Vec2::new(0.08, -0.06),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let g = fixture_topology();
        let mut pipeline = Pipeline::synthetic(g, 1, Stick::Left, center);
        let mut guard = Supervisor::default();
        guard.arm_memory_only(1, 1, true, g).unwrap();
        let f = fixture_frame(1);
        let (_, packet) = pipeline.process(f, 1).unwrap();
        guard.accept_shadow(packet.unwrap(), 1, 8000, 0, g).unwrap();
        assert_eq!(guard.captured(), 1);
        pipeline.update_topology(Topology {
            physical_visible_to_game: Some(true),
            ..g
        });
        assert_eq!(
            guard.accept_shadow(
                packet.unwrap(),
                1,
                16000,
                1,
                Topology {
                    physical_visible_to_game: Some(true),
                    ..g
                }
            ),
            Err(Stop::DuplicateVisibility)
        );
        assert_eq!(guard.held_packet(), None);
        assert!(pipeline.last_packet().is_none());
    }
    #[test]
    fn reproducible_offline_lifecycle_and_refusal_to_overwrite() {
        let out = std::env::temp_dir().join(format!("ga-v38-{}.json", std::process::id()));
        if out.exists() {
            std::fs::remove_file(&out).unwrap();
        }
        run(4096, &out).unwrap();
        let body = std::fs::read_to_string(&out).unwrap();
        assert!(body.contains("\"game_writes\":0"));
        assert!(body.contains("\"auto_rearm\":false"));
        assert!(run(4096, &out).is_err());
        let _ = std::fs::remove_file(out);
    }
}
