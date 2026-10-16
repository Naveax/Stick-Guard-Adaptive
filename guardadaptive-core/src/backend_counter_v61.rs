//! v0.61: defense in depth at the OFFLINE BackendContract boundary.
//! Verifies counter uniqueness even if ObservationWatch is bypassed.
//! No Windows driver, HID IO or physical input is touched.
use crate::{
    driver_contract_v43::{fixture_packet, BackendContract, Failure, State},
    output_supervisor::fixture_topology,
};
use std::{io, path::Path};

const CASES: usize = 8;
const MAX_SAMPLES: usize = 65_536;
fn check(ok: bool, reason: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(reason))
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Results {
    pub samples: usize,
    pub duplicate_id_faults: usize,
    pub rollback_id_faults: usize,
    pub replayed_timestamp_faults: usize,
    pub clock_faults: usize,
    pub timeout_faults: usize,
    pub epoch_faults: usize,
    pub confirmed_rearm_proofs: usize,
    pub shipping_denials: usize,
}
pub fn audit(samples: usize) -> io::Result<Results> {
    if !(CASES..=MAX_SAMPLES).contains(&samples) || !samples.is_multiple_of(CASES) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples 8..65536 and multiple of 8",
        ));
    }
    let topology = fixture_topology();
    let mut out = Results {
        samples,
        duplicate_id_faults: 0,
        rollback_id_faults: 0,
        replayed_timestamp_faults: 0,
        clock_faults: 0,
        timeout_faults: 0,
        epoch_faults: 0,
        confirmed_rearm_proofs: 0,
        shipping_denials: 0,
    };
    for i in 0..samples {
        let epoch = i as u64 + 1;
        let mut b = BackendContract::offline_lab();
        let mut shipping = BackendContract::shipping();
        check(
            shipping.arm_at(epoch, epoch, true, topology, 0) == Err(Failure::NoDriver)
                && shipping.submit(fixture_packet(0), epoch, 1000, 1, topology)
                    == Err(Failure::NoDriver)
                && shipping.latest().is_none(),
            "shipping accepted memory driver output",
        )?;
        out.shipping_denials += 1;
        b.arm_at(epoch, epoch, true, topology, 0)
            .map_err(|e| io::Error::other(e.name()))?;
        b.submit(fixture_packet(0), epoch, 1000, 1, topology)
            .map_err(|e| io::Error::other(e.name()))?;
        b.submit(fixture_packet(1), epoch, 2000, 2, topology)
            .map_err(|e| io::Error::other(e.name()))?;
        let which = i % CASES;
        let (fault, actual) = match which {
            0 => (
                Failure::DuplicatePacketCounter,
                b.submit(fixture_packet(2), epoch, 3000, 2, topology),
            ),
            1 => (
                Failure::PacketRollback,
                b.submit(fixture_packet(2), epoch, 3000, 1, topology),
            ),
            2 => (
                Failure::ReplayedPoll,
                b.submit(fixture_packet(2), epoch, 2000, 2, topology),
            ),
            3 => (
                Failure::ClockRollback,
                b.submit(fixture_packet(2), epoch, 1999, 3, topology),
            ),
            4 => (
                Failure::PollTimeout,
                b.submit(fixture_packet(2), epoch, 52_001, 3, topology),
            ),
            5 => (
                Failure::DeviceEpoch,
                b.submit(fixture_packet(2), epoch + 1, 3000, 3, topology),
            ),
            6 => {
                b.submit(fixture_packet(2), epoch, 3000, 3, topology)
                    .map_err(|e| io::Error::other(e.name()))?;
                b.submit(fixture_packet(3), epoch, 4000, 4, topology)
                    .map_err(|e| io::Error::other(e.name()))?;
                (
                    Failure::DuplicatePacketCounter,
                    b.submit(fixture_packet(4), epoch, 5000, 4, topology),
                )
            }
            7 => {
                b.submit(fixture_packet(2), epoch, 3000, u32::MAX, topology)
                    .map_err(|e| io::Error::other(e.name()))?;
                // A counter cannot wrap when more memory reports are sent.
                (
                    Failure::PacketRollback,
                    b.submit(fixture_packet(3), epoch, 4000, 0, topology),
                )
            }
            _ => unreachable!(),
        };
        check(actual == Err(fault), "wrong direct-backend fault reason")?;
        check(
            b.state() == State::LatchedFault
                && b.retained() == 0
                && b.latest().is_none()
                && b.last_fault() == Some(fault),
            "direct backend replay failed to latch and clear memory",
        )?;
        check(
            b.submit(fixture_packet(4), epoch, 6000, 8, topology) == Err(Failure::MustReset),
            "staged a packet after fault",
        )?;
        check(
            !b.operator_reset(false)
                && b.arm_at(epoch + 1, epoch + 1, true, topology, 6000) == Err(Failure::MustReset),
            "unauthorized automatic reset",
        )?;
        check(b.operator_reset(true), "confirmed backend reset failed")?;
        check(
            b.arm_at(epoch, epoch + 1, true, topology, 7000) == Err(Failure::ReplayAuthorization),
            "replayed auth token accepted",
        )?;
        b.arm_at(epoch + 1, epoch + 1, true, topology, 7000)
            .map_err(|e| io::Error::other(e.name()))?;
        check(b.retained() == 0, "stale ring carried across epochs")?;
        // Newly confirmed authorization may restart its packet counter at 0.
        b.submit(fixture_packet(0), epoch + 1, 8000, 0, topology)
            .map_err(|e| io::Error::other(e.name()))?;
        check(b.retained() == 1, "fresh epoch could not stage memory only")?;
        out.confirmed_rearm_proofs += 1;
        match fault {
            Failure::DuplicatePacketCounter => out.duplicate_id_faults += 1,
            Failure::PacketRollback => out.rollback_id_faults += 1,
            Failure::ReplayedPoll => out.replayed_timestamp_faults += 1,
            Failure::ClockRollback => out.clock_faults += 1,
            Failure::PollTimeout => out.timeout_faults += 1,
            Failure::DeviceEpoch => out.epoch_faults += 1,
            _ => return Err(io::Error::other("unexpected classification")),
        }
    }
    check(
        out.duplicate_id_faults > 0
            && out.rollback_id_faults > 0
            && out.replayed_timestamp_faults > 0
            && out.clock_faults > 0
            && out.timeout_faults > 0
            && out.epoch_faults > 0,
        "incomplete duplicate packet counter fault matrix",
    )?;
    Ok(out)
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let v = audit(samples)?;
    let result = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v61_backend_direct_packet_counter\",",
            "\"synthetic_cases\":{},\"duplicate_packet_counter_denied\":{},",
            "\"rollback_counter_denied\":{},\"same_timestamp_denied\":{},",
            "\"clock_rollback_denied\":{},\"heartbeat_gap_denied\":{},",
            "\"wrong_epoch_denied\":{},\"confirmed_new_epoch_rearms\":{},",
            "\"shipping_driver_absent_denials\":{},",
            "\"backend_counter_strictly_increasing\":true,",
            "\"duplicate_packet_counter_latches_memory_clear\":true,",
            "\"zero_based_counter_reset_requires_new_authorization\":true,",
            "\"bypassed_observation_watcher_tested\":true,",
            "\"memory_only\":true,\"real_provider_verified\":false,",
            "\"real_driver_abi_verified\":false,\"driver_installs\":0,",
            "\"driver_writes\":0,\"game_input_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        v.samples,
        v.duplicate_id_faults,
        v.rollback_id_faults,
        v.replayed_timestamp_faults,
        v.clock_faults,
        v.timeout_faults,
        v.epoch_faults,
        v.confirmed_rearm_proofs,
        v.shipping_denials
    );
    crate::truth_lab::save_new(out, &result)?;
    println!("V061_BACKEND_COUNTER=PASS cases={} duplicates={} zero_real_driver_writes=true production=BLOCKED",
        samples,v.duplicate_id_faults);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_and_counter_wrap_are_rejected_by_direct_backend() {
        let x = audit(8192).unwrap();
        assert_eq!(x.samples, 8192);
        assert_eq!(x.duplicate_id_faults, 2048);
        assert_eq!(x.rollback_id_faults, 2048);
        assert_eq!(x.replayed_timestamp_faults, 1024);
        assert_eq!(x.clock_faults, 1024);
        assert_eq!(x.timeout_faults, 1024);
        assert_eq!(x.epoch_faults, 1024);
        assert_eq!(x.confirmed_rearm_proofs, 8192);
    }
    #[test]
    fn invalid_case_counts_are_rejected() {
        for n in [0, 1, 7, 9, 8191, MAX_SAMPLES + 1] {
            assert!(audit(n).is_err());
        }
    }
    #[test]
    fn evidence_is_create_new_only() {
        let folder = std::env::temp_dir().join(format!("ga-v61-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("receipt.json");
        if file.exists() {
            std::fs::remove_file(&file).unwrap();
        }
        run(8, &file).unwrap();
        let first = std::fs::read(&file).unwrap();
        assert!(run(8, &file).is_err());
        assert_eq!(first, std::fs::read(&file).unwrap());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
