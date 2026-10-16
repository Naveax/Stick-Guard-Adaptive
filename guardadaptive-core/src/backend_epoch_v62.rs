//! v0.62 synthetic epoch anti-replay regression. No driver or game I/O.
//! Authorization token and epoch are independent monotonically increasing
//! watermarks; neither one is reset by an explicit operator reset.
use crate::{
    driver_contract_v43::{fixture_packet, BackendContract, Failure, State},
    output_supervisor::fixture_topology,
    provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState},
};
use std::{io, path::Path};

const CASES: usize = 8;
const MAX_CASES: usize = 65_536;

fn check(ok: bool, message: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub cases: usize,
    pub epoch_replays: usize,
    pub token_replays: usize,
    pub zero_epoch_denials: usize,
    pub watcher_epoch_replays: usize,
    pub explicit_operator_resets: usize,
    pub safe_new_epochs: usize,
    pub stale_reports_cleared: usize,
    pub shipping_denials: usize,
}

/// Checks eight independent cases with a fresh in-process backend per case.
/// Invalid attempts do not change authorization watermarks or rearm hardware.
pub fn audit(samples: usize) -> io::Result<Evidence> {
    if !(CASES..=MAX_CASES).contains(&samples) || !samples.is_multiple_of(CASES) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples must be a multiple of 8 within 8..65536",
        ));
    }
    let topo = fixture_topology();
    let mut result = Evidence {
        cases: samples,
        epoch_replays: 0,
        token_replays: 0,
        zero_epoch_denials: 0,
        watcher_epoch_replays: 0,
        explicit_operator_resets: 0,
        safe_new_epochs: 0,
        stale_reports_cleared: 0,
        shipping_denials: 0,
    };

    for i in 0..samples {
        let e = i as u64 + 10;
        let mut shipping = BackendContract::shipping();
        check(
            shipping.arm_at(e, e, true, topo, 1000) == Err(Failure::NoDriver)
                && shipping.submit(fixture_packet(0), e, 2000, 1, topo) == Err(Failure::NoDriver),
            "shipping transport unexpectedly enabled",
        )?;
        result.shipping_denials += 1;

        if i % CASES == 6 {
            let mut watch = ObservationWatch::offline_lab();
            watch
                .begin(e, e, Observation::synthetic(1, 1000), topo)
                .map_err(|x| io::Error::other(x.name()))?;
            watch
                .observe(Observation::synthetic(2, 2000))
                .map_err(|x| io::Error::other(x.name()))?;
            watch
                .submit_memory(0, e, 2000, 2000, topo)
                .map_err(|x| io::Error::other(x.name()))?;
            let mut stopped = Observation::synthetic(3, 3000);
            stopped.service_running = false;
            check(
                watch.observe(stopped) == Err(WatchFault::ServiceStopped)
                    && watch.state() == WatchState::LatchedFault
                    && watch.retained() == 0,
                "watcher did not latch service failure",
            )?;
            result.stale_reports_cleared += 1;
            check(
                watch.begin(e + 1, e + 1, Observation::synthetic(4, 4000), topo)
                    == Err(WatchFault::RequiresOperatorReset),
                "watcher rearmed without reset",
            )?;
            check(
                !watch.operator_reset(false) && watch.operator_reset(true),
                "watcher reset did not require confirmation",
            )?;
            result.explicit_operator_resets += 1;
            check(
                watch.begin(e + 1, e, Observation::synthetic(4, 4000), topo)
                    == Err(WatchFault::EpochReplay)
                    && watch.retained() == 0
                    && watch.state() == WatchState::Off,
                "watcher accepted an old epoch under a fresh token",
            )?;
            result.watcher_epoch_replays += 1;
            watch
                .begin(e + 1, e + 1, Observation::synthetic(4, 4000), topo)
                .map_err(|x| io::Error::other(x.name()))?;
            watch
                .observe(Observation::synthetic(5, 5000))
                .map_err(|x| io::Error::other(x.name()))?;
            watch
                .submit_memory(0, e + 1, 5000, 5000, topo)
                .map_err(|x| io::Error::other(x.name()))?;
            check(
                watch.retained() == 1,
                "fresh watcher epoch could not stage memory",
            )?;
            result.safe_new_epochs += 1;
            continue;
        }

        let mut b = BackendContract::offline_lab();
        if i % CASES == 7 {
            check(
                b.arm_at(1, 0, true, topo, 1000) == Err(Failure::EpochReplay)
                    && b.state() == State::Off
                    && b.retained() == 0,
                "initial zero epoch was permitted",
            )?;
            result.zero_epoch_denials += 1;
            b.arm_at(1, 1, true, topo, 1000)
                .map_err(|x| io::Error::other(x.name()))?;
            b.submit(fixture_packet(0), 1, 2000, 1, topo)
                .map_err(|x| io::Error::other(x.name()))?;
            check(b.retained() == 1, "valid first epoch failed")?;
            result.safe_new_epochs += 1;
            continue;
        }

        b.arm_at(e, e, true, topo, 1000)
            .map_err(|x| io::Error::other(x.name()))?;
        b.submit(fixture_packet(0), e, 2000, 1, topo)
            .map_err(|x| io::Error::other(x.name()))?;
        b.emergency_off();
        check(
            b.state() == State::LatchedFault && b.latest().is_none() && b.retained() == 0,
            "emergency failed to clear synthetic ring",
        )?;
        result.stale_reports_cleared += 1;
        check(
            b.arm_at(e + 1, e + 1, true, topo, 3000) == Err(Failure::MustReset),
            "unconfirmed rearm permitted",
        )?;
        check(
            !b.operator_reset(false) && b.operator_reset(true),
            "operator confirmation was bypassed",
        )?;
        result.explicit_operator_resets += 1;

        let (token, epoch, expected, timestamped) = match i % CASES {
            0 => (e + 1, e, Failure::EpochReplay, false),
            1 => (e + 1, e - 1, Failure::EpochReplay, false),
            2 => (e + 1, 0, Failure::EpochReplay, false),
            3 => (e, e + 1, Failure::ReplayAuthorization, false),
            4 => (e - 1, e + 1, Failure::ReplayAuthorization, false),
            5 => (e + 1, e, Failure::EpochReplay, true),
            _ => unreachable!(),
        };
        let denied = if timestamped {
            b.arm_at(token, epoch, true, topo, 3000)
        } else {
            b.arm(token, epoch, true, topo)
        };
        check(
            denied == Err(expected) && b.state() == State::Off && b.retained() == 0,
            "replayed token/epoch mutated the backend",
        )?;
        match expected {
            Failure::EpochReplay => result.epoch_replays += 1,
            Failure::ReplayAuthorization => result.token_replays += 1,
            _ => unreachable!(),
        }
        // A new independent authorization pair is accepted only AFTER an
        // explicit reset; invalid attempts must not consume either watermark.
        b.arm_at(e + 1, e + 1, true, topo, 3000)
            .map_err(|x| io::Error::other(x.name()))?;
        check(
            b.retained() == 0 && b.latest().is_none(),
            "old ring survived rearm",
        )?;
        b.submit(fixture_packet(0), e + 1, 4000, 0, topo)
            .map_err(|x| io::Error::other(x.name()))?;
        check(
            b.retained() == 1,
            "fresh epoch did not accept offline report",
        )?;
        result.safe_new_epochs += 1;
    }

    check(
        result.epoch_replays > 0
            && result.token_replays > 0
            && result.zero_epoch_denials > 0
            && result.watcher_epoch_replays > 0
            && result.safe_new_epochs == samples,
        "incomplete independent epoch replay coverage",
    )?;
    Ok(result)
}

pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let e = audit(samples)?;
    let result = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v62_offline_epoch_authorization_replay\",",
            "\"synthetic_cases\":{},\"direct_epoch_replays_denied\":{},",
            "\"token_replays_denied\":{},\"zero_epoch_denied\":{},",
            "\"watcher_epoch_replays_denied\":{},\"confirmed_operator_resets\":{},",
            "\"fresh_epochs_accepted_memory_only\":{},\"stale_rings_cleared\":{},",
            "\"shipping_driver_unavailable\":{},",
            "\"token_and_epoch_watermarks_independently_monotonic\":true,",
            "\"epoch_watermark_persists_after_operator_reset\":true,",
            "\"rejected_attempts_do_not_consume_valid_epoch\":true,",
            "\"synthetic_memory_only\":true,\"real_provider_attested\":false,",
            "\"real_virtual_gamepad_abi_verified\":false,\"driver_installs\":0,",
            "\"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        e.cases,
        e.epoch_replays,
        e.token_replays,
        e.zero_epoch_denials,
        e.watcher_epoch_replays,
        e.explicit_operator_resets,
        e.safe_new_epochs,
        e.stale_reports_cleared,
        e.shipping_denials,
    );
    crate::truth_lab::save_new(out, &result)?;
    println!(
        "V062_EPOCH=PASS samples={} direct_epoch_replays={} watcher_epoch_replays={} real_writes=0 production=BLOCKED",
        e.cases, e.epoch_replays, e.watcher_epoch_replays
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_epoch_and_token_replays_rejected() {
        let r = audit(8192).unwrap();
        assert_eq!(r.epoch_replays, 4096);
        assert_eq!(r.token_replays, 2048);
        assert_eq!(r.zero_epoch_denials, 1024);
        assert_eq!(r.watcher_epoch_replays, 1024);
        assert_eq!(r.safe_new_epochs, 8192);
        assert_eq!(r.explicit_operator_resets, 7168);
        assert_eq!(r.stale_reports_cleared, 7168);
        assert_eq!(r.shipping_denials, 8192);
    }

    #[test]
    fn bad_sample_counts_are_denied() {
        for n in [0, 1, 7, 9, 8191, MAX_CASES + 1] {
            assert!(audit(n).is_err(), "{n}");
        }
    }

    #[test]
    fn proof_is_create_new() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v62-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("v62.json");
        if f.exists() {
            std::fs::remove_file(&f).unwrap();
        }
        run(8, &f).unwrap();
        let previous = std::fs::read(&f).unwrap();
        assert!(run(8, &f).is_err());
        assert_eq!(previous, std::fs::read(&f).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
