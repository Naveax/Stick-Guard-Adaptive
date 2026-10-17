# GuardAdaptive v0.89 stabilization notes

Research only: this Rust observer is not connected to live game or driver outputs.

Fixed: switching timed and untimed observations now resets detection evidence. Clock rollback starts a new observation epoch. Recalibration, rollback, and device invalidation reset clock history. Without a reference, a stable signal stays Uncalibrated rather than being treated as confirmed new drift.

Validation: 17 v0.89 tests, 373 core Rust tests plus integration suites. Tested 441 signed synthetic static offsets and a deterministic 30,000-report stress stream. Clippy all targets with warnings denied, cargo fmt and cargo test passed.

Boundaries: A held intentional stick position can be indistinguishable from real drift. No unknown-center auto-learning, driver writes, hardware verification, or physical below-0.9-percent guarantee. Release remains BLOCKED.
