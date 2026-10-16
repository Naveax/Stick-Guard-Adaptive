//! v0.32 end-to-end SOFTWARE packet chain without a Windows output backend.
//! Frames are synthetic; the tested sink is in-memory only.
//! Never treat topology supplied by a test as authenticated real hardware.
use crate::{
    output_adapter::{DisabledBackend, VirtualGamepad, XboxPacket},
    profile_store::Stick,
    reacquire::VerifiedCenter,
    routing_guard::{Topology, Verdict},
    runtime_core::{Core, Frame, Reason, State, Timing},
    Vec2,
};
use std::{io, path::Path, time::Instant};
const COUNTS: usize = 6;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    QuarantinedNoRoute,
    NotArmed,
    PreviewOnly,
    Fault,
}
impl Delivery {
    pub fn name(self) -> &'static str {
        match self {
            Self::QuarantinedNoRoute => "route_unverified_no_delivery",
            Self::NotArmed => "not_armed",
            Self::PreviewOnly => "memory_only_preview",
            Self::Fault => "fault_quarantine",
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct MemoryPackets {
    pub accepted: u64,
    pub last: Option<XboxPacket>,
}
impl MemoryPackets {
    pub fn clear(&mut self) {
        self.last = None;
    }
    pub fn accept(&mut self, p: XboxPacket) {
        self.last = Some(p);
        self.accepted += 1;
    }
}
/// A pure software topology is not permission to write to a real device.
/// This object has no OS handle, no driver or transport argument.
pub struct Pipeline {
    core: Core,
    sink: MemoryPackets,
    topology: Topology,
    expected_epoch: u64,
    pub observed: u64,
    pub rejected: u64,
    pub faulted: u64,
    pub reasons: [u64; COUNTS],
    pub encode_timing: Timing,
}
impl Pipeline {
    pub fn synthetic(topology: Topology, epoch: u64, stick: Stick, center: VerifiedCenter) -> Self {
        let mut core = Core::default();
        // The test-only consent bit can arm an in-memory preview. No code
        // path exposes a game writer regardless of the supplied topology.
        core.arm_stick(center, epoch, topology.explicit_user_consent, stick);
        Self {
            core,
            sink: MemoryPackets::default(),
            topology,
            expected_epoch: epoch,
            observed: 0,
            rejected: 0,
            faulted: 0,
            reasons: [0; COUNTS],
            encode_timing: Timing::default(),
        }
    }
    pub fn update_topology(&mut self, t: Topology) {
        // Any topology change invalidates authorization. Never implicitly
        // re-arm a profile even if the new topology appears valid.
        self.topology = t;
        self.core.emergency_off();
        self.sink.clear();
    }
    pub fn synthetic_rearm(&mut self, center: VerifiedCenter, stick: Stick) -> bool {
        if self.topology.check() != Verdict::SimulatedTopologyEligible {
            return false;
        }
        self.core.arm_stick(
            center,
            self.expected_epoch,
            self.topology.explicit_user_consent,
            stick,
        )
    }
    pub fn emergency_stop(&mut self) {
        self.core.emergency_off();
        self.sink.clear();
    }
    pub fn disconnect(&mut self) {
        self.core.disconnect();
        self.sink.clear();
    }
    pub fn sent(&self) -> u64 {
        self.sink.accepted
    }
    pub fn last_packet(&self) -> Option<XboxPacket> {
        self.sink.last
    }
    pub fn state(&self) -> State {
        self.core.state()
    }
    pub fn process(
        &mut self,
        frame: Frame,
        epoch: u64,
    ) -> io::Result<(Delivery, Option<XboxPacket>)> {
        self.observed += 1;
        if epoch != self.expected_epoch {
            self.core.disconnect();
            self.sink.clear();
            self.expected_epoch = epoch;
        }
        let out = self.core.step(frame, epoch);
        if self.core.state() == State::LatchedFault {
            self.sink.clear();
            self.faulted += 1;
            self.rejected += 1;
            self.reasons[3] += 1;
            return Ok((Delivery::Fault, None));
        }
        if self.topology.check() != Verdict::SimulatedTopologyEligible {
            self.sink.clear();
            self.rejected += 1;
            self.reasons[0] += 1;
            return Ok((Delivery::QuarantinedNoRoute, None));
        }
        if out.reason != Reason::PreviewOnly || self.core.state() != State::Preview {
            self.sink.clear();
            self.rejected += 1;
            self.reasons[1] += 1;
            return Ok((Delivery::NotArmed, None));
        }
        let tick = Instant::now();
        let packet = match XboxPacket::from_shadow(out) {
            Ok(p) => p,
            Err(e) => {
                self.core.emergency_off();
                self.sink.clear();
                self.rejected += 1;
                self.reasons[3] += 1;
                return Err(e);
            }
        };
        self.encode_timing
            .observe(tick.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        self.sink.accept(packet);
        self.reasons[2] += 1;
        Ok((Delivery::PreviewOnly, Some(packet)))
    }
    pub fn manifest(&self, frames: usize) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"release\":\"0.32.0\",",
                "\"source\":\"synthetic_end_to_end_memory_packet_pipeline\",",
                "\"frames_requested\":{},\"frames_seen\":{},",
                "\"in_memory_packets\":{},\"rejected\":{},\"fault_frames\":{},",
                "\"quarantined_no_route\":{},\"not_armed\":{},\"memory_preview\":{},",
                "\"core_timing\":{},\"encoding_timing\":{},",
                "\"route_verdict\":\"{}\",",
                "\"windows_virtual_backend_loaded\":false,",
                "\"real_usb_controller_authenticated\":false,",
                "\"game_input_delivered\":false,",
                "\"driver_calls\":0,",
                "\"physical_latency_verified\":false,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            frames,
            self.observed,
            self.sent(),
            self.rejected,
            self.faulted,
            self.reasons[0],
            self.reasons[1],
            self.reasons[2],
            self.core.timing.json(),
            self.encode_timing.json(),
            self.topology.check().name()
        )
    }
}
fn fixture_topology() -> Topology {
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
fn fixture_center() -> VerifiedCenter {
    VerifiedCenter::restore_checked(Vec2::new(0.08, -0.06), Vec2::new(0.0001, 0.0001), 0.0001)
        .expect("bounded synthetic fixture")
}
#[derive(Clone, Copy, Default)]
pub struct Integration {
    pub left_preview: u64,
    pub right_preview: u64,
    pub left_rejected: u64,
    pub right_rejected: u64,
    pub frames: u64,
    pub backend_send_denials: u64,
    pub left_fault_frames: u64,
    pub right_fault_frames: u64,
    pub equal_non_target_controls: u64,
    pub final_driver_writes: u64,
}
impl Integration {
    pub fn json(&self) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v032_two_stick_synthetic_integration\",",
                "\"left_preview\":{},\"right_preview\":{},",
                "\"left_rejected\":{},\"right_rejected\":{},",
                "\"frames\":{},\"backend_send_denials\":{},",
                "\"left_fault_frames\":{},\"right_fault_frames\":{},",
                "\"untargeted_controls_exact_matches\":{},",
                "\"physical_gamepad_writes\":{},",
                "\"physical_controller_tested\":false,",
                "\"driver_integration_tested\":false,",
                "\"automated_candidate_selection\":false,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.left_preview,
            self.right_preview,
            self.left_rejected,
            self.right_rejected,
            self.frames,
            self.backend_send_denials,
            self.left_fault_frames,
            self.right_fault_frames,
            self.equal_non_target_controls,
            self.final_driver_writes
        )
    }
}
fn frame(i: usize) -> Frame {
    let flick = if (i / 181).is_multiple_of(2) {
        0.3
    } else {
        -0.3
    };
    Frame {
        monotonic_us: (i as u64) * 8_000,
        packet: (i / 2) as u32,
        left: Vec2::new(0.08 + flick, -0.06 + flick * 0.25),
        right: Vec2::new(0.08 - flick, -0.06 - flick * 0.25),
        buttons: (i as u16) ^ 0xC35A,
        left_trigger: (i % 256) as u8,
        right_trigger: (255 - i % 256) as u8,
    }
}
pub fn integrated(count: usize) -> io::Result<Integration> {
    if !(10000..=1_000_000).contains(&count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frames must be 10000..1000000",
        ));
    }
    let mut l = Pipeline::synthetic(fixture_topology(), 1, Stick::Left, fixture_center());
    let mut r = Pipeline::synthetic(fixture_topology(), 1, Stick::Right, fixture_center());
    let mut result = Integration {
        frames: count as u64,
        ..Integration::default()
    };
    for i in 0..count {
        let mut f = frame(i);
        // Route loss, route restoration without rearming, explicit
        // synthetic rearm, and a stale packet injected after rearm.
        if i == count / 4 {
            l.update_topology(Topology {
                physical_visible_to_game: Some(true),
                ..fixture_topology()
            });
            r.update_topology(Topology {
                physical_visible_to_game: Some(true),
                ..fixture_topology()
            });
        }
        if i == count / 2 {
            l.update_topology(fixture_topology());
            r.update_topology(fixture_topology());
        }
        // Restoring topology alone must NOT re-arm the session.
        if i == 3 * count / 4
            && (!l.synthetic_rearm(fixture_center(), Stick::Left)
                || !r.synthetic_rearm(fixture_center(), Stick::Right))
        {
            return Err(io::Error::other("explicit synthetic rearm failed"));
        }
        if i == 7 * count / 8 {
            f.monotonic_us = f.monotonic_us.saturating_add(80_000);
        }
        let (ls, lo) = l.process(f, 1)?;
        let (rs, ro) = r.process(f, 1)?;
        for (mode, packet) in [(ls, lo), (rs, ro)] {
            if let Some(p) = packet {
                if p.buttons != f.buttons
                    || p.left_trigger != f.left_trigger
                    || p.right_trigger != f.right_trigger
                {
                    return Err(io::Error::other("nonaxis controls changed"));
                }
                result.equal_non_target_controls += 1;
                if mode != Delivery::PreviewOnly {
                    return Err(io::Error::other("delivery mode mismatch"));
                }
            }
        }
        if ls == Delivery::Fault {
            result.left_fault_frames += 1;
        }
        if rs == Delivery::Fault {
            result.right_fault_frames += 1;
        }
        // The opposite stick passes through bit-exact after quantization.
        if let Some(p) = lo {
            let raw = XboxPacket::encode(f)?;
            if (p.right_x, p.right_y) != (raw.right_x, raw.right_y) {
                return Err(io::Error::other("right axis corrupted during left preview"));
            }
            result.left_preview += 1;
        } else {
            result.left_rejected += 1;
        }
        if let Some(p) = ro {
            let raw = XboxPacket::encode(f)?;
            if (p.left_x, p.left_y) != (raw.left_x, raw.left_y) {
                return Err(io::Error::other("left axis corrupted during right preview"));
            }
            result.right_preview += 1;
        } else {
            result.right_rejected += 1;
        }
    }
    let mut driver = DisabledBackend;
    if l.last_packet().is_some() || r.last_packet().is_some() {
        return Err(io::Error::other(
            "stale synthetic packet retained after fault",
        ));
    }
    for p in [
        XboxPacket::encode(frame(0))?,
        XboxPacket::encode(frame(count - 1))?,
    ] {
        if driver.send(p).is_ok() {
            result.final_driver_writes += 1;
        } else {
            result.backend_send_denials += 1;
        }
    }
    let _ = driver.deactivate();
    if result.final_driver_writes != 0 {
        return Err(io::Error::other("device written"));
    }
    if result.left_preview == 0
        || result.right_preview == 0
        || result.left_rejected == 0
        || result.left_fault_frames == 0
        || result.right_fault_frames == 0
        || result.backend_send_denials != 2
        || result.left_preview + result.left_rejected != result.frames
        || result.right_preview + result.right_rejected != result.frames
    {
        return Err(io::Error::other("integration coverage missing"));
    }
    Ok(result)
}
pub fn run(frames: usize, out: &Path) -> io::Result<()> {
    let result = integrated(frames)?;
    crate::truth_lab::save_new(out, &result.json())?;
    println!("V032 PIPELINE {frames} frames: left_preview={} right_preview={} rejected_left={} rejected_right={}",
        result.left_preview,result.right_preview,result.left_rejected,result.right_rejected);
    println!(
        "OUTPUT_BACKEND=DISABLED GAME_WRITES=0 PRODUCTION=BLOCKED evidence={}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_sticks_have_in_memory_pipeline_and_blocked_physical_sink() {
        let v = integrated(20000).unwrap();
        assert!(v.left_preview > 0 && v.right_preview > 0);
        assert!(v.left_rejected > 0 && v.right_rejected > 0);
        assert_eq!(v.final_driver_writes, 0);
        assert!(v.json().contains("\"physical_gamepad_writes\":0"));
    }
    #[test]
    fn simulated_routing_failure_blocks_packet_before_encoder() {
        let mut p = Pipeline::synthetic(
            Topology {
                physical_visible_to_game: Some(true),
                ..fixture_topology()
            },
            12,
            Stick::Left,
            fixture_center(),
        );
        let v = p.process(frame(0), 12).unwrap();
        assert_eq!(v, (Delivery::QuarantinedNoRoute, None));
        assert_eq!(p.sent(), 0);
        assert_eq!(p.state(), State::Preview);
        let text = p.manifest(1);
        assert!(text.contains("\"production_gate\":\"BLOCKED\""));
    }
    #[test]
    fn session_epoch_mismatch_does_not_resume_on_new_epoch() {
        let mut p = Pipeline::synthetic(fixture_topology(), 1, Stick::Left, fixture_center());
        assert_eq!(p.process(frame(0), 1).unwrap().0, Delivery::PreviewOnly);
        assert_eq!(p.process(frame(1), 2).unwrap().0, Delivery::NotArmed);
        assert!(p.last_packet().is_none());
        assert_eq!(p.process(frame(2), 2).unwrap().0, Delivery::NotArmed);
        assert_eq!(p.state(), State::Off);
    }
    #[test]
    fn stale_packet_fault_latches_raw_and_never_encodes_it() {
        let mut p = Pipeline::synthetic(fixture_topology(), 1, Stick::Left, fixture_center());
        assert_eq!(p.process(frame(0), 1).unwrap().0, Delivery::PreviewOnly);
        let mut delayed = frame(1);
        delayed.monotonic_us = 100_000;
        assert_eq!(p.process(delayed, 1).unwrap().0, Delivery::Fault);
        assert!(p.last_packet().is_none());
        assert_eq!(p.process(frame(2), 1).unwrap().0, Delivery::Fault);
    }
    #[test]
    fn untrusted_topology_cannot_enable_device_backend() {
        let mut p = Pipeline::synthetic(fixture_topology(), 1, Stick::Left, fixture_center());
        let _ = p.process(frame(0), 1).unwrap();
        p.update_topology(fixture_topology());
        assert!(p.last_packet().is_none());
        assert_eq!(p.process(frame(1), 1).unwrap().0, Delivery::NotArmed);
        p.emergency_stop();
        assert_eq!(p.process(frame(2), 1).unwrap().0, Delivery::NotArmed);
        assert_eq!(p.sent(), 1);
    }
}
