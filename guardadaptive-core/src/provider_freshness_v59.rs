//! v0.59: deterministic, memory-only time-of-submit gate regression.
//! No live PnP, Windows driver ABI or game input access is possible here.
use crate::output_supervisor::fixture_topology;
use crate::provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState};
use std::{io, path::Path};

const CASES: usize = 8;
const MIN_SAMPLES: usize = CASES;
const MAX_SAMPLES: usize = 65_536;
fn check(ok: bool, why: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(why))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessEvidence {
    pub cases: usize,
    pub blocked_latched: usize,
    pub successful_memory_prechecks: usize,
    pub boundary_at_50ms_accepted: usize,
    pub clock_rollback_faults: usize,
    pub heartbeat_gap_faults: usize,
    pub reset_with_confirmation_required: usize,
    pub stale_observation_rearms_denied: usize,
}
/// Test eight fault paths. Each independently constructs a new *offline*
/// in-process synthetic backend. The supplied "now" cannot prove host time.
pub fn audit(samples: usize) -> io::Result<FreshnessEvidence> {
    if !(MIN_SAMPLES..=MAX_SAMPLES).contains(&samples) || !samples.is_multiple_of(CASES) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be a multiple of 8, 8..65536",
        ));
    }
    let topo = fixture_topology();
    let mut evidence = FreshnessEvidence {
        cases: samples,
        blocked_latched: 0,
        successful_memory_prechecks: 0,
        boundary_at_50ms_accepted: 0,
        clock_rollback_faults: 0,
        heartbeat_gap_faults: 0,
        reset_with_confirmation_required: 0,
        stale_observation_rearms_denied: 0,
    };
    for i in 0..samples {
        let epoch = i as u64 + 1;
        let origin = epoch * 100_000;
        let stamp = origin + 1_000;
        let mut watch = ObservationWatch::offline_lab();
        let first = Observation::synthetic(epoch * 10, origin);
        watch
            .begin(epoch, epoch, first, topo)
            .map_err(|f| io::Error::other(f.name()))?;
        watch
            .observe(Observation::synthetic(epoch * 10 + 1, stamp))
            .map_err(|f| io::Error::other(f.name()))?;
        let case = i % CASES;
        let expected = if matches!(case, 2..=5) {
            WatchFault::ClockRollback
        } else {
            WatchFault::HeartbeatGap
        };
        let result = match case {
            // A missing watchdog call must NOT permit a delayed submit.
            0 => watch.submit_memory(0, epoch, stamp, stamp + 50_001, topo),
            // The precise 50-ms boundary is permitted, 50 ms + 1 us latches.
            1 => {
                watch
                    .submit_memory(0, epoch, stamp, stamp + 50_000, topo)
                    .map_err(|f| io::Error::other(f.name()))?;
                evidence.boundary_at_50ms_accepted += 1;
                evidence.successful_memory_prechecks += 1;
                check(
                    watch.heartbeat(stamp + 50_001),
                    "watchdog did not reject one microsecond past the boundary",
                )?;
                Err(WatchFault::HeartbeatGap)
            }
            // Submitted "now" is before the observed snapshot.
            2 => watch.submit_memory(0, epoch, stamp, stamp - 1, topo),
            // A once-advanced submit clock cannot be moved backward.
            3 => {
                watch
                    .submit_memory(0, epoch, stamp, stamp + 2_000, topo)
                    .map_err(|f| io::Error::other(f.name()))?;
                evidence.successful_memory_prechecks += 1;
                // A newer observation avoids the v0.60 duplicate-submit
                // gate, so the intended clock-rollback path is tested.
                watch
                    .observe(Observation::synthetic(epoch * 10 + 2, stamp + 2_000))
                    .map_err(|f| io::Error::other(f.name()))?;
                watch.submit_memory(1, epoch, stamp + 2_000, stamp + 1_999, topo)
            }
            // Heartbeat now is also a monotonic watermark for submit.
            4 => {
                check(
                    !watch.heartbeat(stamp + 2_000),
                    "in-window heartbeat faulted",
                )?;
                watch.submit_memory(0, epoch, stamp, stamp + 1_999, topo)
            }
            // A new signed-looking observation cannot rewind time behind
            // a previously accepted memory report.
            5 => {
                watch
                    .submit_memory(0, epoch, stamp, stamp + 2_000, topo)
                    .map_err(|f| io::Error::other(f.name()))?;
                evidence.successful_memory_prechecks += 1;
                watch.observe(Observation::synthetic(epoch * 10 + 2, stamp + 1_999))
            }
            // A latched timeout holds the last failed now across a
            // confirmed operator reset. A prior event cannot rearm.
            6 => watch.submit_memory(0, epoch, stamp, stamp + 50_001, topo),
            // Explicit heartbeat must still detect a stale report.
            7 => {
                check(watch.heartbeat(stamp + 50_001), "heartbeat missed timeout")?;
                Err(WatchFault::HeartbeatGap)
            }
            _ => unreachable!(),
        };
        check(result == Err(expected), "fault reason mismatch")?;
        check(
            watch.state() == WatchState::LatchedFault
                && watch.retained() == 0
                && !watch.latest_available()
                && watch.last_fault() == Some(expected),
            "fault did not clear staged memory or latch",
        )?;
        check(
            watch.submit_memory(4, epoch, stamp, stamp + 1000, topo)
                == Err(WatchFault::RequiresOperatorReset),
            "latched output accepted",
        )?;
        evidence.blocked_latched += 1;
        match expected {
            WatchFault::ClockRollback => evidence.clock_rollback_faults += 1,
            WatchFault::HeartbeatGap => evidence.heartbeat_gap_faults += 1,
            _ => unreachable!(),
        }
        check(
            !watch.operator_reset(false),
            "reset without explicit confirmation worked",
        )?;
        evidence.reset_with_confirmation_required += 1;
        if case == 6 {
            check(watch.operator_reset(true), "confirmed reset failed")?;
            // Failed submit advanced watermark to stamp+50001.
            check(
                watch.begin(
                    epoch + 1,
                    epoch + 1,
                    Observation::synthetic(epoch * 10 + 2, stamp + 50_000),
                    topo,
                ) == Err(WatchFault::ClockRollback),
                "old observation rearmed",
            )?;
            evidence.stale_observation_rearms_denied += 1;
            watch
                .begin(
                    epoch + 1,
                    epoch + 1,
                    Observation::synthetic(epoch * 10 + 3, stamp + 50_002),
                    topo,
                )
                .map_err(|f| io::Error::other(f.name()))?;
            check(watch.retained() == 0, "old memory survived rearm")?;
        }
    }
    check(
        evidence.clock_rollback_faults > 0
            && evidence.heartbeat_gap_faults > 0
            && evidence.boundary_at_50ms_accepted > 0
            && evidence.stale_observation_rearms_denied > 0,
        "missing required boundary coverage",
    )?;
    Ok(evidence)
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let e = audit(samples)?;
    let payload = format!(
        "{{\"schema\":1,\"source\":\"v59_offline_submit_freshness_regression\",\
        \"tested_scenarios\":{},\"fault_events\":{},\"blocked_latched_submits\":{},\
        \"accepted_memory_prechecks\":{},\"boundary_50000us_accepted\":{},\
        \"clock_rollback_faults\":{},\"heartbeat_gap_faults\":{},\
        \"unconfirmed_resets_denied\":{},\"stale_rearm_denied\":{},\
        \"every_submit_requires_caller_now_us\":true,\
        \"submit_time_and_observation_time_share_monotonic_watermark\":true,\
        \"caller_now_is_not_trusted_host_clock\":true,\
        \"all_frames_synthetic_in_process_only\":true,\
        \"windows_driver_loaded\":false,\"provider_abi_verified\":false,\
        \"automatic_rearm_allowed\":false,\"driver_installs\":0,\
        \"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n",
        CASES,
        e.cases,
        e.blocked_latched,
        e.successful_memory_prechecks,
        e.boundary_at_50ms_accepted,
        e.clock_rollback_faults,
        e.heartbeat_gap_faults,
        e.reset_with_confirmation_required,
        e.stale_observation_rearms_denied
    );
    crate::truth_lab::save_new(out, &payload)?;
    println!(
        "V059_FRESHNESS=PASS scenarios={} faults={} no_driver_writes=true production=BLOCKED",
        CASES, e.cases
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freshness_all_eight_memory_only_fail_closed_scenarios() {
        let e = audit(8192).unwrap();
        assert_eq!(e.cases, 8192);
        assert_eq!(e.blocked_latched, 8192);
        assert_eq!(e.clock_rollback_faults, 4096);
        assert_eq!(e.heartbeat_gap_faults, 4096);
        assert_eq!(e.boundary_at_50ms_accepted, 1024);
        assert_eq!(e.stale_observation_rearms_denied, 1024);
    }
    #[test]
    fn malformed_lengths_rejected_without_running() {
        for n in [0, 1, 7, 9, 8191, 65_537, 1_000_000] {
            assert!(audit(n).is_err(), "{n}");
        }
    }
    #[test]
    fn output_is_written_once() {
        let root = std::env::temp_dir().join(format!("guardadaptive-v59-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let out = root.join("report.json");
        if out.exists() {
            std::fs::remove_file(&out).unwrap();
        }
        run(8, &out).unwrap();
        let old = std::fs::read(&out).unwrap();
        assert!(run(8, &out).is_err());
        assert_eq!(old, std::fs::read(&out).unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
