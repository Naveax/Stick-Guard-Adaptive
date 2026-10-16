//! v0.65 OS-monotonic timer bound to the ACTUAL offline Rust watcher.
//! Win32 uses Rust Instant/QueryPerformanceCounter underneath. Claims about
//! a real device, kernel driver, physical intent, or cross-process clock
//! attestation must never be inferred from this in-process test.
use crate::{
    output_supervisor::fixture_topology,
    provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState},
    routing_guard::Topology,
};
use std::{
    io,
    path::Path,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
struct HostState {
    watcher: ObservationWatch,
    last_observed_us: Option<u64>,
}
pub struct HostTimedOfflineWatch {
    started: Instant,
    state: Mutex<HostState>,
}
impl Default for HostTimedOfflineWatch {
    fn default() -> Self {
        Self::new()
    }
}
impl HostTimedOfflineWatch {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            state: Mutex::new(HostState {
                watcher: ObservationWatch::offline_lab(),
                last_observed_us: None,
            }),
        }
    }
    fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
    }
    fn access<T>(&self, f: impl FnOnce(&mut HostState, u64) -> T) -> Result<T, WatchFault> {
        // Lock serializes last observation timestamp and the submit action,
        // so a concurrent snapshot cannot splice a stale stamp into a report.
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(poisoned) => {
                let mut s = poisoned.into_inner();
                s.watcher.lock_poison_fail_closed();
                return Err(WatchFault::Backend);
            }
        };
        Ok(f(&mut state, self.now_us()))
    }
    pub fn begin(
        &self,
        token: u64,
        epoch: u64,
        mut observation: Observation,
        topo: Topology,
    ) -> Result<(), WatchFault> {
        self.access(|s, now| {
            observation.stamp_us = now;
            s.watcher.begin(token, epoch, observation, topo)?;
            s.last_observed_us = Some(now);
            Ok(())
        })?
    }
    pub fn observe(&self, mut observation: Observation) -> Result<(), WatchFault> {
        self.access(|s, now| {
            observation.stamp_us = now;
            s.watcher.observe(observation)?;
            s.last_observed_us = Some(now);
            Ok(())
        })?
    }
    pub fn submit_now(&self, frame: usize, epoch: u64, topo: Topology) -> Result<(), WatchFault> {
        self.access(|s, now| {
            let last = s
                .last_observed_us
                .ok_or(WatchFault::RequiresOperatorReset)?;
            s.watcher.submit_memory(frame, epoch, last, now, topo)
        })?
    }
    pub fn heartbeat_now(&self) -> Result<bool, WatchFault> {
        self.access(|s, now| s.watcher.heartbeat(now))
    }
    pub fn operator_reset(&self, confirmed: bool) -> Result<bool, WatchFault> {
        self.access(|s, _| {
            let done = s.watcher.operator_reset(confirmed);
            if done {
                s.last_observed_us = None;
            }
            done
        })
    }
    pub fn state(&self) -> Result<(WatchState, usize, Option<WatchFault>), WatchFault> {
        self.access(|s, _| {
            (
                s.watcher.state(),
                s.watcher.retained(),
                s.watcher.last_fault(),
            )
        })
    }
}
pub fn audit() -> io::Result<()> {
    let topo = fixture_topology();
    let w = HostTimedOfflineWatch::new();
    w.begin(1, 1, Observation::synthetic(1, 0), topo)
        .map_err(|e| io::Error::other(e.name()))?;
    thread::sleep(Duration::from_millis(3));
    w.observe(Observation::synthetic(2, 0))
        .map_err(|e| io::Error::other(e.name()))?;
    w.submit_now(0, 1, topo)
        .map_err(|e| io::Error::other(e.name()))?;
    if w.state() != Ok((WatchState::WatchingMemoryOnly, 1, None)) {
        return Err(io::Error::other(
            "monotonic submission failed to stage memory",
        ));
    }
    if w.submit_now(1, 1, topo) != Err(WatchFault::DuplicateSubmission)
        || w.state()
            != Ok((
                WatchState::LatchedFault,
                0,
                Some(WatchFault::DuplicateSubmission),
            ))
    {
        return Err(io::Error::other(
            "duplicate host-timed submit did not latch",
        ));
    }
    if w.operator_reset(false) != Ok(false) || w.operator_reset(true) != Ok(true) {
        return Err(io::Error::other("operator reset confirmation missing"));
    }
    let stale = HostTimedOfflineWatch::new();
    stale
        .begin(1, 1, Observation::synthetic(1, 0), topo)
        .map_err(|e| io::Error::other(e.name()))?;
    thread::sleep(Duration::from_millis(3));
    stale
        .observe(Observation::synthetic(2, 0))
        .map_err(|e| io::Error::other(e.name()))?;
    thread::sleep(Duration::from_millis(56));
    if stale.submit_now(0, 1, topo) != Err(WatchFault::HeartbeatGap)
        || stale.state() != Ok((WatchState::LatchedFault, 0, Some(WatchFault::HeartbeatGap)))
    {
        return Err(io::Error::other(
            "expired real monotonic submit did not latch",
        ));
    }
    Ok(())
}
pub fn run(out: &Path) -> io::Result<()> {
    audit()?;
    let json = concat!(
        "{\"schema\":1,\"source\":\"v65_rust_instant_bound_offline_watcher\",",
        "\"test_paths\":2,\"fresh_memory_stage\":true,",
        "\"duplicate_submit_latched\":true,\"expired_over_50ms_latched\":true,",
        "\"all_staged_memory_cleared_after_fault\":true,",
        "\"caller_cannot_supply_now_in_this_wrapper\":true,",
        "\"clock_source\":\"Rust_Instant_monotonic_host\",",
        "\"clock_is_cryptographically_attested\":false,",
        "\"cross_process_continuity_attested\":false,",
        "\"real_driver_clock_binding\":false,\"real_driver_abi_verified\":false,",
        "\"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}\n"
    );
    crate::truth_lab::save_new(out, json)?;
    println!("V065_RUST_HOST_CLOCK=PASS FRESH=PASS OVER_50MS=BLOCKED NO_DRIVER_IO=TRUE PRODUCTION=BLOCKED");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_rust_instant_drives_stale_submission_protection() {
        audit().unwrap();
    }
}
