# GuardAdaptive v0.87 | Local-only two-window evidence

## Objective
Allow a volunteer to save the *observed* two-window neutral calibration measurements to a **local JSON file** after a successful browser-only check. These are research observations, **not** independently verified ground truth.

## Workflow
1. Open either existing GuardAdaptive Remote Lab web page and choose a controller category.
2. Connect a supported controller. Explicitly start the first five-second neutral capture without touching the sticks.
3. Move each available stick beyond the observed 7.5% excursion threshold; release it.
4. Start the second five-second neutral capture.
5. Only if both windows are stable and centers agree, the **Download neutral JSON** button becomes enabled. A local browser download occurs **only on an explicit button click**.
6. Changing the controller, remapping axes, disconnecting or hiding the tab revokes the previous download-ready state.

## Data contract
`ga-neutral-evidence-v87`, `data_origin=VOLUNTEER_SELF_REPORTED_NEUTRAL`. Explicitly false: `physically_verified`, `independent_motion_ground_truth`, `eligible_for_training`, `eligible_for_holdout`. `production_gate=BLOCKED`. Includes only the numeric `first_window` and `second_window` (four mapped axes per observation), two excursion flags, observed center, axes count, and measured stability diagnostics.

No device ID, hardware address, account identity, IP address, survey answers, button states, or server submission is included. **Important:** analog drift profiles can still be distinctive of the physical device, so do not treat the measurements as completely non-identifying in every context. The visitor chooses whether to download and later share the file.

The JavaScript serializer requires two stable windows, finite bounded axes, observed excursions and mutually consistent centers. Import-side validation refuses any positive claims of independent physical evidence or model-training authorization. Malformed or partial captures are not exported.

## Verification
- Both computers: 441 signed static synthetic center positions, malicious evidence flags, window contamination, disagreement, missing movement, and identifiers covered by `neutral-evidence-v87.test.cjs`.
- Real-game input and drivers: **unchanged**.
- Network upload: **none** from the new download button.
- Existing two volunteer sessions were never altered or imported as evidence.

## What remains unproven
Clicking a prompt is not proof of physically releasing a controller. Sensor readings alone cannot distinguish certain held intentional movements from drift. Real `<0.9%` radial residual with preserved micro-motion requires independently controlled physical-release labels and a separate trusted motion reference for blinded holdout trials. No live/game release has been authorized.

## Relevant implementation
`remote-lab/public/neutral-evidence-v87.js` and `naveax-lab/public-simple/neutral-evidence-v87.js` plus explicit download controls in the associated frontends. GitHub CI runs the contract tests for both labs.
