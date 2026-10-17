# GuardAdaptive v0.91: advisory notice throttling

Research component: `src/advisory_v91.rs`.

The component reads existing `Observation` values and returns **presentation decisions only**. It cannot modify analog input, calibration, output, or controller drivers.

A new ambiguous-input episode is displayed immediately. Additional episode starts within 10 seconds are counted as coalesced. Continuous frames from the same episode never generate repeated notices. Invalid inputs cannot create notices. A duplicated/backward timestamp locks notice presentation until an explicit session reset. An updated center alone does not bypass the cooldown.

The eight Rust tests cover boundaries, repeated frames, malformed observations, timestamp errors, center changes, and identical signals arising from held player movement or drift.

Run `cargo test --locked --offline advisory_v91` and `cargo clippy --locked --offline --all-targets -- -D warnings` from `guardadaptive-core/`.

**Limitations:** A warning is not proof of drift. Unknown drift is not automatically corrected. Real controller residual under 0.9% and preserved intentional micro-input remain unverified. The component has no game output or browser integration. Keep production blocked until independent physical testing.
