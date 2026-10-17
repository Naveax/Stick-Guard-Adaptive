# GuardAdaptive v0.89 | Unknown drift, motion-first shadow observer

## Goal and the unavoidable identification limit

User goal: reduce **unknown**, changing analog drift in [-10%,+10%] per axis to **less than 0.9% radial error** without destroying intentional controller input. The goal is a target, **not a verified universal promise**.

A controller sample of +10% with no other independent reference has at least two physically plausible explanations:
- Neutral intent = 0% and unrecognized drift = +10%.
- Deliberate held intent = +10% and drift = 0%.

The observed raw data are identical. A filter cannot output both zero and +10% from that same evidence. Thus it **cannot guarantee** <0.9% neutral residual and essentially perfect user control under all possible unknown drift states without trusted additional information.

## Research implementation

New isolated Rust module `src/unknown_drift_v89.rs` reuses the project's existing operator-confirmed, two-window `GuidedWizard`/`VerifiedCenter` token. It does NOT add any gamepad output, driver hook, hidden online recentering or learning from volunteer records.

- **No calibration yet:** passthrough raw movement, including micro input; quietly search for unexplained stable nonzero plateaus as *hypotheses only*.
- **Authorized calibration:** per-frame direct `raw - center` radial saturation, **no hard deadzone** and no temporal smoothing.
- **Potential new drift:** a stable offset >=1.2% from the currently adopted center is flagged as `MotionOrDriftAmbiguous`. This alert explicitly also includes valid held player movement. The detected candidate NEVER replaces the active center.
- **Time-aware observation:** `observe_at(raw, monotonic_ms)` requires >=800 ms and >=24 consistent reports across either 60Hz or 250Hz; >250 ms gaps and non-increasing clock samples clear/reject the hypothesis. Untimed `observe(raw)` is an offline count-based fallback requiring >=128 consistent frames.
- **Non-destructive reacquisition:** a new explicit, two-window center can be accepted. Single transaction center changes above 15% radial are rejected and leave previous mapping intact. This is a conservative safety bound, not a claim of physically impossible drift.
- **Undo:** the last accepted center, *including the very first calibration*, can be rolled back to the previous center or raw passthrough. Any disconnected/different control device must invalidate its prior center.
- **No automatic adaptation:** `unverified_center_updates` remains zero. An invalid raw sample returns zero only for that invalid sample; no invalid data teach the model.

### Measured signed static synthetic sweep

441 independent signed x/y offsets (from -10% through +10% each axis); synthetic-only calibration uses XInput16 quantization and alternating tiny neutral jitter.

| Measurement | v0.89 isolated test |
| --- | ---: |
| Constant signed offsets after reference | 441/441 passed |
| Max normalized radial neutral residual | **0.00129%** |
| Max intended-vector micro error among tested motions | **0.00301%** |
| Blind/unverified center updates | 0 |
| True change without new trustworthy release | **not solved** |
| Live game/device input changes | **none** |

Note the diagonal of (+10%,+10%) has >10% radial offset; the full independent axis square is tested.

### Adversarial tests

1. Sudden unknown +10%/-10% jump: flags after sufficiently quiet evidence but **does not suppress the observed value** or fake a physical PASS. A new externally authorized release can recenter.
2. Gradual linear drift ramp to +10%: only reports ambiguity **after** a new plateau is observed.
3. Deliberate held +5% input: may trigger an ambiguous warning, **but the input remains intact** under correct calibration.
4. 10,000-frame intentional 0.3%/-0.2% held micro vector: preserved, no automatic center updates.
5. Rapid reversing input: the direct output follows the same-frame corrected raw input.
6. Both 60Hz and 250Hz sample rates; duplicate or reordered timestamps; long interruptions.
7. Large recalibration shift rejected; previous calibration retained; rollback of both later and first ever calibration.
8. False neutral statement: **can still erase real input if accepted**, explicitly documented, not concealed.
9. Invalid samples, changed device/session, unknown centers all follow conservative fallback paths.

## Why this is useful without pretending it is magic

This module can distinguish **observationally suspicious stable behavior** from fast motion and can safely propose a new calibration workflow, including undo. It **cannot distinguish a human holding a stick perfectly still from a drift plateau** based on raw analog data alone. Requiring trusted external release information is essential to avoid eating real player inputs.

The CLI and web sites are intentionally **not connected** to the new observer. Deployment to actual games remains blocked. Existing real volunteer measurements (one participant, two sessions, 5,447 samples) are noisy/unverified and were **not** used to calibrate or train v0.89.

## Next experiment

Collect physical reference data using an independent stick-position measurement (mechanical fixture or calibrated position sensor), across multiple controllers and high/low sampling rates. Freeze the new observer thresholds *before* blinded holdout, measure true false-positive alerts, missed abrupt drift, calibration rollback recovery, vector micro fidelity and latency, and explicitly report unsolved ambiguity cases. A physically verified reference is required before claiming <0.9% in real use.

## Reproduce

From the Rust core directory:

```powershell
cargo test --locked --offline unknown_drift_v89 -- --nocapture
cargo test --locked --offline
cargo fmt --all -- --check
```

**Research-only**. `GAME_OUTPUT=DISABLED`, `TRAINING=BLOCKED`, `PRODUCTION=BLOCKED`.
