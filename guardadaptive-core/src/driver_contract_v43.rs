//! v0.43 preliminary XUSB-style 12-byte report serializer and gated
//! transport lifecycle. This is a memory-only contract, NOT a ViGEm/Windows
//! driver binding. The production transport is always unavailable.
//! Synthetic routing claims are never accepted as physical authorization.
use crate::{
    output_adapter::XboxPacket,
    output_supervisor::fixture_topology,
    routing_guard::{Topology, Verdict},
};
use std::{collections::VecDeque, io, path::Path};
const WIRE_SIZE: usize = 12;
const MAX_STAGED: usize = 64;
const POLL_MAX_US: u64 = 50_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XusbWire([u8; WIRE_SIZE]);
impl XusbWire {
    /// Candidate report encoding; requires vendor ABI validation before any
    /// driver can legally/technically consume it.
    pub fn encode(packet: XboxPacket) -> Self {
        let mut b = [0u8; WIRE_SIZE];
        b[0..2].copy_from_slice(&packet.buttons.to_le_bytes());
        b[2] = packet.left_trigger;
        b[3] = packet.right_trigger;
        for (i, a) in [packet.left_x, packet.left_y, packet.right_x, packet.right_y]
            .iter()
            .enumerate()
        {
            b[4 + 2 * i..6 + 2 * i].copy_from_slice(&a.to_le_bytes());
        }
        Self(b)
    }
    pub fn bytes(self) -> [u8; WIRE_SIZE] {
        self.0
    }
    pub fn decode(self) -> XboxPacket {
        let a = self.0;
        XboxPacket {
            buttons: u16::from_le_bytes([a[0], a[1]]),
            left_trigger: a[2],
            right_trigger: a[3],
            left_x: i16::from_le_bytes([a[4], a[5]]),
            left_y: i16::from_le_bytes([a[6], a[7]]),
            right_x: i16::from_le_bytes([a[8], a[9]]),
            right_y: i16::from_le_bytes([a[10], a[11]]),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    ProductionDisabled,
    OfflineMemoryLab,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Shadow,
    LatchedFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    NoDriver,
    NoConsent,
    NoTopology,
    DuplicateVisibility,
    ActiveAlready,
    MustReset,
    ReplayAuthorization,
    EpochReplay,
    CounterExhausted,
    DeviceEpoch,
    PacketRollback,
    DuplicatePacketCounter,
    ClockRollback,
    PollTimeout,
    Emergency,
    UntrustedReport,
    ReplayedPoll,
}
impl Failure {
    pub fn name(self) -> &'static str {
        match self {
            Self::NoDriver => "driver_unavailable",
            Self::NoConsent => "explicit_consent_required",
            Self::NoTopology => "unverified_simulated_topology",
            Self::DuplicateVisibility => "duplicate_input_blocked",
            Self::ActiveAlready => "second_activation_fault",
            Self::MustReset => "latched_fault_requires_reset",
            Self::ReplayAuthorization => "replayed_authorization_rejected",
            Self::EpochReplay => "stale_or_replayed_device_epoch",
            Self::CounterExhausted => "token_or_epoch_counter_at_u64_limit",
            Self::DeviceEpoch => "device_epoch_mismatch",
            Self::PacketRollback => "packet_counter_rollback",
            Self::DuplicatePacketCounter => "packet_counter_reused_with_new_timestamp",
            Self::ClockRollback => "clock_rollback",
            Self::PollTimeout => "gap_or_watchdog_over_50ms",
            Self::Emergency => "emergency_stop",
            Self::UntrustedReport => "untrusted_report",
            Self::ReplayedPoll => "duplicate_poll_timestamp",
        }
    }
}
pub struct BackendContract {
    mode: Mode,
    state: State,
    fault: Option<Failure>,
    last_auth: u64,
    // Never reset this watermark; a new token cannot resurrect an old epoch.
    last_epoch: u64,
    epoch: Option<u64>,
    topology: Option<Topology>,
    last_poll: Option<(u64, u32)>,
    armed_at: Option<u64>,
    staged: VecDeque<XusbWire>,
    accepted: u64,
    cleared: u64,
    fault_count: u64,
}
impl BackendContract {
    pub fn shipping() -> Self {
        Self::new(Mode::ProductionDisabled)
    }
    pub fn offline_lab() -> Self {
        Self::new(Mode::OfflineMemoryLab)
    }
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            state: State::Off,
            fault: None,
            last_auth: 0,
            last_epoch: 0,
            epoch: None,
            topology: None,
            last_poll: None,
            armed_at: None,
            staged: VecDeque::with_capacity(MAX_STAGED),
            accepted: 0,
            cleared: 0,
            fault_count: 0,
        }
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn last_fault(&self) -> Option<Failure> {
        self.fault
    }
    pub fn retained(&self) -> usize {
        self.staged.len()
    }
    pub fn accepted(&self) -> u64 {
        self.accepted
    }
    pub fn fault_count(&self) -> u64 {
        self.fault_count
    }
    pub fn latest(&self) -> Option<XusbWire> {
        self.staged.back().copied()
    }
    fn clear(&mut self) {
        self.staged.clear();
        self.cleared += 1;
    }
    fn latch(&mut self, why: Failure) {
        self.clear();
        self.state = State::LatchedFault;
        self.fault = Some(why);
        self.epoch = None;
        self.topology = None;
        self.last_poll = None;
        self.armed_at = None;
        self.fault_count += 1;
    }
    fn topology_reason(topology: Topology) -> Failure {
        if topology.check() == Verdict::DuplicateGameVisibleController {
            Failure::DuplicateVisibility
        } else {
            Failure::NoTopology
        }
    }
    pub fn arm(
        &mut self,
        token: u64,
        epoch: u64,
        consent: bool,
        topology: Topology,
    ) -> Result<(), Failure> {
        // Shipping backend is permanently disabled, regardless of the
        // apparently-valid synthetic topology or user-provided claims.
        if self.mode == Mode::ProductionDisabled {
            return Err(Failure::NoDriver);
        }
        if self.state == State::LatchedFault {
            return Err(Failure::MustReset);
        }
        if self.state != State::Off {
            self.latch(Failure::ActiveAlready);
            return Err(Failure::ActiveAlready);
        }
        if !consent {
            return Err(Failure::NoConsent);
        }
        if token == 0 || token <= self.last_auth {
            return Err(Failure::ReplayAuthorization);
        }
        if epoch == 0 || epoch <= self.last_epoch {
            return Err(Failure::EpochReplay);
        }
        // Explicit terminal boundary. No modular wrap or hidden reuse
        // of token/epoch identifiers at u64::MAX is ever permitted.
        if token == u64::MAX || epoch == u64::MAX {
            return Err(Failure::CounterExhausted);
        }
        if topology.check() != Verdict::SimulatedTopologyEligible {
            return Err(Self::topology_reason(topology));
        }
        self.clear();
        self.epoch = Some(epoch);
        self.topology = Some(topology);
        self.last_auth = token;
        self.last_epoch = epoch;
        self.last_poll = None;
        self.armed_at = None;
        self.state = State::Shadow;
        self.fault = None;
        Ok(())
    }
    /// Timestamp-bound offline arm. Unlike the historical synthetic `arm`,
    /// it detects a stalled source before the FIRST poll. Neither entrypoint
    /// can ever turn the production/driver path on.
    pub fn arm_at(
        &mut self,
        token: u64,
        epoch: u64,
        consent: bool,
        topology: Topology,
        now_us: u64,
    ) -> Result<(), Failure> {
        self.arm(token, epoch, consent, topology)?;
        self.armed_at = Some(now_us);
        Ok(())
    }
    /// A synthetic packet is staged into a BOUNDED memory ring, not a driver.
    /// Full packet conversion is separately validated through XusbWire.
    pub fn submit(
        &mut self,
        packet: XboxPacket,
        epoch: u64,
        stamp: u64,
        packet_id: u32,
        topology: Topology,
    ) -> Result<(), Failure> {
        if self.mode == Mode::ProductionDisabled {
            return Err(Failure::NoDriver);
        }
        if self.state != State::Shadow {
            return Err(Failure::MustReset);
        }
        if self.epoch != Some(epoch) {
            self.latch(Failure::DeviceEpoch);
            return Err(Failure::DeviceEpoch);
        }
        if self.topology != Some(topology) || topology.check() != Verdict::SimulatedTopologyEligible
        {
            let why = Self::topology_reason(topology);
            self.latch(why);
            return Err(why);
        }
        if let Some((prev, id)) = self.last_poll {
            let fault = if stamp < prev {
                Some(Failure::ClockRollback)
            } else if stamp == prev {
                Some(Failure::ReplayedPoll)
            } else if stamp - prev > POLL_MAX_US {
                Some(Failure::PollTimeout)
            } else if packet_id < id {
                Some(Failure::PacketRollback)
            } else if packet_id == id {
                // Preserve the invariant even when callers skip the
                // higher-level ObservationWatch anti-replay guard.
                Some(Failure::DuplicatePacketCounter)
            } else {
                None
            };
            if let Some(why) = fault {
                self.latch(why);
                return Err(why);
            }
        } else if let Some(initial) = self.armed_at {
            let reason = if stamp < initial {
                Some(Failure::ClockRollback)
            } else if stamp - initial > POLL_MAX_US {
                Some(Failure::PollTimeout)
            } else {
                None
            };
            if let Some(why) = reason {
                self.latch(why);
                return Err(why);
            }
        }
        let wire = XusbWire::encode(packet);
        if wire.decode() != packet {
            self.latch(Failure::UntrustedReport);
            return Err(Failure::UntrustedReport);
        }
        self.last_poll = Some((stamp, packet_id));
        if self.staged.len() == MAX_STAGED {
            self.staged.pop_front();
        }
        self.staged.push_back(wire);
        self.accepted += 1;
        Ok(())
    }
    pub fn watchdog(&mut self, stamp: u64) -> bool {
        if self.state != State::Shadow {
            return false;
        }
        let Some(previous) = self.last_poll.map(|(stamp, _)| stamp).or(self.armed_at) else {
            return false;
        };
        if stamp < previous {
            self.latch(Failure::ClockRollback);
            return true;
        }
        if stamp - previous > POLL_MAX_US {
            self.latch(Failure::PollTimeout);
            return true;
        }
        false
    }
    pub fn emergency_off(&mut self) {
        self.latch(Failure::Emergency);
    }
    pub fn disconnect(&mut self) {
        self.latch(Failure::DeviceEpoch);
    }
    pub fn operator_reset(&mut self, confirmed: bool) -> bool {
        if !confirmed || self.state != State::LatchedFault {
            return false;
        }
        self.clear();
        self.epoch = None;
        self.topology = None;
        self.last_poll = None;
        self.armed_at = None;
        self.fault = None;
        self.state = State::Off;
        true
    }
}
pub fn fixture_packet(i: usize) -> XboxPacket {
    XboxPacket {
        buttons: (i as u16) ^ 0xA531,
        left_trigger: (i % 256) as u8,
        right_trigger: (255 - i % 256) as u8,
        left_x: ((i % 65536) as i32 - 32768) as i16,
        left_y: ((i * 7 % 65536) as i32 - 32768) as i16,
        right_x: ((i * 13 % 65536) as i32 - 32768) as i16,
        right_y: ((i * 29 % 65536) as i32 - 32768) as i16,
    }
}
pub fn run(n: usize, out: &Path) -> io::Result<()> {
    if !(16384..=1_048_576).contains(&n) || !n.is_multiple_of(4096) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline frames must be 16384..1048576 divisible by 4096",
        ));
    }
    let topology = fixture_topology();
    let mut prod = BackendContract::shipping();
    if prod.arm(1, 1, true, topology) != Err(Failure::NoDriver)
        || prod.submit(fixture_packet(0), 1, 8000, 1, topology) != Err(Failure::NoDriver)
    {
        return Err(io::Error::other(
            "disabled shipping backend accepted output",
        ));
    }
    let mut lab = BackendContract::offline_lab();
    let mut rejected_while_latched = 0u64;
    let mut faults = [0u64; 4];
    for segment in 0..n / 4096 {
        let epoch = segment as u64 + 1;
        if segment > 0 && !lab.operator_reset(true) {
            return Err(io::Error::other("missing operator reset"));
        }
        lab.arm(epoch, epoch, true, topology)
            .map_err(|why| io::Error::other(why.name()))?;
        for j in 0..4096 {
            let pkt = fixture_packet(segment * 4096 + j);
            lab.submit(pkt, epoch, (j as u64 + 1) * 8000, j as u32, topology)
                .map_err(|why| io::Error::other(why.name()))?;
        }
        let last = 32_768_000;
        match segment % 4 {
            0 => {
                let bad = Topology {
                    physical_visible_to_game: Some(true),
                    ..topology
                };
                if lab.submit(fixture_packet(0), epoch, last + 8000, 2048, bad)
                    != Err(Failure::DuplicateVisibility)
                {
                    return Err(io::Error::other("double input accepted"));
                }
            }
            1 => {
                if lab.submit(fixture_packet(0), epoch + 1, last + 8000, 2048, topology)
                    != Err(Failure::DeviceEpoch)
                {
                    return Err(io::Error::other("epoch switch accepted"));
                }
            }
            2 => {
                if !lab.watchdog(last + 50_001) {
                    return Err(io::Error::other("dead connection survived watchdog"));
                }
            }
            _ => {
                lab.emergency_off();
                if lab.last_fault() != Some(Failure::Emergency) {
                    return Err(io::Error::other("emergency did not stop"));
                }
            }
        }
        faults[segment % 4] += 1;
        if lab.retained() != 0 || lab.state() != State::LatchedFault {
            return Err(io::Error::other("stale virtual report survived fault"));
        }
        if lab.arm(epoch + 1, epoch + 1, true, topology) != Err(Failure::MustReset) {
            return Err(io::Error::other("implicit rearm allowed"));
        }
        rejected_while_latched += 1;
    }
    if faults.contains(&0) {
        return Err(io::Error::other("incomplete injected fault coverage"));
    }
    if !lab.operator_reset(true) {
        return Err(io::Error::other("final reset denied"));
    }
    if lab.arm((n / 4096) as u64, (n / 4096) as u64, true, topology)
        != Err(Failure::ReplayAuthorization)
    {
        return Err(io::Error::other("token replay accepted"));
    }
    let json = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v43_xusb_style_memory_contract\",",
            "\"frames\":{},\"wire_bytes_per_report\":12,",
            "\"memory_reports_accepted\":{},\"memory_ring_max\":64,",
            "\"retained_after_stop\":{},\"fault_events\":{},",
            "\"collision_faults\":{},\"epoch_faults\":{},",
            "\"watchdog_faults\":{},\"emergency_faults\":{},",
            "\"auto_rearms_blocked\":{},\"replay_token_accepted\":false,",
            "\"shipping_backend_available\":false,",
            "\"wire_abi_verified_against_actual_driver\":false,",
            "\"simulated_topology_not_physical_authorization\":true,",
            "\"physical_controller_present\":false,",
            "\"driver_game_writes\":0,\"production\":\"BLOCKED\"}}\n"
        ),
        n,
        lab.accepted(),
        lab.retained(),
        lab.fault_count(),
        faults[0],
        faults[1],
        faults[2],
        faults[3],
        rejected_while_latched
    );
    crate::truth_lab::save_new(out, &json)?;
    println!("V043: {} synthetic XUSB-style memory reports; {} faults; real driver writes 0; production BLOCKED",
        lab.accepted(),lab.fault_count());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xusb_style_wire_has_byte_exact_endian_roundtrip() {
        let p = XboxPacket {
            buttons: 0xA15C,
            left_trigger: 0x19,
            right_trigger: 0xE4,
            left_x: -32768,
            left_y: 32767,
            right_x: -1234,
            right_y: 1234,
        };
        let bytes = XusbWire::encode(p).bytes();
        assert_eq!(
            bytes,
            [0x5C, 0xA1, 0x19, 0xE4, 0x00, 0x80, 0xFF, 0x7F, 0x2E, 0xFB, 0xD2, 0x04]
        );
        assert_eq!(XusbWire(bytes).decode(), p);
        for i in 0..10000 {
            let p = fixture_packet(i);
            assert_eq!(XusbWire::encode(p).decode(), p);
        }
    }
    #[test]
    fn disabled_backend_does_not_depend_on_simulated_claims() {
        let mut b = BackendContract::shipping();
        let g = fixture_topology();
        assert_eq!(b.arm(1, 1, true, g), Err(Failure::NoDriver));
        assert_eq!(
            b.submit(fixture_packet(0), 1, 8000, 1, g),
            Err(Failure::NoDriver)
        );
        assert_eq!(b.state(), State::Off);
        assert_eq!(b.retained(), 0);
    }
    #[test]
    fn shadow_memory_arm_rejects_consent_and_double_visibility() {
        let mut b = BackendContract::offline_lab();
        let g = fixture_topology();
        assert_eq!(b.arm(1, 1, false, g), Err(Failure::NoConsent));
        assert_eq!(
            b.arm(
                1,
                1,
                true,
                Topology {
                    physical_visible_to_game: Some(true),
                    ..g
                }
            ),
            Err(Failure::DuplicateVisibility)
        );
        assert_eq!(b.state(), State::Off);
        b.arm(1, 1, true, g).unwrap();
        b.submit(fixture_packet(0), 1, 8000, 1, g).unwrap();
        assert_eq!(b.arm(2, 2, true, g), Err(Failure::ActiveAlready));
        assert_eq!(b.state(), State::LatchedFault);
        assert_eq!(b.retained(), 0);
        assert!(!b.operator_reset(false));
        assert!(b.operator_reset(true));
        assert_eq!(b.arm(1, 1, true, g), Err(Failure::ReplayAuthorization));
        b.arm(2, 2, true, g).unwrap();
    }
    #[test]
    fn ring_is_bounded_and_no_old_packets_after_stop() {
        let mut b = BackendContract::offline_lab();
        let g = fixture_topology();
        b.arm(1, 1, true, g).unwrap();
        for i in 0..10_000 {
            b.submit(fixture_packet(i), 1, (i as u64 + 1) * 8000, i as u32, g)
                .unwrap();
        }
        assert_eq!(b.retained(), 64);
        assert_eq!(b.accepted(), 10_000);
        b.emergency_off();
        assert_eq!(b.retained(), 0);
        assert_eq!(b.latest(), None);
        assert_eq!(
            b.submit(fixture_packet(0), 1, 0, 0, g),
            Err(Failure::MustReset)
        );
    }
    #[test]
    fn epoch_clock_packet_and_timeout_changes_latch() {
        let g = fixture_topology();
        for (epoch, time, packet, reason) in [
            (2, 16000, 2, Failure::DeviceEpoch),
            (1, 7000, 2, Failure::ClockRollback),
            (1, 60001, 2, Failure::PollTimeout),
            (1, 16000, 0, Failure::PacketRollback),
        ] {
            let mut b = BackendContract::offline_lab();
            b.arm(1, 1, true, g).unwrap();
            b.submit(fixture_packet(0), 1, 8000, 1, g).unwrap();
            assert_eq!(
                b.submit(fixture_packet(1), epoch, time, packet, g),
                Err(reason)
            );
            assert_eq!(b.retained(), 0);
            assert_eq!(b.state(), State::LatchedFault);
        }
    }
    #[test]
    fn real_pipeline_shadow_packet_roundtrips_into_memory_protocol() {
        use crate::{
            pipeline_v32::Pipeline, profile_store::Stick, reacquire::VerifiedCenter,
            runtime_core::Frame, Vec2,
        };
        let center = VerifiedCenter::restore_checked(
            Vec2::new(0.08, -0.06),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let g = fixture_topology();
        for stick in [Stick::Left, Stick::Right] {
            let mut pipeline = Pipeline::synthetic(g, 1, stick, center);
            let input = Frame {
                monotonic_us: 8000,
                packet: 1,
                left: Vec2::new(0.08, -0.06),
                right: Vec2::new(0.35, -0.12),
                left_trigger: 12,
                right_trigger: 251,
                buttons: 0xA53A,
            };
            let (_, wire_packet) = pipeline.process(input, 1).unwrap();
            let packet = wire_packet.unwrap();
            let mut lab = BackendContract::offline_lab();
            lab.arm(1, 1, true, g).unwrap();
            lab.submit(packet, 1, 8000, 1, g).unwrap();
            let decoded = lab.latest().unwrap().decode();
            assert_eq!(packet, decoded);
            assert_eq!(decoded.buttons, input.buttons);
            assert_eq!(decoded.left_trigger, input.left_trigger);
            assert_eq!(decoded.right_trigger, input.right_trigger);
            lab.disconnect();
            assert_eq!(lab.retained(), 0);
        }
    }
    #[test]
    fn driver_audit_never_overwrites_previous_evidence() {
        let path = std::env::temp_dir().join(format!("ga-v43-{}.json", std::process::id()));
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        run(16384, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(run(16384, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_file(path);
    }
}
