# GuardAdaptive v0.86 | Neutral preview & read-only volunteer replay

## Implemented

- **Naveax live website** now hosts the conservative two-neutral-window browser preview: `remote-lab/public-simple/idle-quality.js`, `neutral-preview.js`, `neutral-ui.js`, updated `app.js`, `server.cjs`, HTML and styles.
- Exactly two 5-second stationary windows with an observed excursion between them. Each must pass a conservative stability/noise screen and the center difference must be <=0.006 normalized radial units. Player acknowledgment and stable axes are **not** independently verified physical-neutral truth.
- The comparison shows live **raw**, **12% fixed radial deadzone**, and **center subtraction with zero deadzone** on each stick. It is a browser preview only; no game writes, virtual controller output, driver hooks, uploads, background training or model promotion.
- Browser flow isolated from existing 6-stage telemetry and 20-second preview; locks conflicting buttons. Detach/visibility/axis-change invalidates the current preview.
- Rust CLI `src/bin/replay_v86.rs` consumes strict axis-only CSV from standard input; `remote-lab/tools/replay-v86-input.cjs` validates/converts original Naveax session locally. Converter for work-pc `remote-lab/replay-v86-workpc-input.cjs` is separate.
- Rust replay compares raw RMS, fixed 8% deadzone RMS, and **uncalibrated** `GuardAdaptive::with_policy(PreserveMicro)` output. It explicitly disables learning (`auto_alpha=0`), reports unreliable neutral windows, and NEVER treats output RMS as verified drift correction.

## Read-only real volunteer measurements

Both samples come from **one volunteer** with reported mislabeled/rushed tests; neither physical release nor intended movement has independent ground truth.

| Phase | Naveax | work-pc |
| --- | ---: | ---: |
| Samples overall | 3,960 | 1,487 |
| Left small-movement phase samples | 660 | 248 |
| Small-movement samples with nonzero raw signal entirely erased by 8% deadzone | 460 | 89 |
| Fraction erased | 69.7% | 35.9% |
| Left neutral observational quality | UNSTABLE | UNSTABLE |
| Right neutral observational quality | UNSTABLE | UNSTABLE |
| Rust live auto center updates | 0 | 0 |

**Warning:** These erased samples are not necessarily intended player movements. The records cannot justify either a %0.9 real-device guarantee or model training. Large neutral-phase spikes are present, and radial stick magnitudes can exceed 100% for diagonal positions; this is not itself a physical overflow error.

work-pc axes were temporarily converted to a numeric-only CSV stream for Rust execution on Naveax (without IDs, buttons, comments, surveys, or consent data); the transient data file was removed immediately afterward. Original volunteer files were not modified. No real recordings were added to the repository.

## Validation

- Naveax browser two-step synthetic flow on work-pc isolated Edge: first neutral -> both stick excursions -> second neutral -> accepted center preview; output showed raw 6.71%, fixed deadzone 0.00%, center-only 0.00% for synthetic static axes.
- Work-pc analogous preview previously passed, including rejection of disagreeing neutral windows.
- v0.85 fixed signed ±10% synthetic 441-point grid: max synthetic neutral residual 0.0333%, max micro-vector error 0.0288% (not physically validated).
- New Rust `replay_v86` has four unit tests for corrupt format, zero learning, ambiguous held input, and contaminated neutral.
- Original Naveax website + backend test suite, including work-pc-shared neutral preview tests, checked independently.

## Reproduction

```powershell
cd C:\Users\navea\source\GuardAdaptive
cargo test --locked --offline
$path = Join-Path $env:LOCALAPPDATA 'GuardAdaptiveRemoteLabStable\submissions\<session_id>.json'
node remote-lab\tools\replay-v86-input.cjs $path | cargo run --quiet --locked --offline --bin replay_v86
```

## Acceptance blocker

True `<0.9%` drift error **and** preserved intentional micro-input requires blinded, physically verified holdout recordings from multiple controllers. An arbitrary unobserved drift jump and a held deliberate input can be observationally identical, so they cannot both be perfectly corrected without additional trusted evidence. Production remains BLOCKED.
