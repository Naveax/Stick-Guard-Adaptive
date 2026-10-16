//! v0.46 deterministic fault-injection laboratory for the OFFLINE memory backend.
//! This is not a hardware driver. Shipping remains permanently disabled.
use crate::{
    driver_contract_v43::{fixture_packet, BackendContract, Failure, State},
    output_supervisor::fixture_topology,
    routing_guard::Topology,
};
use std::{io, path::Path};

const SEGMENT: usize = 1024;
const CASES: usize = 11;
const POLL_PERIOD_US: u64 = 1_000;

#[derive(Debug, PartialEq, Eq)]
pub struct Evidence {
    pub submitted: u64,
    pub successful_memory_reports: u64,
    pub fault_counts: [u64; CASES],
    pub rejected_implicit_arms: u64,
    pub rejected_stale_submits: u64,
}

fn check(condition: bool, message: &'static str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

fn assert_fault(
    backend: &mut BackendContract,
    expected: Failure,
    actual: Result<(), Failure>,
) -> io::Result<()> {
    check(actual == Err(expected), "fault injection was not rejected")?;
    check(
        backend.state() == State::LatchedFault
            && backend.retained() == 0
            && backend.latest().is_none()
            && backend.last_fault() == Some(expected),
        "fault did not clear reports or remain latched",
    )
}

/// Runs deliberately deterministic synthetic faults, including a timestamp
/// replay and a dead first-poll connection, with no driver I/O.
pub fn audit(samples: usize) -> io::Result<Evidence> {
    if !(CASES * SEGMENT..=1_048_576).contains(&samples) || !samples.is_multiple_of(CASES * SEGMENT)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be 11264..1048576 divisible by 11264",
        ));
    }
    let topology = fixture_topology();
    let mut shipping = BackendContract::shipping();
    check(
        shipping.arm_at(1, 1, true, topology, 0) == Err(Failure::NoDriver)
            && shipping.submit(fixture_packet(0), 1, 1000, 0, topology) == Err(Failure::NoDriver)
            && shipping.latest().is_none()
            && shipping.retained() == 0,
        "shipping backend unexpectedly exposed game output",
    )?;

    let mut backend = BackendContract::offline_lab();
    let mut counts = [0; CASES];
    let mut rejected_arms = 0u64;
    let mut rejected_submits = 0u64;
    for segment in 0..samples / SEGMENT {
        let epoch = segment as u64 + 1;
        if segment > 0 {
            check(
                backend.arm_at(epoch, epoch, true, topology, 0) == Err(Failure::MustReset),
                "automatic rearm accepted",
            )?;
            rejected_arms += 1;
            check(!backend.operator_reset(false), "unconfirmed reset accepted")?;
            check(backend.operator_reset(true), "confirmed reset rejected")?;
            check(
                backend.arm_at(epoch - 1, epoch, true, topology, 0)
                    == Err(Failure::ReplayAuthorization),
                "authorization token replay accepted",
            )?;
        }
        let scenario = segment % CASES;
        let origin = if scenario == 10 { 100_000 } else { 0 };
        backend
            .arm_at(epoch, epoch, true, topology, origin)
            .map_err(|e| io::Error::other(e.name()))?;
        check(
            backend.state() == State::Shadow
                && backend.retained() == 0
                && backend.latest().is_none(),
            "new epoch exposed stale packets",
        )?;
        if scenario < 8 {
            for frame in 0..SEGMENT {
                backend
                    .submit(
                        fixture_packet(segment * SEGMENT + frame),
                        epoch,
                        origin + (frame as u64 + 1) * POLL_PERIOD_US,
                        frame as u32,
                        topology,
                    )
                    .map_err(|e| io::Error::other(e.name()))?;
            }
            check(
                backend.retained() == 64 && backend.latest().is_some(),
                "ring is not bounded or failed to stage reports",
            )?;
        }
        let last = origin + SEGMENT as u64 * POLL_PERIOD_US;
        let current = (SEGMENT - 1) as u32;
        match scenario {
            0 => {
                let actual = backend.submit(fixture_packet(0), epoch, last, current, topology);
                assert_fault(&mut backend, Failure::ReplayedPoll, actual)?;
            }
            1 => {
                let actual = backend.submit(fixture_packet(0), epoch, last + 1000, 0, topology);
                assert_fault(&mut backend, Failure::PacketRollback, actual)?;
            }
            2 => {
                check(backend.watchdog(last + 50_001), "watchdog did not fire")?;
                check(
                    backend.last_fault() == Some(Failure::PollTimeout),
                    "watchdog reason",
                )?;
            }
            3 => {
                backend.disconnect();
                check(
                    backend.last_fault() == Some(Failure::DeviceEpoch),
                    "disconnect reason",
                )?;
            }
            4 => {
                let collided = Topology {
                    physical_visible_to_game: Some(true),
                    ..topology
                };
                let actual =
                    backend.submit(fixture_packet(0), epoch, last + 1000, current + 1, collided);
                assert_fault(&mut backend, Failure::DuplicateVisibility, actual)?;
            }
            5 => {
                backend.emergency_off();
                check(
                    backend.last_fault() == Some(Failure::Emergency),
                    "emergency reason",
                )?;
            }
            6 => {
                let actual = backend.submit(
                    fixture_packet(0),
                    epoch,
                    last + 50_001,
                    current + 1,
                    topology,
                );
                assert_fault(&mut backend, Failure::PollTimeout, actual)?;
            }
            7 => {
                let actual = backend.submit(
                    fixture_packet(0),
                    epoch + 1,
                    last + 1000,
                    current + 1,
                    topology,
                );
                assert_fault(&mut backend, Failure::DeviceEpoch, actual)?;
            }
            8 => {
                check(backend.watchdog(50_001), "first-poll watchdog did not fire")?;
                check(
                    backend.last_fault() == Some(Failure::PollTimeout),
                    "first-poll watchdog reason",
                )?;
            }
            9 => {
                let actual = backend.submit(fixture_packet(0), epoch, 50_001, 1, topology);
                assert_fault(&mut backend, Failure::PollTimeout, actual)?;
            }
            10 => {
                let actual = backend.submit(fixture_packet(0), epoch, origin - 1, 1, topology);
                assert_fault(&mut backend, Failure::ClockRollback, actual)?;
            }
            _ => unreachable!(),
        }
        counts[scenario] += 1;
        check(
            backend.state() == State::LatchedFault
                && backend.retained() == 0
                && backend.latest().is_none()
                && backend.last_fault().is_some(),
            "stale report leaked after injected fault",
        )?;
        check(
            backend.submit(fixture_packet(0), epoch, last + 2000, current + 2, topology)
                == Err(Failure::MustReset),
            "submit was accepted after fault",
        )?;
        rejected_submits += 1;
    }
    check(counts.iter().all(|&n| n > 0), "incomplete injection matrix")?;
    check(
        backend.fault_count() == (samples / SEGMENT) as u64,
        "fault counter mismatch",
    )?;
    check(
        backend.state() == State::LatchedFault && backend.retained() == 0,
        "fault latch not retained",
    )?;
    Ok(Evidence {
        submitted: samples as u64,
        successful_memory_reports: backend.accepted(),
        fault_counts: counts,
        rejected_implicit_arms: rejected_arms,
        rejected_stale_submits: rejected_submits,
    })
}

pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let result = audit(samples)?;
    let all_faults: u64 = result.fault_counts.iter().sum();
    let mut report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v46_memory_fault_injection\",",
            "\"synthetic_frames_requested\":{},\"memory_reports_accepted\":{},",
            "\"fault_scenarios\":11,\"fault_events\":{},",
            "\"rejected_implicit_arms\":{},\"rejected_after_fault_submits\":{},",
            "\"each_fault_scenario_counts\":["
        ),
        result.submitted,
        result.successful_memory_reports,
        all_faults,
        result.rejected_implicit_arms,
        result.rejected_stale_submits,
    );
    for (i, n) in result.fault_counts.iter().enumerate() {
        if i != 0 {
            report.push(',');
        }
        report.push_str(&n.to_string());
    }
    report.push_str(
        "],\"first_poll_watchdog_covered\":true,\"duplicate_timestamp_rejected\":true,\
         \"legacy_memory_contract_retained\":true,\"driver_authorized\":false,\
         \"hardware_controller_authenticated\":false,\"virtual_output_enabled\":false,\
         \"driver_writes\":0,\"game_input_writes\":0,\"production\":\"BLOCKED\"}\n",
    );
    crate::truth_lab::save_new(out, &report)?;
    println!(
        "V046: {} synthetic memory reports; {} injected faults; no device output; production BLOCKED",
        result.successful_memory_reports, all_faults
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_poll_and_duplicate_timestamp_faults_are_detected() {
        let g = fixture_topology();
        let mut b = BackendContract::offline_lab();
        b.arm_at(1, 1, true, g, 40_000).unwrap();
        assert!(!b.watchdog(90_000));
        assert!(b.watchdog(90_001));
        assert_eq!(b.last_fault(), Some(Failure::PollTimeout));
        assert!(!b.operator_reset(false));
        assert!(b.operator_reset(true));
        b.arm_at(2, 2, true, g, 500_000).unwrap();
        b.submit(fixture_packet(0), 2, 501_000, 1, g).unwrap();
        assert_eq!(
            b.submit(fixture_packet(1), 2, 501_000, 2, g),
            Err(Failure::ReplayedPoll)
        );
        assert_eq!(b.retained(), 0);
        assert_eq!(b.latest(), None);
    }
    #[test]
    fn source_must_not_submit_late_or_before_arm() {
        let g = fixture_topology();
        for (first_stamp, failure) in [
            (60_001, Failure::PollTimeout),
            (9_999, Failure::ClockRollback),
        ] {
            let mut b = BackendContract::offline_lab();
            b.arm_at(1, 1, true, g, 10_000).unwrap();
            assert_eq!(
                b.submit(fixture_packet(0), 1, first_stamp, 1, g),
                Err(failure)
            );
            assert_eq!(b.retained(), 0);
        }
    }
    #[test]
    fn offline_full_fault_matrix_and_no_writes() {
        let e = audit(11 * 1024 * 4).unwrap();
        assert_eq!(e.submitted, 45056);
        assert_eq!(e.successful_memory_reports, 32 * 1024);
        assert_eq!(e.fault_counts, [4; CASES]);
        assert_eq!(e.rejected_implicit_arms, 43);
        assert_eq!(e.rejected_stale_submits, 44);
        assert!(audit(1024).is_err());
    }
    #[test]
    fn evidence_is_non_overwriting() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v46-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("evidence.json");
        let _ = std::fs::remove_file(&out);
        run(11 * 1024, &out).unwrap();
        let before = std::fs::read(&out).unwrap();
        assert!(run(11 * 1024, &out).is_err());
        assert_eq!(before, std::fs::read(&out).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
