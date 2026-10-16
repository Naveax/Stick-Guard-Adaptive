//! v0.63: u64 authorization boundary acceptance, memory-only. No device IO.
//! Explicit MAX is denied before a token/epoch watermark can be consumed.
use crate::{
    driver_contract_v43::{fixture_packet, BackendContract, Failure, State},
    output_supervisor::fixture_topology,
    provider_fault_v54::{Observation, ObservationWatch, WatchFault, WatchState},
};
use std::{io, path::Path};

const CASES: usize = 8;
const MAX_CASES: usize = 65_536;
fn check(ok: bool, what: &'static str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(what))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub cases: usize,
    pub terminal_denials: usize,
    pub direct_denials: usize,
    pub watcher_denials: usize,
    pub max_minus_one_accepted: usize,
    pub no_consumption_recoveries: usize,
    pub confirmed_resets: usize,
    pub shipping_denials: usize,
}
pub fn audit(n: usize) -> io::Result<Evidence> {
    if !(CASES..=MAX_CASES).contains(&n) || !n.is_multiple_of(CASES) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cases must be multiple of 8 in 8..65536",
        ));
    }
    let topo = fixture_topology();
    let mut result = Evidence {
        cases: n,
        terminal_denials: 0,
        direct_denials: 0,
        watcher_denials: 0,
        max_minus_one_accepted: 0,
        no_consumption_recoveries: 0,
        confirmed_resets: 0,
        shipping_denials: 0,
    };
    for i in 0..n {
        let which = i % CASES;
        let mut shipping = BackendContract::shipping();
        check(
            shipping.arm_at(u64::MAX, u64::MAX, true, topo, 1000) == Err(Failure::NoDriver)
                && shipping.submit(fixture_packet(0), 0, 2000, 0, topo) == Err(Failure::NoDriver),
            "shipping driver unlocked by boundary",
        )?;
        result.shipping_denials += 1;
        if which >= 6 {
            let mut w = ObservationWatch::offline_lab();
            let (token, epoch) = if which == 6 {
                (u64::MAX, 1)
            } else {
                (1, u64::MAX)
            };
            let expected = WatchFault::CounterExhausted;
            check(
                w.begin(token, epoch, Observation::synthetic(1, 1000), topo) == Err(expected)
                    && w.state() == WatchState::Off
                    && w.retained() == 0,
                "watcher accepted MAX authorization",
            )?;
            result.terminal_denials += 1;
            result.watcher_denials += 1;
            let safe_epoch = if which == 6 { 1 } else { u64::MAX - 1 };
            w.begin(1, safe_epoch, Observation::synthetic(1, 1000), topo)
                .map_err(|f| io::Error::other(f.name()))?;
            w.observe(Observation::synthetic(2, 2000))
                .map_err(|f| io::Error::other(f.name()))?;
            w.submit_memory(0, safe_epoch, 2000, 2000, topo)
                .map_err(|f| io::Error::other(f.name()))?;
            check(w.retained() == 1, "watcher denied good observation")?;
            let mut fault = Observation::synthetic(3, 3000);
            fault.device_present = false;
            check(
                w.observe(fault) == Err(WatchFault::DeviceGone) && w.retained() == 0,
                "watcher failed to drop staged memory",
            )?;
            check(
                !w.operator_reset(false) && w.operator_reset(true),
                "watcher reset confirmation missing",
            )?;
            result.confirmed_resets += 1;
            if which == 7 {
                check(
                    w.begin(2, u64::MAX, Observation::synthetic(4, 4000), topo)
                        == Err(WatchFault::CounterExhausted)
                        && w.state() == WatchState::Off,
                    "watcher accepted terminal epoch after near-max authorization",
                )?;
                result.terminal_denials += 1;
                result.watcher_denials += 1;
                result.max_minus_one_accepted += 1;
            } else {
                w.begin(2, 2, Observation::synthetic(4, 4000), topo)
                    .map_err(|f| io::Error::other(f.name()))?;
                result.no_consumption_recoveries += 1;
            }
            continue;
        }
        let mut b = BackendContract::offline_lab();
        if which <= 2 {
            let (token, epoch) = match which {
                0 => (u64::MAX, 1),
                1 => (1, u64::MAX),
                _ => (u64::MAX, u64::MAX),
            };
            check(
                b.arm_at(token, epoch, true, topo, 1000) == Err(Failure::CounterExhausted)
                    && b.state() == State::Off
                    && b.retained() == 0,
                "MAX authorization was accepted or mutated backend",
            )?;
            result.terminal_denials += 1;
            result.direct_denials += 1;
            b.arm_at(1, 1, true, topo, 1000)
                .map_err(|f| io::Error::other(f.name()))?;
            result.no_consumption_recoveries += 1;
        } else {
            let (token, epoch) = match which {
                3 => (u64::MAX - 1, u64::MAX - 1),
                4 => (u64::MAX - 1, 1),
                _ => (1, u64::MAX - 1),
            };
            b.arm_at(token, epoch, true, topo, 1000)
                .map_err(|f| io::Error::other(f.name()))?;
            result.max_minus_one_accepted += 1;
        }
        let epoch = if which <= 2 || which == 4 {
            1
        } else {
            u64::MAX - 1
        };
        b.submit(fixture_packet(0), epoch, 2000, 0, topo)
            .map_err(|f| io::Error::other(f.name()))?;
        check(b.retained() == 1, "valid boundary report was not staged")?;
        b.emergency_off();
        check(
            b.retained() == 0
                && b.state() == State::LatchedFault
                && !b.operator_reset(false)
                && b.operator_reset(true),
            "fault reset or memory clear failed",
        )?;
        result.confirmed_resets += 1;
        let (next_token, next_epoch) = match which {
            0..=2 => (2, 2),
            3 => (u64::MAX, u64::MAX),
            4 => (u64::MAX, 2),
            _ => (2, u64::MAX),
        };
        if which <= 2 {
            b.arm_at(next_token, next_epoch, true, topo, 3000)
                .map_err(|f| io::Error::other(f.name()))?;
            result.no_consumption_recoveries += 1;
        } else {
            check(
                b.arm_at(next_token, next_epoch, true, topo, 3000)
                    == Err(Failure::CounterExhausted)
                    && b.state() == State::Off
                    && b.latest().is_none(),
                "counter wrapped or terminal epoch was accepted",
            )?;
            result.terminal_denials += 1;
            result.direct_denials += 1;
        }
    }
    check(
        result.terminal_denials > 0
            && result.direct_denials > 0
            && result.watcher_denials > 0
            && result.max_minus_one_accepted > 0
            && result.no_consumption_recoveries > 0,
        "boundary-case coverage incomplete",
    )?;
    Ok(result)
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    let e = audit(samples)?;
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v63_offline_authorization_u64_limits\",",
            "\"synthetic_cases\":{},\"terminal_counter_denials\":{},",
            "\"direct_backend_denials\":{},\"observation_watcher_denials\":{},",
            "\"max_minus_one_authorizations_accepted\":{},",
            "\"recovery_without_consuming_failed_pins\":{},",
            "\"confirmed_resets\":{},\"shipping_driver_denials\":{},",
            "\"u64_max_auth_is_rejected_before_mutation\":true,",
            "\"terminal_exhaustion_has_no_wrap_or_rearm\":true,",
            "\"max_minus_one_can_be_last_valid_authorization\":true,",
            "\"no_implicit_reset\":true,\"synthetic_only\":true,",
            "\"real_driver_identity_verified\":false,\"driver_abi_verified\":false,",
            "\"driver_installs\":0,\"driver_writes\":0,\"game_input_writes\":0,",
            "\"production_gate\":\"BLOCKED\"}}\n"
        ),
        e.cases,
        e.terminal_denials,
        e.direct_denials,
        e.watcher_denials,
        e.max_minus_one_accepted,
        e.no_consumption_recoveries,
        e.confirmed_resets,
        e.shipping_denials
    );
    crate::truth_lab::save_new(out, &report)?;
    println!(
        "V063_LIMITS=PASS cases={} denials={} driver_writes=0 production=BLOCKED",
        e.cases, e.terminal_denials
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_boundary_and_recovery_matrix() {
        let e = audit(8192).unwrap();
        assert_eq!(e.terminal_denials, 9216);
        assert_eq!(e.direct_denials, 6144);
        assert_eq!(e.watcher_denials, 3072);
        assert_eq!(e.max_minus_one_accepted, 4096);
        assert_eq!(e.no_consumption_recoveries, 7168);
        assert_eq!(e.confirmed_resets, 8192);
        assert_eq!(e.shipping_denials, 8192);
    }
    #[test]
    fn invalid_sample_counts() {
        for n in [0, 1, 7, 9, 8191, MAX_CASES + 1] {
            assert!(audit(n).is_err(), "{n}");
        }
    }
    #[test]
    fn evidence_create_new_only() {
        let dir = std::env::temp_dir().join(format!("guardadaptive-v63-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bounds.json");
        if p.exists() {
            std::fs::remove_file(&p).unwrap();
        }
        run(8, &p).unwrap();
        let before = std::fs::read(&p).unwrap();
        assert!(run(8, &p).is_err());
        assert_eq!(before, std::fs::read(&p).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
