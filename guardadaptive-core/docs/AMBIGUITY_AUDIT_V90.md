# GuardAdaptive v0.90 | Unknown-drift ambiguity audit

## Purpose

A reproducible, **synthetic-only** independent observer audit added to the existing Rust core:

`src/bin/ambiguity_audit_v90.rs`

The objective is to measure the cost of treating a stationary off-center stick position as suspicious, **without claiming that a suspicion is confirmed drift** and without changing any real game input.

**Physical drift correction under 0.9% is NOT achieved or verified for unseen sudden drift.**

## Reproduce

```powershell
cd C:\Users\navea\source\GuardAdaptive
cargo test --locked --offline --bin ambiguity_audit_v90
cargo run --quiet --locked --offline --bin ambiguity_audit_v90 -- --synthetic-matrix
cargo clippy --locked --offline --all-targets -- -D warnings
```

On the public repository, run the commands under `guardadaptive-core/` instead. The executable uses no network access, physical joystick IO, real-volunteer session files, learning, virtual gamepad or driver.

The model is initially calibrated to a **known, synthetic +5% / -4% baseline** using two automatically generated neutral windows and the existing `GuidedWizard`. That token is merely a simulator oracle, not an independently verified human release.

## Test matrix

Seven scenarios at 16ms, 8ms and 4ms sample intervals (nominal 62.5, 125 and 250 Hz), 5.6 seconds each: neutral, intentionally held 0.3% micro vector, intentionally held 10% direction, newly introduced 10% drift, gradual ramp to 10% followed by a plateau, rapid alternating directions, and low-amplitude sensor noise.

| Synthetic measurement | Measured |
|---|---:|
| Observer regression conditions met | 21/21 |
| Suspicion alerts on previously unseen real-*simulated* drift | 6/6 |
| Suspicion alerts on deliberately held 10% direction | **3/3** |
| Unknown-drift residual meets strict <0.9% goal | **0/6** |
| Previously unseen abrupt drift residual | approx. **10%** |
| Loss of deliberately held micro input | 0 for the tested idealized cases |
| Automatic, unverified center updates | 0 |

The label `OBSERVER_REGRESSION_GATE=21/21` only means the **expected protective observer behavior** was exercised. It **does not imply a successful drift-removal algorithm**.

False suspicion of drift on held intent is a known, deliberately exposed confound. Identical raw input produces identical warnings in the held-intent and drift scenarios despite entirely different actual intent. In this observer no false suspicion triggers automatic correction.

The gradual-ramp case can enter its eventual stable plateau slightly before the nominal end of the ramp due to existing 0.3%-radius stability tolerance. Detection begins around 4704–4796 ms in this simulator, not necessarily 800 ms after exactly 4000 ms. Timing must be evaluated on physical hardware independently.

## Safety invariants

- No driver, HID output, virtual controller or game write.
- No unattended center acquisition; `unverified_center_updates=0`.
- No self-reported volunteer recordings are promoted into physical holdout.
- No guarantee of universal micro-motion preservation when user falsely confirms a non-neutral calibration.
- Measurement labels are **generator-provided synthetic ground truth**, not physical ground truth.
- Production/physical acceptance remain **BLOCKED**.

## Next evidence required

Obtain independent synchronized physical shaft-position reference or a controlled mechanical fixture across multiple real controllers. Record intentionally held positions, truly released neutral periods, drift shifts and clock jitter. Report false suspicion rate per held-motion trial, true suspicion delay, max neutral output residual against **independent position evidence**, micro-direction errors, and actual device-to-output latency. Freeze thresholds before blind holdout.

A read-only observer is not a real drift corrector. It is safer to surface ambiguity than to silently consume someone's intended input.

## Extended high-polling-rate regression (2026-10-17)
The original 21-case synthetic matrix (16ms/8ms/4ms; 62.5/125/250 Hz) now also runs 2ms and 1ms intervals (500 and 1000 Hz). The generator produces 35 total cases under the same fixed 5.6-second synthetic timeline. Independent physical controller testing has **not** occurred.
- `OBSERVER_REGRESSION_GATE=35/35`: correct protective observer behavior (not drift correction success).
- Held intentional direction falsely suspected: `5/5` scenarios, while output is not erased.
- Unknown drift is suspected: `10/10` synthetic cases.
- Unknown drift achieves strict <0.9% correction: `0/10`; remaining error reaches ~10%.
- No unverified center learning. No game output. No measured USB polling or end-to-end device latency.

The 1ms synthetic sample interval alone does not prove that an actual joystick reports valid 1000 Hz, nor that the running system has low jitter. Real physical device availability, synchronized independent motion ground truth, and unmodified live captures remain required.
