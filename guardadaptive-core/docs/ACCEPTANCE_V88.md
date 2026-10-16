# GuardAdaptive v0.88: Ground-truth acceptance research gate

## Purpose
Make the primary target falsifiable: **<0.9% normalized radial remaining error** after signed axis offsets of **-10% to +10%**, while retaining user-intended micro-motion, holds, and reversals. An unverified sudden drift jump is **a test failure**, not a reason to silently recenter on gameplay input.

The real-world goal cannot be guaranteed from the raw stick signal alone: the same controller reading may be true drift or a held intentional movement. This test framework therefore **never unlocks physical acceptance or production**, regardless of favorable numbers.

## Implementation
- Original core: `src/bin/replay_v88.rs`, independently compiled Rust executable.
- Five ordered segments: `neutral`, `micro`, `hold`, `reversal`, `abrupt`.
- At least 32 samples per segment (>=160 overall), no more than 9,000 rows, strictly increasing timestamps <=180,000 ms, finite normalized axes in [-1,+1].
- Input is **eight-column CSV** on standard input:
  `phase,tick_ms,raw_x,raw_y,intended_x,intended_y,output_x,output_y`
- `intended` is an **external annotation** used only by the scorer, not by the filter. Its reliability **cannot be authenticated** by this application.
- Measurements: maximum and RMS vector error for every segment, count of micro movements erased, count of vectors reversed relative to intended motion.
- Research constraints: strictly below 0.9% vector residual in `neutral` and `abrupt`; strictly below 0.2 percentage-point maximum vector error in `micro`, `hold`, and `reversal`, with zero erased and reversed intended movements.
- Output labels are deliberately distinct: `V88_RESEARCH_THRESHOLDS=PASS|FAIL`, `V88_PHYSICAL_ACCEPTANCE=BLOCKED`, `V88_PRODUCTION=BLOCKED`, `V88_GAME_OUTPUT=DISABLED`.

## Run
From `guardadaptive-core/` (in the public GitHub repo):

```powershell
cargo test --locked --offline --bin replay_v88
cargo run --locked --offline --bin replay_v88 -- --synthetic-demo
Get-Content .\unverified_lab_trace.csv -Raw | cargo run --locked --offline --bin replay_v88 -- --score-unverified
```

The final command **does not imply** that the CSV is physically valid. Do not derive such a CSV by labeling ordinary volunteer samples as known intended input.

### Demonstrated conservative failure
Using the synthetic oracle, an explicitly calibrated **constant** +5%/-4% offset is corrected exactly in the tested ideal/noise-free scenario. The same model with a previously unobserved **+10% sudden drift jump** leaves **10.0% normalized vector error**. No blind recentering occurs.

```
PHASE=neutral max_error_pct=0.00000
PHASE=micro max_error_pct=0.00000
PHASE=hold max_error_pct=0.00000
PHASE=reversal max_error_pct=0.00000
PHASE=abrupt max_error_pct=10.00000
V88_RESEARCH_THRESHOLDS=FAIL
V88_PHYSICAL_ACCEPTANCE=BLOCKED
```

The zero errors are **synthetic, idealized** measurements. They are not precision estimates for real controllers or evidence of 100% accuracy.

## Required physical experiment design
1. Collect **multiple physically distinct devices** across truly near-neutral, drifted, and heavily drifted hardware. Pre-register distinct holdout devices; do not count two sessions by the same person as two participants.
2. Establish genuinely independent neutral and intended-position reference, ideally with a mechanical jig / external angular position sensor / optically measured stick position, with timestamp alignment and uncertainty stated. A UI confirmation, camera footage without calibrated reference, or an unchanged controller signal alone is **not** physical ground truth.
3. During calibration, record two independent released-position windows, an intentional excursion, measured clock stability, and full per-axis signal. Reject windows with actual motion, inconsistent centers, missing samples or disturbed release.
4. On separate **blind holdout** sessions, apply the frozen candidate to scripted ±10% static offsets, micro motions under 1%, long holds, fast direction reversals, deliberate diagonal moves, thermal ramps, and **unverified sudden shifts**. Report each class, including failures, separately.
5. Evaluate both **neutral residual (<0.9%)** and **intent preservation**. Calculate max/RMS vector error against independent reference, erased tiny vectors, direction flips, added latency and calibration repeatability. Do not merge class scores into a flattering but misleading global average.
6. No physical acceptance without independent reference calibration/trace integrity review, repeated physical controls, deterministic test results, operator approval, and a new, separately authorized game-output safety review.

## Evidence and privacy limits
The existing two volunteer sessions (5,447 samples, one participant, unreliable category/survey and contaminated idle windows) remain **unverified research data**, never training or blind physical acceptance. The v0.87 `ga-neutral-evidence-v87` JSON is also self-reported; two stable readings cannot prove physical release.

The v0.88 CLI reads stdin only, does not connect to a gamepad, does not write hardware/driver outputs, and emits aggregate diagnostics without raw sample storage. Output content itself can reflect a device behavior; secure any externally saved traces separately, with explicit consent and appropriate deletion/retention policies.

## CI and regression checks
Rust's unit tests exercise the complete signed 441-case static grid, sudden drift fail, phase-order/sample-count validation, malformed/unlabeled input rejection, erased micro-motion, reversed input, and malicious/falsified annotations that could produce a spurious research PASS. **Physical and production acceptance remain blocked regardless of score.**
