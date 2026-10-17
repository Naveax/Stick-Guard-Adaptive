# GuardAdaptive v0.93 | Axis activity integrity

This is an offline, read-only audit of previously consented local telemetry, not controller output code.

The `field-session-audit-v93.cjs` report now includes an `axis_activity` section. For every physically represented stick it compares the chosen axis pair against its neutral-phase median and the named full-sweep phase. The signal must have sustained excursion in **each mapped axis**: at least 7.5% from the neutral median for at least 12 samples or 5% of sweep samples (whichever is greater). A lone spike is insufficient.

It also measures activity of the chosen axes in the *opposite* stick's sweep phase and reports if otherwise unselected axes move. Evidence states are `PHASE_ISOLATION_OBSERVED`, `NEUTRAL_UNSTABLE`, `SWEEP_NOT_DEMONSTRATED`, `CROSS_ACTIVITY_IN_OTHER_STICK_PHASE`, and `OTHER_UNMAPPED_AXIS_ACTIVE`.

**Important:** `PHASE_ISOLATION_OBSERVED` is an observational consistency check, not proof of mapping polarity, true physical release, hardware identity, or player intention. No detected pattern permits automatic training, calibration, physical acceptance, production or game output.

Local observations from the original four stored work-pc sessions: the oldest trial has unstable neutral windows on both sticks; among the three newer trials, the first shows both sides phase-consistent, the second has unstable right neutral, and the third (10-axis controller, manually mapped indices 0,1,5,2) shows both sides phase-consistent. All four remain **physically unverified**.

Run from `remote-lab`:

```powershell
node --test field-session-audit-v93.test.cjs
$dir = Join-Path $env:LOCALAPPDATA 'GuardAdaptiveRemoteLabFailover\sessions\telemetry-v1'
node field-session-audit-v93.cjs --data-dir $dir
```

No original JSON file, joystick raw trace, respondent comment or identity is included in the repository. The unit tests generate their own synthetic records. Physical axis direction confirmation, independent stick-angle reference and blinded real-device residual testing remain blocked.
