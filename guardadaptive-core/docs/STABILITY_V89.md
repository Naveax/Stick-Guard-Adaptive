# GuardAdaptive v0.89 stabilization notes

Research only: this Rust observer is not connected to live game or driver outputs.

Fixed: switching timed and untimed observations now resets detection evidence. Clock rollback starts a new observation epoch. Recalibration, rollback, and device invalidation reset clock history. Without a reference, a stable signal stays Uncalibrated rather than being treated as confirmed new drift.

Validation: 17 v0.89 tests, 373 core Rust tests plus integration suites. Tested 441 signed synthetic static offsets and a deterministic 30,000-report stress stream. Clippy all targets with warnings denied, cargo fmt and cargo test passed.

Boundaries: A held intentional stick position can be indistinguishable from real drift. No unknown-center auto-learning, driver writes, hardware verification, or physical below-0.9-percent guarantee. Release remains BLOCKED.

CI parity correction: Windows GitHub Actions had selected Rust 1.99, while local verification used Rust 1.93.0. The workflow now installs/pins 1.93.0 with Clippy. An additional equivalent filename-suffix simplification in profile_inventory.rs resolves the newer Clippy warning without changing parsing behavior.

2026-10-17 additional stability pass:
- Invalid or nonfinite raw axes are rejected before timestamp validation. Previously an out-of-range axis accompanied by a duplicate clock could reach the timestamp branch and emit nonzero clamped output. It now returns InvalidInput with zero output and cannot enter center learning.
- Ambiguity metrics include ambiguous_episode_starts, counting alert onset only once per continuous plateau. Existing ambiguous_stable_reports retains the previous per-frame diagnostic semantics.
- v0.89 regression coverage increased from 17 to 21 tests, including conflicting invalid axis/timestamp, timestamp near u64::MAX, long held plateaus and fresh episodes after invalid samples.
- 377 core Rust tests + 4, 7 and 4 auxiliary tests, cargo fmt and strict clippy passed locally.
- All work remains in the isolated research observer. There is no trusted physical reference, no driver/game output, and no automatic unknown-drift center update. Physical acceptance stays BLOCKED.
