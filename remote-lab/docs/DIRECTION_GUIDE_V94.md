# GuardAdaptive v0.94 | Direction guide and cross-test safety

## Scope

Read-only interactive **browser Gamepad API** guide for controllers with at least 2 axes. It is part of the work-pc web research lab and is intentionally separate from live game inputs, training, and Rust calibration.

- `public/axis-direction-v94.js` is loaded after `app.js`, exposes a pure analysis API for local Node tests, and installs a browser guide in the axis-mapping panel.
- Each user-requested directional hold is measured for 1.3 seconds after an explicit click. The guide begins with a neutral observation that must be stable over at least 25 samples, but **stability is not proof of physical release**.
- Left/right and up/down are checked independently on each existing mapped axis. Unstable/weak motion, reversed sign, excessive orthogonal movement and movement in the other stick are flagged instead of silently remapping.
- For a 2-axis device only the left four directions are tested. A 10-axis controller may use `[0,1,5,2]`, but this is only a test of the user's current manual mapping.
- The module never changes selected mappings, drift centers, game input, browser storage or controller firmware. It does not send telemetry or create an export.
- If the selected device, mapping or browser focus changes, the guide cancels and drops all temporary samples.
- Existing short tests, calibration, filter preview and full study cannot begin while the guide is active; the guide also refuses to start during those tests.
- The results are **consistent or inconsistent with manual instructions**, not independent physical proof of direction or intention, and cannot qualify any session for training.

## Status of previous records

The user explained the earlier second trial's unstable right-neutral reading was an **operator-handling error**. Preserve that observation but do not treat it as proven hardware damage or the main project blocker. That specific window is unsuitable for neutral calibration; the other recorded data may still be reviewed under their own quality flags. No source recording is deleted or altered.

## Reproduce

```powershell
cd remote-lab
node --check public/axis-direction-v94.js
node --test axis-direction-v94.test.cjs
node --test *.test.cjs test.cjs
```

The six new synthetic tests cover the inexpensive 10-axis mapping, inverted Y, cross-stick interference, a single noisy spike, invalid/unstable neutral, and 2-axis controllers. Existing server HTTP tests check that the guide asset is accessible from the page.

## P0/P1/P2 gates

**P0:** A local guide now exists for manual direction checks. Actual physical axis polarity and independently measured released neutral remain unverified; unknown drift removal <0.9% remains unsolved.

**P1:** New axis checks run alongside existing data quality, telemetry, v0.89/v0.90 Rust observers, v0.92 reference gate and v0.93 records audit. This is not the Rust core running inside the browser, and device-to-display latency remains unmeasured.

**P2:** Users can perform and cancel guided direction checks. No automatic production unlock, no user-facing claims of repaired hardware, no output drivers, and no hidden uploads. Physical acceptance requires a separate independent motion-reference study and explicit safety review.

`PRODUCTION=BLOCKED`; `REAL_GAME_INPUT=DISABLED`; `PHYSICAL_ACCURACY=PENDING`.
