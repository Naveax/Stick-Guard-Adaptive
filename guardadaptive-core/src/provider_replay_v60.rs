//! v0.60 memory-only stale/duplicate report and checked frame-counter suite.
//! This module never opens Windows devices or transmits game inputs.
use crate::{
    output_supervisor::fixture_topology,
    provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState},
};
use std::{io, path::Path};

const CASES: usize = 8;
const MAX_SAMPLES: usize = 65_536;

fn ensure(condition: bool, message: &'static str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayEvidence {
    pub tested: usize,
    pub stale_stamp: usize,
    pub duplicate_observation: usize,
    pub frame_sequence: usize,
    pub overflow: usize,
    pub timeout: usize,
    pub safe_rearms: usize,
    pub stale_rearms_denied: usize,
    pub latched_and_cleared: usize,
}
/// Eight reproducible synthetic cases, one disposable memory-only backend per
/// case. "Successful" packets below are staged only into an in-process ring.
pub fn audit(samples: usize) -> io::Result<ReplayEvidence> {
    if !(CASES..=MAX_SAMPLES).contains(&samples) || !samples.is_multiple_of(CASES) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "sample count must be a multiple of 8 in 8..65536",
        ));
    }
    let topology = fixture_topology();
    let mut proof = ReplayEvidence {
        tested: samples,
        stale_stamp: 0,
        duplicate_observation: 0,
        frame_sequence: 0,
        overflow: 0,
        timeout: 0,
        safe_rearms: 0,
        stale_rearms_denied: 0,
        latched_and_cleared: 0,
    };
    for i in 0..samples {
        let epoch = i as u64 + 1;
        let base = epoch * 1_000_000;
        let seq = epoch * 10;
        let mut w = ObservationWatch::offline_lab();
        w.begin(epoch, epoch, Observation::synthetic(seq, base), topology)
            .map_err(|f| io::Error::other(f.name()))?;
        w.observe(Observation::synthetic(seq + 1, base + 1000))
            .map_err(|f| io::Error::other(f.name()))?;
        let case = i % CASES;
        let (expected, actual) = match case {
            0 => (
                WatchFault::StaleSubmission,
                w.submit_memory(0, epoch, base, base + 1000, topology),
            ),
            1 => {
                w.submit_memory(0, epoch, base + 1000, base + 1000, topology)
                    .map_err(|f| io::Error::other(f.name()))?;
                (
                    WatchFault::DuplicateSubmission,
                    w.submit_memory(1, epoch, base + 1000, base + 1001, topology),
                )
            }
            2 => {
                w.submit_memory(0, epoch, base + 1000, base + 1000, topology)
                    .map_err(|f| io::Error::other(f.name()))?;
                w.observe(Observation::synthetic(seq + 2, base + 2000))
                    .map_err(|f| io::Error::other(f.name()))?;
                (
                    WatchFault::StaleSubmission,
                    w.submit_memory(1, epoch, base + 1000, base + 2000, topology),
                )
            }
            3 => {
                w.submit_memory(5, epoch, base + 1000, base + 1000, topology)
                    .map_err(|f| io::Error::other(f.name()))?;
                w.observe(Observation::synthetic(seq + 2, base + 2000))
                    .map_err(|f| io::Error::other(f.name()))?;
                (
                    WatchFault::FrameSequenceRollback,
                    w.submit_memory(5, epoch, base + 2000, base + 2000, topology),
                )
            }
            4 => {
                w.submit_memory(9, epoch, base + 1000, base + 1000, topology)
                    .map_err(|f| io::Error::other(f.name()))?;
                w.observe(Observation::synthetic(seq + 2, base + 2000))
                    .map_err(|f| io::Error::other(f.name()))?;
                (
                    WatchFault::FrameSequenceRollback,
                    w.submit_memory(8, epoch, base + 2000, base + 2000, topology),
                )
            }
            5 => {
                // Host Windows tests use 64-bit usize. On 32-bit use a
                // duplicate-observation fault; truncation is impossible.
                if usize::BITS > 32 {
                    let too_large = usize::try_from(u64::from(u32::MAX) + 1)
                        .map_err(|_| io::Error::other("usize width inconsistent"))?;
                    (
                        WatchFault::FrameIndexOverflow,
                        w.submit_memory(too_large, epoch, base + 1000, base + 1000, topology),
                    )
                } else {
                    w.submit_memory(0, epoch, base + 1000, base + 1000, topology)
                        .map_err(|f| io::Error::other(f.name()))?;
                    (
                        WatchFault::DuplicateSubmission,
                        w.submit_memory(1, epoch, base + 1000, base + 1001, topology),
                    )
                }
            }
            6 => (
                WatchFault::HeartbeatGap,
                w.submit_memory(0, epoch, base + 1000, base + 51_001, topology),
            ),
            7 => {
                w.observe(Observation::synthetic(seq + 2, base + 2000))
                    .map_err(|f| io::Error::other(f.name()))?;
                (
                    WatchFault::StaleSubmission,
                    w.submit_memory(0, epoch, base + 2001, base + 2001, topology),
                )
            }
            _ => unreachable!(),
        };
        ensure(actual == Err(expected), "replay/overflow failure mismatch")?;
        ensure(
            w.state() == WatchState::LatchedFault
                && w.retained() == 0
                && !w.latest_available()
                && w.last_fault() == Some(expected),
            "fault failed to latch and clear staged memory",
        )?;
        proof.latched_and_cleared += 1;
        match expected {
            WatchFault::StaleSubmission => proof.stale_stamp += 1,
            WatchFault::DuplicateSubmission => proof.duplicate_observation += 1,
            WatchFault::FrameSequenceRollback => proof.frame_sequence += 1,
            WatchFault::FrameIndexOverflow => proof.overflow += 1,
            WatchFault::HeartbeatGap => proof.timeout += 1,
            _ => return Err(io::Error::other("unexpected fault classification")),
        }
        ensure(
            w.submit_memory(10, epoch, base + 2000, base + 2000, topology)
                == Err(WatchFault::RequiresOperatorReset),
            "latched memory stage accepted",
        )?;
        ensure(!w.operator_reset(false), "unconfirmed reset accepted")?;
        ensure(
            w.begin(
                epoch + 1,
                epoch + 1,
                Observation::synthetic(seq + 3, base + 1000),
                topology,
            ) == Err(WatchFault::RequiresOperatorReset),
            "implicit rearm bypassed latch",
        )?;
        ensure(w.operator_reset(true), "confirmed reset rejected")?;
        let old_stamp = base + 1000;
        ensure(
            w.begin(
                epoch + 1,
                epoch + 1,
                Observation::synthetic(seq + 4, old_stamp),
                topology,
            )
            .is_err(),
            "stale epoch/observation rearmed",
        )?;
        proof.stale_rearms_denied += 1;
        w.begin(
            epoch + 1,
            epoch + 1,
            Observation::synthetic(seq + 5, base + 200_000),
            topology,
        )
        .map_err(|f| io::Error::other(f.name()))?;
        ensure(w.retained() == 0, "memory survived operator reset")?;
        w.observe(Observation::synthetic(seq + 6, base + 201_000))
            .map_err(|f| io::Error::other(f.name()))?;
        // Frame indices reset per epoch; a genuinely fresh observation
        // may begin at frame 0 even after a prior epoch faulted at frame 9.
        w.submit_memory(0, epoch + 1, base + 201_000, base + 201_000, topology)
            .map_err(|f| io::Error::other(f.name()))?;
        ensure(w.retained() == 1, "new epoch staging failed")?;
        proof.safe_rearms += 1;
    }
    ensure(
        proof.stale_stamp > 0
            && proof.duplicate_observation > 0
            && proof.frame_sequence > 0
            && proof.timeout > 0
            && (usize::BITS <= 32 || proof.overflow > 0),
        "required replay/overflow scenarios absent",
    )?;
    Ok(proof)
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let e = audit(samples)?;
    let body = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v60_offline_report_replay_and_frame_index\",",
            "\"synthetic_cases\":{},\"latched_and_memory_cleared\":{},",
            "\"stale_observation_stamp_denied\":{},\"duplicate_observation_denied\":{},",
            "\"frame_counter_replay_or_rollback_denied\":{},",
            "\"frame_index_overflow_denied\":{},\"expired_submission_denied\":{},",
            "\"confirmed_fresh_epoch_rearms\":{},\"stale_epoch_rearms_denied\":{},",
            "\"exactly_one_report_per_observation\":true,",
            "\"frame_index_checked_to_u32\":true,",
            "\"frame_counter_strictly_increases_within_epoch\":true,",
            "\"caller_time_not_authenticated\":true,",
            "\"all_packets_in_process_memory_only\":true,",
            "\"real_kernel_provider_authenticated\":false,\"driver_abi_verified\":false,",
            "\"automatic_rearm\":false,\"driver_installs\":0,\"driver_writes\":0,",
            "\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        e.tested,
        e.latched_and_cleared,
        e.stale_stamp,
        e.duplicate_observation,
        e.frame_sequence,
        e.overflow,
        e.timeout,
        e.safe_rearms,
        e.stale_rearms_denied,
    );
    crate::truth_lab::save_new(out, &body)?;
    println!(
        "V060_REPLAY=PASS cases={} faults={} rearmed_only_after_confirmation={} driver_writes=0 production=BLOCKED",
        e.tested, e.latched_and_cleared, e.safe_rearms
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_adversarial_report_replays_latch_and_clear() {
        let e = audit(8192).unwrap();
        assert_eq!(e.tested, 8192);
        assert_eq!(e.latched_and_cleared, 8192);
        assert_eq!(e.stale_stamp, 3072);
        assert_eq!(e.frame_sequence, 2048);
        assert_eq!(e.timeout, 1024);
        assert_eq!(e.safe_rearms, 8192);
        assert_eq!(e.stale_rearms_denied, 8192);
        if usize::BITS > 32 {
            assert_eq!(e.duplicate_observation, 1024);
            assert_eq!(e.overflow, 1024);
        }
    }
    #[test]
    fn invalid_sample_counts_are_rejected() {
        for n in [0, 1, 7, 9, 8191, MAX_SAMPLES + 1] {
            assert!(audit(n).is_err(), "{n}");
        }
    }
    #[test]
    fn evidence_cannot_be_overwritten() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v60-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("v60-evidence.json");
        if file.exists() {
            std::fs::remove_file(&file).unwrap();
        }
        run(8, &file).unwrap();
        let first = std::fs::read(&file).unwrap();
        assert!(run(8, &file).is_err());
        assert_eq!(first, std::fs::read(&file).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
