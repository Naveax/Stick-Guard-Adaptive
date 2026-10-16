# GuardAdaptive v0.85 | Precision Envelope, offline research

## Requested goal
Keep the remaining radial stick offset strictly below 0.9% for an initial signed -10% to +10% bias on each axis, while avoiding loss of deliberately small control movements. No algorithm can promise this for arbitrary unobserved shifts with unknown player intent.

## Implementation and reproducible experiment
- Tests: tests/precision_envelope_v85.rs; no changes to the live engine, gamepad driver, website or model parameters.
- 441 synthetic constant drift cases: independently signed 21 x 21 axis offsets, XInput16 quantization; this includes diagonal radial biases above 10%.
- The synthetic generator explicitly guarantees physical-neutral conditions for 320 calibration frames. Real volunteers' self-reported release does NOT carry this guarantee.
- Candidate: existing GuardAdaptive core with OutputPolicy::PreserveMicro and auto_alpha=0, i.e. no output deadzone and no blind gameplay recentering.
- Micro probe vectors from 0.02% to 5%; tests check preservation and numerical error, with no time-smoothing.
- Two adversarial tests deliberately demonstrate FAILURE of the 0.9% target for an unverified sudden +10%/-10% jump and a falsely acknowledged 1% neutral input.
- A separate 10,000-frame held-micro-motion test verifies that the center is not silently learned from player input.

## Measured result, Naveax offline Rust execution
- Static signed ±10% grid: 441/441 calibration cases satisfy the target.
- Maximum neutral residual: 0.0333% normalized radial output (target: strictly < 0.9%).
- Maximum absolute intended-vector error in micro probe: 0.0288% normalized stick range (assertion bound 0.2%).
- Automatic center updates: zero. Effective output deadzone: zero.
- Sudden unknown drift: NOT RESOLVED; reported explicitly rather than classified as a success.
- Mislabeled held player motion as neutral: NOT RESOLVED; independent physical neutral confirmation is a prerequisite.

## Limitations and production block
This tests a controlled, static synthetic envelope only. It does not prove a bound on real hardware drift, true player intent, sensor noise, USB/Bluetooth transport, uncalibrated changes, or game behavior. Do not train on the two volunteer sessions or claim their neutral stages are physically verified. Existing blind v0.66 and bounded-stable v0.74 gates remain failed. Production remains BLOCKED; game writes and driver manipulation remain disabled.

## Reproduce on Naveax PC
cargo test --locked --offline --test precision_envelope_v85 -- --nocapture
cargo test --locked --offline

## Next experimental gate
Collect opt-in, independently verified two-window neutral samples from distinct physical gamepads, reject contaminated windows, then predeclare blind holdout criteria. Any unverified sudden drift must remain UNRESOLVED instead of silently removing a held user input.
