//! v0.54 deterministic OFFLINE observation-change monitor.
//! No Win32 driver handles, no PnP polling, no hardware IO and NO authority.
//! Untrusted synthetic device/service claims gate a MEMORY-ONLY backend.
use crate::{
    driver_contract_v43::{fixture_packet, BackendContract, Failure, State},
    output_supervisor::fixture_topology,
    routing_guard::Topology,
};
use std::{io, path::Path};

const MAX_GAP_US: u64 = 50_000;
const SEGMENT: usize = 1024;
const SCENARIOS: usize = 9;
const MAX_SAMPLES: usize = SCENARIOS * SEGMENT * 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub sequence: u64,
    pub stamp_us: u64,
    pub instance_tag: u64,
    pub service_tag: u64,
    pub binary_tag: u64,
    pub device_present: bool,
    pub signed_claim: bool,
    pub unique_instance: bool,
    pub service_present: bool,
    pub service_running: bool,
}
impl Observation {
    pub fn synthetic(sequence: u64, stamp_us: u64) -> Self {
        Self {
            sequence,
            stamp_us,
            instance_tag: 11,
            service_tag: 22,
            binary_tag: 33,
            device_present: true,
            signed_claim: true,
            unique_instance: true,
            service_present: true,
            service_running: true,
        }
    }
    fn eligible(self) -> bool {
        self.device_present
            && self.signed_claim
            && self.unique_instance
            && self.service_present
            && self.service_running
            && self.instance_tag != 0
            && self.service_tag != 0
            && self.binary_tag != 0
    }
    fn identity(self) -> (u64, u64, u64) {
        (self.instance_tag, self.service_tag, self.binary_tag)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchState {
    Off,
    WatchingMemoryOnly,
    LatchedFault,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchFault {
    Unready,
    RequiresOperatorReset,
    EpochReplay,
    CounterExhausted,
    StaleSubmission,
    DuplicateSubmission,
    FrameSequenceRollback,
    FrameIndexOverflow,
    DeviceGone,
    ServiceStopped,
    UnsignedClaim,
    DuplicateDevice,
    MissingService,
    IdentityChanged,
    SnapshotReplay,
    ClockRollback,
    HeartbeatGap,
    Backend,
}
impl WatchFault {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unready => "unready_synthetic_provider",
            Self::RequiresOperatorReset => "operator_reset_required",
            Self::EpochReplay => "stale_or_replayed_device_epoch",
            Self::CounterExhausted => "token_or_epoch_counter_at_u64_limit",
            Self::StaleSubmission => "submit_observation_stamp_does_not_match_latest",
            Self::DuplicateSubmission => "same_observation_submitted_twice",
            Self::FrameSequenceRollback => "report_frame_index_replayed_or_rolled_back",
            Self::FrameIndexOverflow => "frame_index_does_not_fit_wire_packet_counter",
            Self::DeviceGone => "device_disappeared",
            Self::ServiceStopped => "service_stopped",
            Self::UnsignedClaim => "unsigned_claim",
            Self::DuplicateDevice => "ambiguous_device_id",
            Self::MissingService => "service_missing",
            Self::IdentityChanged => "instance_service_or_binary_changed",
            Self::SnapshotReplay => "replayed_observation_sequence",
            Self::ClockRollback => "observation_clock_rollback",
            Self::HeartbeatGap => "missing_observation_over_50ms",
            Self::Backend => "offline_backend_rejected",
        }
    }
}
pub struct ObservationWatch {
    backend: BackendContract,
    state: WatchState,
    reason: Option<WatchFault>,
    fingerprint: Option<(u64, u64, u64)>,
    last_sequence: Option<u64>,
    last_stamp: Option<u64>,
    // Monotonic caller-observed time: includes submit and heartbeat checks.
    // This is synthetic and NOT independently attested to a host clock.
    last_checked_now: Option<u64>,
    // Exactly one report per observed sequence; frame indices must increase
    // without narrowing from usize to the u32 in-memory packet counter.
    last_submitted_observation_sequence: Option<u64>,
    last_submitted_frame: Option<usize>,
    observations: u64,
}
impl ObservationWatch {
    pub fn offline_lab() -> Self {
        Self {
            backend: BackendContract::offline_lab(),
            state: WatchState::Off,
            reason: None,
            fingerprint: None,
            last_sequence: None,
            last_stamp: None,
            last_checked_now: None,
            last_submitted_observation_sequence: None,
            last_submitted_frame: None,
            observations: 0,
        }
    }
    pub fn state(&self) -> WatchState {
        self.state
    }
    pub fn last_fault(&self) -> Option<WatchFault> {
        self.reason
    }
    pub fn retained(&self) -> usize {
        self.backend.retained()
    }
    pub fn accepted(&self) -> u64 {
        self.backend.accepted()
    }
    pub fn observations(&self) -> u64 {
        self.observations
    }
    pub fn backend_state(&self) -> State {
        self.backend.state()
    }
    pub fn latest_available(&self) -> bool {
        self.backend.latest().is_some()
    }
    pub fn begin(
        &mut self,
        token: u64,
        epoch: u64,
        observation: Observation,
        topology: Topology,
    ) -> Result<(), WatchFault> {
        if self.state == WatchState::LatchedFault {
            return Err(WatchFault::RequiresOperatorReset);
        }
        if self.state != WatchState::Off {
            self.latch(WatchFault::Backend);
            return Err(WatchFault::Backend);
        }
        if !observation.eligible() {
            return Err(WatchFault::Unready);
        }
        if self
            .last_sequence
            .is_some_and(|old| observation.sequence <= old)
        {
            return Err(WatchFault::SnapshotReplay);
        }
        if self
            .last_checked_now
            .or(self.last_stamp)
            .is_some_and(|old| observation.stamp_us <= old)
        {
            return Err(WatchFault::ClockRollback);
        }
        self.backend
            .arm_at(token, epoch, true, topology, observation.stamp_us)
            .map_err(|failure| match failure {
                Failure::EpochReplay => WatchFault::EpochReplay,
                Failure::CounterExhausted => WatchFault::CounterExhausted,
                _ => WatchFault::Backend,
            })?;
        self.fingerprint = Some(observation.identity());
        self.last_sequence = Some(observation.sequence);
        self.last_stamp = Some(observation.stamp_us);
        self.last_checked_now = Some(observation.stamp_us);
        self.state = WatchState::WatchingMemoryOnly;
        self.last_submitted_frame = None;
        self.reason = None;
        self.observations += 1;
        Ok(())
    }
    // A poisoned concurrent wrapper must not recover an ambiguous partial
    // operation as a usable provider. This only clears an in-memory ring.
    pub(crate) fn lock_poison_fail_closed(&mut self) {
        if self.state != WatchState::LatchedFault {
            self.latch(WatchFault::Backend);
        }
    }
    fn latch(&mut self, fault: WatchFault) {
        if self.state != WatchState::LatchedFault {
            self.backend.disconnect();
            self.state = WatchState::LatchedFault;
            self.reason = Some(fault);
            self.fingerprint = None;
        }
    }
    pub fn observe(&mut self, observation: Observation) -> Result<(), WatchFault> {
        if self.state != WatchState::WatchingMemoryOnly {
            return Err(WatchFault::RequiresOperatorReset);
        }
        let old_seq = self.last_sequence.expect("watch has sequence");
        let old_stamp = self.last_stamp.expect("watch has timestamp");
        let fault = if observation.sequence <= old_seq {
            Some(WatchFault::SnapshotReplay)
        } else if observation.stamp_us <= old_stamp
            || self
                .last_checked_now
                .is_some_and(|now| observation.stamp_us < now)
        {
            Some(WatchFault::ClockRollback)
        } else if observation.stamp_us - old_stamp > MAX_GAP_US {
            Some(WatchFault::HeartbeatGap)
        } else if !observation.device_present {
            Some(WatchFault::DeviceGone)
        } else if !observation.unique_instance {
            Some(WatchFault::DuplicateDevice)
        } else if !observation.signed_claim {
            Some(WatchFault::UnsignedClaim)
        } else if !observation.service_present {
            Some(WatchFault::MissingService)
        } else if !observation.service_running {
            Some(WatchFault::ServiceStopped)
        } else if !observation.eligible() || self.fingerprint != Some(observation.identity()) {
            Some(WatchFault::IdentityChanged)
        } else {
            None
        };
        if let Some(reason) = fault {
            self.last_sequence = Some(self.last_sequence.unwrap().max(observation.sequence));
            self.last_stamp = Some(self.last_stamp.unwrap().max(observation.stamp_us));
            self.last_checked_now = Some(self.last_checked_now.unwrap().max(observation.stamp_us));
            self.latch(reason);
            return Err(reason);
        }
        self.last_sequence = Some(observation.sequence);
        self.last_stamp = Some(observation.stamp_us);
        self.last_checked_now = Some(self.last_checked_now.unwrap().max(observation.stamp_us));
        self.observations += 1;
        Ok(())
    }
    pub fn heartbeat(&mut self, now_us: u64) -> bool {
        if self.state != WatchState::WatchingMemoryOnly {
            return false;
        }
        let last = self.last_stamp.expect("watch has timestamp");
        if now_us < last || self.last_checked_now.is_some_and(|old| now_us < old) {
            self.latch(WatchFault::ClockRollback);
            return true;
        }
        if now_us - last > MAX_GAP_US {
            self.last_checked_now = Some(now_us);
            self.latch(WatchFault::HeartbeatGap);
            return true;
        }
        self.last_checked_now = Some(now_us);
        false
    }
    pub fn submit_memory(
        &mut self,
        frame: usize,
        epoch: u64,
        stamp_us: u64,
        now_us: u64,
        topology: Topology,
    ) -> Result<(), WatchFault> {
        if self.state != WatchState::WatchingMemoryOnly {
            return Err(WatchFault::RequiresOperatorReset);
        }
        if self.last_stamp != Some(stamp_us) {
            self.latch(WatchFault::StaleSubmission);
            return Err(WatchFault::StaleSubmission);
        }
        let sequence = self.last_sequence.expect("watch has observation sequence");
        if self.last_submitted_observation_sequence == Some(sequence) {
            self.latch(WatchFault::DuplicateSubmission);
            return Err(WatchFault::DuplicateSubmission);
        }
        if self
            .last_submitted_frame
            .is_some_and(|prior| frame <= prior)
        {
            self.latch(WatchFault::FrameSequenceRollback);
            return Err(WatchFault::FrameSequenceRollback);
        }
        let packet_id = match u32::try_from(frame) {
            Ok(value) => value,
            Err(_) => {
                self.latch(WatchFault::FrameIndexOverflow);
                return Err(WatchFault::FrameIndexOverflow);
            }
        };
        let watermark = self
            .last_checked_now
            .expect("watch has monotonic time watermark");
        if now_us < stamp_us || now_us < watermark {
            self.latch(WatchFault::ClockRollback);
            return Err(WatchFault::ClockRollback);
        }
        if now_us - stamp_us > MAX_GAP_US {
            self.last_checked_now = Some(now_us);
            self.latch(WatchFault::HeartbeatGap);
            return Err(WatchFault::HeartbeatGap);
        }
        self.last_checked_now = Some(now_us);
        if self
            .backend
            .submit(fixture_packet(frame), epoch, stamp_us, packet_id, topology)
            .is_err()
        {
            self.latch(WatchFault::Backend);
            return Err(WatchFault::Backend);
        }
        self.last_submitted_observation_sequence = Some(sequence);
        self.last_submitted_frame = Some(frame);
        Ok(())
    }
    pub fn operator_reset(&mut self, confirmed: bool) -> bool {
        if !confirmed || self.state != WatchState::LatchedFault {
            return false;
        }
        if !self.backend.operator_reset(true) {
            return false;
        }
        self.fingerprint = None;
        self.reason = None;
        self.state = WatchState::Off;
        // Sequence/time watermarks intentionally persist across reset.
        true
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub total_frames: usize,
    pub accepted_memory_frames: u64,
    pub observations: u64,
    pub faults: [u64; SCENARIOS],
    pub blocked_automatic_rearms: u64,
    pub blocked_stale_submits: u64,
}
fn ensure(ok: bool, msg: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(msg))
    }
}
/// Deterministic synthetic transition matrix; every staged packet stays in
/// a bounded in-process memory ring, NEVER in the Windows/virtual driver.
pub fn audit(samples: usize) -> io::Result<Evidence> {
    if !(SCENARIOS * SEGMENT..=MAX_SAMPLES).contains(&samples)
        || !samples.is_multiple_of(SCENARIOS * SEGMENT)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be a multiple of 9216 in the range 9216..589824",
        ));
    }
    let topology = fixture_topology();
    let mut shipping = BackendContract::shipping();
    ensure(
        shipping.arm_at(1, 1, true, topology, 0) == Err(Failure::NoDriver)
            && shipping.submit(fixture_packet(0), 1, 1000, 0, topology) == Err(Failure::NoDriver)
            && shipping.latest().is_none(),
        "shipping transport no longer fail-closed",
    )?;
    let mut watch = ObservationWatch::offline_lab();
    let mut sequence = 1_u64;
    let mut faults = [0_u64; SCENARIOS];
    let mut rearm_blocked = 0_u64;
    let mut stale_submit_blocked = 0_u64;
    for cycle in 0..samples / SEGMENT {
        let epoch = cycle as u64 + 1;
        let base = cycle as u64 * 2_000_000;
        sequence += 1;
        let initial = Observation::synthetic(sequence, base);
        watch
            .begin(epoch, epoch, initial, topology)
            .map_err(|f| io::Error::other(f.name()))?;
        ensure(
            watch.retained() == 0 && !watch.latest_available(),
            "stale memory report carried into next authorization epoch",
        )?;
        let mut last = initial;
        for frame in 0..SEGMENT {
            sequence += 1;
            last = Observation::synthetic(sequence, base + (frame as u64 + 1) * 1000);
            watch
                .observe(last)
                .map_err(|f| io::Error::other(f.name()))?;
            watch
                .submit_memory(frame, epoch, last.stamp_us, last.stamp_us, topology)
                .map_err(|f| io::Error::other(f.name()))?;
        }
        ensure(
            watch.retained() == 64 && watch.latest_available(),
            "bounded in-memory report retention failed",
        )?;
        let scenario = cycle % SCENARIOS;
        let expected = match scenario {
            0 => WatchFault::DeviceGone,
            1 => WatchFault::ServiceStopped,
            2 => WatchFault::UnsignedClaim,
            3 => WatchFault::DuplicateDevice,
            4 => WatchFault::MissingService,
            5 => WatchFault::IdentityChanged,
            6 => WatchFault::SnapshotReplay,
            7 => WatchFault::ClockRollback,
            _ => WatchFault::HeartbeatGap,
        };
        let result = if scenario == 8 {
            ensure(
                watch.heartbeat(last.stamp_us + MAX_GAP_US + 1),
                "missed snapshot failed to trigger watchdog",
            )?;
            Err(WatchFault::HeartbeatGap)
        } else {
            let mut changed = last;
            changed.sequence += 1;
            changed.stamp_us += 1000;
            match scenario {
                0 => changed.device_present = false,
                1 => changed.service_running = false,
                2 => changed.signed_claim = false,
                3 => changed.unique_instance = false,
                4 => changed.service_present = false,
                5 => changed.binary_tag = 9000 + epoch,
                6 => changed.sequence = last.sequence,
                7 => changed.stamp_us = last.stamp_us - 1,
                _ => unreachable!(),
            }
            sequence = sequence.max(changed.sequence);
            watch.observe(changed)
        };
        ensure(result == Err(expected), "fault scenario not classified")?;
        faults[scenario] += 1;
        ensure(
            watch.state() == WatchState::LatchedFault
                && watch.backend_state() == State::LatchedFault
                && watch.retained() == 0
                && !watch.latest_available()
                && watch.last_fault() == Some(expected),
            "fault failed to clear memory and latch",
        )?;
        ensure(
            watch.begin(epoch + 100, epoch + 1, initial, topology)
                == Err(WatchFault::RequiresOperatorReset),
            "automatic provider recovery accepted",
        )?;
        rearm_blocked += 1;
        ensure(
            watch.submit_memory(0, epoch, last.stamp_us, last.stamp_us, topology)
                == Err(WatchFault::RequiresOperatorReset),
            "old report submitted after fault",
        )?;
        stale_submit_blocked += 1;
        ensure(!watch.operator_reset(false), "unconfirmed reset accepted")?;
        ensure(watch.operator_reset(true), "confirmed reset rejected")?;
        ensure(
            watch.begin(epoch, epoch + 1, initial, topology).is_err(),
            "stale token or observation was accepted",
        )?;
    }
    ensure(
        watch.accepted() == samples as u64
            && faults.iter().all(|x| *x > 0)
            && watch.backend_state() == State::Off,
        "aggregate memory and reset invariants not met",
    )?;
    Ok(Evidence {
        total_frames: samples,
        accepted_memory_frames: watch.accepted(),
        observations: watch.observations(),
        faults,
        blocked_automatic_rearms: rearm_blocked,
        blocked_stale_submits: stale_submit_blocked,
    })
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let proof = audit(samples)?;
    let mut body = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v54_offline_provider_transition_matrix\",",
            "\"synthetic_frames\":{},\"memory_reports_accepted\":{},\"observations\":{},",
            "\"fault_scenarios\":9,\"injected_fault_events\":{},",
            "\"blocked_automatic_rearms\":{},\"blocked_stale_submits\":{},",
            "\"fault_counts\":["
        ),
        proof.total_frames,
        proof.accepted_memory_frames,
        proof.observations,
        proof.faults.iter().sum::<u64>(),
        proof.blocked_automatic_rearms,
        proof.blocked_stale_submits
    );
    for (i, count) in proof.faults.iter().enumerate() {
        if i != 0 {
            body.push(',');
        }
        body.push_str(&count.to_string());
    }
    body.push_str(
        "],\"device_reappearance_requires_new_confirmed_operator_reset\":true,\
         \"sequence_and_clock_watermarks_persist_across_reset\":true,\
         \"offline_untrusted_metadata_only\":true,\"real_pnp_inventory_examined\":false,\
         \"driver_authorized\":false,\"driver_abi_verified\":false,\
         \"game_output_enabled\":false,\"driver_writes\":0,\"game_input_writes\":0,\
         \"production_gate\":\"BLOCKED\"}\n",
    );
    crate::truth_lab::save_new(out, &body)?;
    println!(
        "V054: {} synthetic memory reports; {} provider faults; real game writes 0; production BLOCKED",
        proof.accepted_memory_frames,
        proof.faults.iter().sum::<u64>()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_offline_transition_matrix_and_watermark_recovery() {
        let result = audit(SCENARIOS * SEGMENT * 4).unwrap();
        assert_eq!(result.total_frames, 36864);
        assert_eq!(result.faults, [4; SCENARIOS]);
        assert_eq!(result.accepted_memory_frames, 36864);
        assert_eq!(result.blocked_automatic_rearms, 36);
        assert_eq!(result.blocked_stale_submits, 36);
        assert!(result.observations >= 36864);
    }
    #[test]
    fn malformed_sample_counts_rejected() {
        for samples in [0, 1024, 9215, 9217, 590_000, 9216 * 65] {
            assert!(audit(samples).is_err(), "{samples}");
        }
    }
    #[test]
    fn missing_provider_never_stages_first_report_even_when_it_reappears() {
        let g = fixture_topology();
        let mut watch = ObservationWatch::offline_lab();
        let mut o = Observation::synthetic(1, 100);
        o.device_present = false;
        assert_eq!(watch.begin(1, 1, o, g), Err(WatchFault::Unready));
        assert_eq!(watch.state(), WatchState::Off);
        assert_eq!(watch.backend_state(), State::Off);
        o.device_present = true;
        o.sequence = 2;
        o.stamp_us = 200;
        watch.begin(1, 1, o, g).unwrap();
        o.sequence = 3;
        o.stamp_us = 1200;
        watch.observe(o).unwrap();
        watch
            .submit_memory(0, 1, o.stamp_us, o.stamp_us, g)
            .unwrap();
        o.device_present = false;
        o.sequence = 4;
        o.stamp_us = 2200;
        assert_eq!(watch.observe(o), Err(WatchFault::DeviceGone));
        assert_eq!(watch.retained(), 0);
        o.device_present = true;
        o.sequence = 5;
        o.stamp_us = 3200;
        assert_eq!(watch.observe(o), Err(WatchFault::RequiresOperatorReset));
        assert_eq!(
            watch.begin(2, 2, o, g),
            Err(WatchFault::RequiresOperatorReset)
        );
        assert!(!watch.operator_reset(false));
        assert!(watch.operator_reset(true));
        assert!(watch.begin(1, 2, o, g).is_err());
        watch.begin(2, 2, o, g).unwrap();
        assert_eq!(watch.retained(), 0);
    }
    #[test]
    fn service_restart_and_image_swaps_never_auto_reauthorize() {
        let g = fixture_topology();
        let mut w = ObservationWatch::offline_lab();
        let a = Observation::synthetic(1, 100);
        w.begin(1, 1, a, g).unwrap();
        let mut b = Observation::synthetic(2, 1100);
        b.service_running = false;
        assert_eq!(w.observe(b), Err(WatchFault::ServiceStopped));
        b.service_running = true;
        b.sequence = 3;
        b.stamp_us = 2100;
        assert!(w.begin(2, 2, b, g).is_err());
        assert!(w.operator_reset(true));
        w.begin(2, 2, b, g).unwrap();
        let mut c = Observation::synthetic(4, 3100);
        c.binary_tag = 999;
        assert_eq!(w.observe(c), Err(WatchFault::IdentityChanged));
        assert!(!w.latest_available());
    }
    #[test]
    fn heartbeat_and_clock_rollback_fail_closed_at_boundaries() {
        let g = fixture_topology();
        let mut w = ObservationWatch::offline_lab();
        w.begin(1, 1, Observation::synthetic(1, 100), g).unwrap();
        assert!(!w.heartbeat(50_100));
        assert!(w.heartbeat(50_101));
        assert_eq!(w.last_fault(), Some(WatchFault::HeartbeatGap));
        assert!(w.operator_reset(true));
        w.begin(2, 2, Observation::synthetic(2, 50_200), g).unwrap();
        assert!(w.heartbeat(50_199));
        assert_eq!(w.last_fault(), Some(WatchFault::ClockRollback));
    }
    #[test]
    fn evidence_created_once_without_overwrite() {
        let folder = std::env::temp_dir().join(format!("ga-v54-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let out = folder.join("v54.json");
        if out.exists() {
            std::fs::remove_file(&out).unwrap();
        }
        run(9216, &out).unwrap();
        let first = std::fs::read(&out).unwrap();
        assert!(run(9216, &out).is_err());
        assert_eq!(first, std::fs::read(&out).unwrap());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
