//! v0.64 serialized OFFLINE watcher access and real OS-thread interleaving
//! checks. No kernel driver, HID, controller or game output can be enabled.
use crate::{
    output_supervisor::fixture_topology,
    provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState},
    routing_guard::Topology,
};
use std::{
    io,
    path::Path,
    sync::{Arc, Barrier, Mutex},
    thread,
};

const WORKERS: usize = 8;
const MAX_ROUNDS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessFault {
    Watch(WatchFault),
    Poisoned,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchSnapshot {
    pub state: WatchState,
    pub retained: usize,
    pub accepted: u64,
    pub last_fault: Option<WatchFault>,
}

/// Clones share exactly ONE in-process watch and ONE lock. The guard stays
/// held across validation, state transitions, and the memory-only staging.
/// A poisoned lock is not silently unpoisoned or treated as authorization.
#[derive(Clone)]
pub struct ConcurrentOfflineWatch {
    inner: Arc<Mutex<ObservationWatch>>,
}
impl ConcurrentOfflineWatch {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ObservationWatch::offline_lab())),
        }
    }
    fn access<T>(&self, f: impl FnOnce(&mut ObservationWatch) -> T) -> Result<T, AccessFault> {
        match self.inner.lock() {
            Ok(mut guard) => Ok(f(&mut guard)),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.lock_poison_fail_closed();
                Err(AccessFault::Poisoned)
            }
        }
    }
    pub fn begin(
        &self,
        token: u64,
        epoch: u64,
        observation: Observation,
        topology: Topology,
    ) -> Result<(), AccessFault> {
        self.access(|watch| watch.begin(token, epoch, observation, topology))?
            .map_err(AccessFault::Watch)
    }
    pub fn observe(&self, observation: Observation) -> Result<(), AccessFault> {
        self.access(|watch| watch.observe(observation))?
            .map_err(AccessFault::Watch)
    }
    pub fn submit_memory(
        &self,
        frame: usize,
        epoch: u64,
        stamp_us: u64,
        now_us: u64,
        topology: Topology,
    ) -> Result<(), AccessFault> {
        self.access(|watch| watch.submit_memory(frame, epoch, stamp_us, now_us, topology))?
            .map_err(AccessFault::Watch)
    }
    pub fn operator_reset(&self, confirmed: bool) -> Result<bool, AccessFault> {
        self.access(|watch| watch.operator_reset(confirmed))
    }
    pub fn snapshot(&self) -> Result<WatchSnapshot, AccessFault> {
        self.access(|watch| WatchSnapshot {
            state: watch.state(),
            retained: watch.retained(),
            accepted: watch.accepted(),
            last_fault: watch.last_fault(),
        })
    }
    pub fn heartbeat(&self, now_us: u64) -> Result<bool, AccessFault> {
        self.access(|watch| watch.heartbeat(now_us))
    }
}
impl Default for ConcurrentOfflineWatch {
    fn default() -> Self {
        Self::new()
    }
}
fn check(ok: bool, msg: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(msg))
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub rounds: usize,
    pub workers_per_round: usize,
    pub worker_operations: usize,
    pub submit_storms: usize,
    pub mixed_fault_races: usize,
    pub competing_arms: usize,
    pub reset_denial_races: usize,
    pub exactly_one_submit_winner_rounds: usize,
    pub confirmed_rearms_after_race: usize,
    pub final_latches: usize,
    pub cleared_memory_after_race: usize,
}
#[derive(Clone, Copy)]
enum Action {
    Submit,
    FaultObserve,
    Begin,
    ResetFalse,
}
fn racing_round(
    w: &ConcurrentOfflineWatch,
    actions: [Action; WORKERS],
    epoch: u64,
    stamp: u64,
    topo: Topology,
) -> io::Result<Vec<bool>> {
    let start = Arc::new(Barrier::new(WORKERS + 1));
    let mut handles = Vec::with_capacity(WORKERS);
    for action in actions {
        let copy = w.clone();
        let gate = start.clone();
        handles.push(thread::spawn(move || {
            gate.wait();
            match action {
                Action::Submit => copy.submit_memory(0, epoch, stamp, stamp, topo).is_ok(),
                Action::FaultObserve => {
                    let mut bad = Observation::synthetic(epoch * 10 + 2, stamp + 1000);
                    bad.service_running = false;
                    copy.observe(bad).is_ok()
                }
                Action::Begin => copy
                    .begin(
                        epoch,
                        epoch,
                        Observation::synthetic(epoch * 10, stamp - 1000),
                        topo,
                    )
                    .is_ok(),
                Action::ResetFalse => copy.operator_reset(false).unwrap_or(false),
            }
        }));
    }
    start.wait();
    handles
        .into_iter()
        .map(|h| {
            h.join()
                .map_err(|_| io::Error::other("racing OS thread panicked"))
        })
        .collect()
}
pub fn audit(rounds: usize) -> io::Result<Evidence> {
    if !(8..=MAX_ROUNDS).contains(&rounds) || !rounds.is_multiple_of(8) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rounds must be multiple of 8 within 8..1024",
        ));
    }
    let topo = fixture_topology();
    let mut r = Evidence {
        rounds,
        workers_per_round: WORKERS,
        worker_operations: 0,
        submit_storms: 0,
        mixed_fault_races: 0,
        competing_arms: 0,
        reset_denial_races: 0,
        exactly_one_submit_winner_rounds: 0,
        confirmed_rearms_after_race: 0,
        final_latches: 0,
        cleared_memory_after_race: 0,
    };
    for i in 0..rounds {
        let case = i % 8;
        let epoch = i as u64 + 1;
        let stamp = epoch * 100_000 + 2000;
        let w = ConcurrentOfflineWatch::new();
        if case != 6 {
            w.begin(
                epoch,
                epoch,
                Observation::synthetic(epoch * 10, stamp - 1000),
                topo,
            )
            .map_err(|_| io::Error::other("initial offline begin failed"))?;
            w.observe(Observation::synthetic(epoch * 10 + 1, stamp))
                .map_err(|_| io::Error::other("initial offline observe failed"))?;
        }
        let actions = if case < 4 {
            r.submit_storms += 1;
            [Action::Submit; WORKERS]
        } else if case < 6 {
            r.mixed_fault_races += 1;
            [
                Action::FaultObserve,
                Action::Submit,
                Action::Submit,
                Action::Submit,
                Action::Submit,
                Action::Submit,
                Action::Submit,
                Action::Submit,
            ]
        } else if case == 6 {
            r.competing_arms += 1;
            [Action::Begin; WORKERS]
        } else {
            r.reset_denial_races += 1;
            let mut bad = Observation::synthetic(epoch * 10 + 2, stamp + 1000);
            bad.service_running = false;
            check(
                w.observe(bad) == Err(AccessFault::Watch(WatchFault::ServiceStopped)),
                "failed to prepare latched watcher",
            )?;
            [
                Action::ResetFalse,
                Action::Submit,
                Action::ResetFalse,
                Action::Submit,
                Action::ResetFalse,
                Action::Submit,
                Action::ResetFalse,
                Action::Submit,
            ]
        };
        let outcomes = racing_round(&w, actions, epoch, stamp, topo)?;
        r.worker_operations += WORKERS;
        let wins = outcomes.iter().filter(|v| **v).count();
        if case < 4 {
            check(wins == 1, "duplicate concurrent submit succeeded")?;
            r.exactly_one_submit_winner_rounds += 1;
        } else if case < 6 {
            check(
                wins <= 1,
                "service fault race produced multiple submissions",
            )?;
        } else if case == 6 {
            check(wins == 1, "competing begin admitted several authorizations")?;
        } else {
            check(wins == 0, "unconfirmed reset or stale submit won")?;
        }
        let snap = w
            .snapshot()
            .map_err(|_| io::Error::other("snapshot unexpectedly poisoned"))?;
        check(
            snap.state == WatchState::LatchedFault
                && snap.retained == 0
                && snap.last_fault.is_some(),
            "race leaked memory or failed latch",
        )?;
        r.final_latches += 1;
        r.cleared_memory_after_race += 1;
        check(
            w.operator_reset(false) == Ok(false),
            "reset without confirmation accepted",
        )?;
        check(
            w.operator_reset(true) == Ok(true),
            "confirmed reset refused",
        )?;
        let fresh = Observation::synthetic(epoch * 10 + 3, stamp + 5000);
        w.begin(epoch + 1, epoch + 1, fresh, topo)
            .map_err(|_| io::Error::other("fresh confirmed epoch refused"))?;
        check(
            w.snapshot()
                .map_err(|_| io::Error::other("snapshot failure"))?
                .retained
                == 0,
            "prior memory survived confirmed reset",
        )?;
        r.confirmed_rearms_after_race += 1;
    }
    check(
        r.final_latches == rounds
            && r.worker_operations == rounds * WORKERS
            && r.submit_storms > 0
            && r.mixed_fault_races > 0
            && r.competing_arms > 0
            && r.reset_denial_races > 0,
        "concurrent test case coverage incomplete",
    )?;
    Ok(r)
}
pub fn run(rounds: usize, out: &Path) -> io::Result<()> {
    let r = audit(rounds)?;
    let text = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v64_offline_mutex_serialized_race_matrix\",",
            "\"rounds\":{},\"workers_per_round\":{},\"worker_operations\":{},",
            "\"simultaneous_submit_storm_rounds\":{},\"service_failure_race_rounds\":{},",
            "\"competing_authorization_rounds\":{},\"unconfirmed_reset_race_rounds\":{},",
            "\"exactly_one_submit_winner_rounds\":{},\"confirmed_rearms\":{},",
            "\"latched_after_race\":{},\"memory_cleared_after_race\":{},",
            "\"linearized_in_process_by_mutex\":true,",
            "\"lock_poison_always_latches_and_denies\":true,",
            "\"os_threads_have_nondeterministic_scheduling\":true,",
            "\"race_stress_not_formal_absence_of_all_races\":true,",
            "\"offline_in_process_memory_only\":true,",
            "\"real_driver_abi_verified\":false,\"driver_installs\":0,",
            "\"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        r.rounds,
        r.workers_per_round,
        r.worker_operations,
        r.submit_storms,
        r.mixed_fault_races,
        r.competing_arms,
        r.reset_denial_races,
        r.exactly_one_submit_winner_rounds,
        r.confirmed_rearms_after_race,
        r.final_latches,
        r.cleared_memory_after_race
    );
    crate::truth_lab::save_new(out, &text)?;
    println!("V064_CONCURRENCY=PASS rounds={} worker_ops={} latched={} driver_writes=0 production=BLOCKED",
             r.rounds,r.worker_operations,r.final_latches);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multithreaded_races_always_latch_and_clear() {
        let e = audit(64).unwrap();
        assert_eq!(e.worker_operations, 512);
        assert_eq!(e.final_latches, 64);
        assert_eq!(e.exactly_one_submit_winner_rounds, 32);
        assert_eq!(e.competing_arms, 8);
        assert_eq!(e.reset_denial_races, 8);
    }
    #[test]
    fn poisoned_lock_is_fail_closed_and_never_rearms() {
        let w = ConcurrentOfflineWatch::new();
        let topo = fixture_topology();
        w.begin(1, 1, Observation::synthetic(1, 1000), topo)
            .unwrap();
        w.observe(Observation::synthetic(2, 2000)).unwrap();
        w.submit_memory(0, 1, 2000, 2000, topo).unwrap();
        let clone = w.clone();
        let h = thread::spawn(move || {
            let _lock = clone.inner.lock().unwrap();
            panic!("deliberate poison for synthetic unit test");
        });
        assert!(h.join().is_err());
        assert_eq!(w.snapshot(), Err(AccessFault::Poisoned));
        assert_eq!(
            w.submit_memory(1, 1, 2000, 2000, topo),
            Err(AccessFault::Poisoned)
        );
        let guard = match w.inner.lock() {
            Ok(_) => panic!("lock poison unexpectedly cleared"),
            Err(poisoned) => poisoned.into_inner(),
        };
        assert_eq!(guard.state(), WatchState::LatchedFault);
        assert_eq!(guard.retained(), 0);
        assert_eq!(guard.last_fault(), Some(WatchFault::Backend));
        drop(guard);
        assert_eq!(w.operator_reset(true), Err(AccessFault::Poisoned));
    }
    #[test]
    fn unsupported_rounds_rejected() {
        for n in [0, 1, 7, 9, 63, 1025] {
            assert!(audit(n).is_err(), "{n}");
        }
    }
    #[test]
    fn create_new_receipt() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v64-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("race.json");
        if f.exists() {
            std::fs::remove_file(&f).unwrap();
        }
        run(8, &f).unwrap();
        let a = std::fs::read(&f).unwrap();
        assert!(run(8, &f).is_err());
        assert_eq!(a, std::fs::read(&f).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
