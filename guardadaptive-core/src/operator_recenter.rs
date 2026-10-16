//! v0.34 operator-assisted re-centering for drifting controllers.
//! This is a research-only, memory-only preview. No game writes, no inferred
//! operator release: the operator must separately confirm each release.
//! A changed packet epoch/disconnect automatically invalidates the preview.
use crate::{
    guided::{GuidedWizard, Screen},
    reacquire::VerifiedCenter,
    safety_preview::{PreviewMode, SafePreview},
    Vec2,
};
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Calibrating,
    Preview,
    LatchedFault,
}
pub struct Recenter {
    state: State,
    epoch: Option<u64>,
    wizard: Option<GuidedWizard>,
    preview: Option<SafePreview>,
    last: Option<(u64, u32)>,
    committed: usize,
}
impl Default for Recenter {
    fn default() -> Self {
        Self {
            state: State::Off,
            epoch: None,
            wizard: None,
            preview: None,
            last: None,
            committed: 0,
        }
    }
}
impl Recenter {
    pub fn state(&self) -> State {
        self.state
    }
    pub fn commits(&self) -> usize {
        self.committed
    }
    pub fn emergency_off(&mut self) {
        self.state = State::Off;
        self.epoch = None;
        self.wizard = None;
        self.preview = None;
        self.last = None;
    }
    pub fn disconnect(&mut self) {
        self.emergency_off();
    }
    fn fault(&mut self) {
        self.preview = None;
        self.wizard = None;
        self.state = State::LatchedFault;
        self.last = None;
    }
    pub fn begin(&mut self, epoch: u64, operator_consent: bool) -> bool {
        self.emergency_off();
        if !operator_consent {
            return false;
        }
        self.epoch = Some(epoch);
        self.wizard = Some(GuidedWizard::new());
        self.state = State::Calibrating;
        true
    }
    pub fn screen(&self) -> Option<Screen> {
        self.wizard.as_ref().map(|w| w.screen())
    }
    pub fn confirm_release(&mut self) -> io::Result<()> {
        if self.state != State::Calibrating {
            return Err(io::Error::other("no consented calibration in progress"));
        }
        self.wizard
            .as_mut()
            .ok_or_else(|| io::Error::other("no wizard"))?
            .confirm_release()
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }
    pub fn confirm_movement(&mut self) -> io::Result<()> {
        if self.state != State::Calibrating {
            return Err(io::Error::other("no consented calibration in progress"));
        }
        self.wizard
            .as_mut()
            .ok_or_else(|| io::Error::other("no wizard"))?
            .confirm_movement_finished()
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }
    /// Operator only: the caller explicitly declares each released window.
    /// The tool cannot prove that a quiet stick means human release.
    pub fn sample_calibration(&mut self, raw: Vec2, epoch: u64) -> io::Result<()> {
        if self.state != State::Calibrating || self.epoch != Some(epoch) {
            self.fault();
            return Err(io::Error::other("calibration epoch or state changed"));
        }
        let step = self
            .wizard
            .as_mut()
            .ok_or_else(|| io::Error::other("no wizard"))?
            .sample(raw);
        if let Err(err) = step {
            self.fault();
            return Err(io::Error::other(format!("{err:?}")));
        }
        Ok(())
    }
    pub fn finish(&mut self, explicit_user_commit: bool) -> io::Result<VerifiedCenter> {
        if !explicit_user_commit || self.state != State::Calibrating {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fresh operator final approval required",
            ));
        }
        if self.screen() != Some(Screen::Review) {
            return Err(io::Error::other("both release windows incomplete"));
        }
        let wizard = self
            .wizard
            .take()
            .ok_or_else(|| io::Error::other("no wizard"))?;
        let verified = match wizard.commit() {
            Ok(value) => value,
            Err(e) => {
                self.fault();
                return Err(io::Error::other(format!("{e:?}")));
            }
        };
        let preview = SafePreview::from_verified(verified);
        if preview.quality().mode != PreviewMode::Calibrated {
            self.fault();
            return Err(io::Error::other(
                "poor quality release windows; preview refused",
            ));
        }
        self.preview = Some(preview);
        self.state = State::Preview;
        self.last = None;
        self.committed += 1;
        Ok(verified)
    }
    /// Hot path: no center learning, no allocation, no future sample access.
    /// Only in-memory output. If invalid, raw input is passed through.
    pub fn sample_live(&mut self, raw: Vec2, epoch: u64, at_us: u64, packet: u32) -> Vec2 {
        if !raw.finite() || raw.x.abs() > 1.0 || raw.y.abs() > 1.0 {
            self.fault();
            return Vec2::ZERO;
        }
        let raw = raw.clamp();
        if self.state != State::Preview {
            return raw;
        }
        if self.epoch != Some(epoch) {
            self.fault();
            return raw;
        }
        if let Some((last_t, last_p)) = self.last {
            if at_us < last_t || at_us - last_t > 50_000 || packet < last_p {
                self.fault();
                return raw;
            }
        }
        self.last = Some((at_us, packet));
        self.preview.as_mut().map_or(raw, |p| p.process(raw))
    }
}
pub fn verify_scripted(center: Vec2, epoch: u64, seed: u64) -> io::Result<Recenter> {
    use crate::resolution::Resolution;
    let mut c = Recenter::default();
    assert!(c.begin(epoch, true));
    c.confirm_release()?;
    for i in 0..128 {
        let tiny = if (i as u64 + seed).is_multiple_of(2) {
            0.00002
        } else {
            -0.00002
        };
        c.sample_calibration(
            Resolution::XInput16.quantize(center + Vec2::new(tiny, -tiny)),
            epoch,
        )?;
    }
    for i in 0..128 {
        let travel = -0.25 * (i as f64 / 127.0);
        c.sample_calibration(
            Resolution::XInput16.quantize(center + Vec2::new(travel, 0.0)),
            epoch,
        )?;
    }
    c.confirm_movement()?;
    c.confirm_release()?;
    for i in 0..128 {
        let tiny = if (i as u64 + seed).is_multiple_of(2) {
            0.00002
        } else {
            -0.00002
        };
        c.sample_calibration(
            Resolution::XInput16.quantize(center + Vec2::new(tiny, -tiny)),
            epoch,
        )?;
    }
    c.finish(true)?;
    Ok(c)
}
#[derive(Clone, Copy, Default)]
pub struct Metrics {
    pub frames: u64,
    pub baseline_sse: f64,
    pub assisted_sse: f64,
    pub micro_total: u64,
    pub baseline_micro_lost: u64,
    pub assisted_micro_lost: u64,
    pub baseline_wrong_dir: u64,
    pub assisted_wrong_dir: u64,
}
impl Metrics {
    fn observe(&mut self, intent: Vec2, baseline: Vec2, assisted: Vec2) {
        self.frames += 1;
        self.baseline_sse += (baseline - intent).length_squared();
        self.assisted_sse += (assisted - intent).length_squared();
        if intent.norm() > 0.0 && intent.norm() <= 0.01 {
            self.micro_total += 1;
            if baseline.norm() < 0.5 * intent.norm() {
                self.baseline_micro_lost += 1;
            }
            if assisted.norm() < 0.5 * intent.norm() {
                self.assisted_micro_lost += 1;
            }
        }
        if intent.norm() > 0.0001 {
            if (baseline.x * intent.x + baseline.y * intent.y) < 0.0 {
                self.baseline_wrong_dir += 1;
            }
            if (assisted.x * intent.x + assisted.y * intent.y) < 0.0 {
                self.assisted_wrong_dir += 1;
            }
        }
    }
    pub fn rmse_baseline(&self) -> f64 {
        (self.baseline_sse / self.frames as f64).sqrt()
    }
    pub fn rmse_assisted(&self) -> f64 {
        (self.assisted_sse / self.frames as f64).sqrt()
    }
    pub fn relative_improvement(&self) -> f64 {
        if self.baseline_sse == 0.0 {
            0.0
        } else {
            1.0 - (self.assisted_sse / self.baseline_sse).sqrt()
        }
    }
}
pub const SCENARIOS: [&str; 4] = [
    "severe_constant_drift",
    "opposite_constant_drift",
    "moderate_constant_drift",
    "post_calibration_thermal_shift",
];
pub struct Bench {
    pub seed: u64,
    pub scenarios: [Metrics; 4],
    pub all_micro_preserved: bool,
    pub all_synthetic_scenarios_helped: bool,
    pub real_device_qualified: bool,
}
fn randomized_noise(x: &mut u64) -> f64 {
    *x ^= *x >> 12;
    *x ^= *x << 25;
    *x ^= *x >> 27;
    let n = x.wrapping_mul(0x2545F4914F6CDD1D);
    ((n >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}
pub fn evaluate(seed: u64, count: usize) -> io::Result<Bench> {
    use crate::resolution::Resolution;
    if !(4096..=1_000_000).contains(&count) || !count.is_multiple_of(4) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frames must be 4096..1000000 and divisible by 4",
        ));
    }
    let shifts = [
        Vec2::new(0.42, -0.23),
        Vec2::new(-0.35, 0.27),
        Vec2::new(0.17, 0.14),
        Vec2::new(0.24, -0.18),
    ];
    let mut result = [Metrics::default(); 4];
    for (case, drift) in shifts.into_iter().enumerate() {
        let epoch = case as u64 + 1;
        let mut monitor = verify_scripted(drift, epoch, seed)?;
        let mut rng = seed ^ ((case as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15));
        for i in 0..count / 4 {
            let t = i % (count / 4 / 4).max(1);
            let intent = match (i / (count / 4 / 4).max(1)) % 4 {
                0 => Vec2::ZERO,
                1 => Vec2::new(if t.is_multiple_of(2) { 0.002 } else { -0.002 }, 0.0005),
                2 => Vec2::new(if t % 32 < 16 { 0.35 } else { -0.35 }, -0.2),
                _ => Vec2::new(0.4 * (t as f64 / (count / 16).max(1) as f64), -0.12),
            };
            let drift_now = if case == 3 && i >= count / 8 {
                drift + Vec2::new(-0.22, 0.16)
            } else {
                drift
            };
            let jitter = Vec2::new(
                randomized_noise(&mut rng) * 0.00012,
                randomized_noise(&mut rng) * 0.00012,
            );
            let raw = Resolution::XInput16.quantize((intent + drift_now + jitter).clamp());
            let assisted = monitor.sample_live(raw, epoch, i as u64 * 8_000, i as u32 / 2);
            result[case].observe(intent, raw, assisted);
        }
        assert_eq!(monitor.commits(), 1);
    }
    let all_micro_preserved = result
        .iter()
        .all(|r| r.assisted_micro_lost <= r.baseline_micro_lost);
    let all_synthetic_scenarios_helped = result.iter().all(|r| r.relative_improvement() > 0.01);
    Ok(Bench {
        seed,
        scenarios: result,
        all_micro_preserved,
        all_synthetic_scenarios_helped,
        real_device_qualified: false,
    })
}
impl Bench {
    pub fn json(&self) -> String {
        let cases = SCENARIOS
            .iter()
            .zip(self.scenarios.iter())
            .map(|(name, m)| {
                format!(
                    concat!(
                        "\"{}\":{{\"frames\":{},\"baseline_rmse\":{:.10},",
                        "\"operator_assisted_rmse\":{:.10},\"relative_improvement\":{:.10},",
                        "\"micro_frames\":{},\"baseline_micro_lost\":{},",
                        "\"assisted_micro_lost\":{},\"baseline_wrong_direction\":{},",
                        "\"assisted_wrong_direction\":{}}}"
                    ),
                    name,
                    m.frames,
                    m.rmse_baseline(),
                    m.rmse_assisted(),
                    m.relative_improvement(),
                    m.micro_total,
                    m.baseline_micro_lost,
                    m.assisted_micro_lost,
                    m.baseline_wrong_dir,
                    m.assisted_wrong_dir
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"explicit_operator_recenter_synthetic_benchmark\",",
                "\"seed\":{},\"cases\":{{{}}},",
                "\"all_micro_preserved_vs_uncorrected\":{},",
                "\"all_synthetic_scenarios_helped\":{},",
                "\"operator_release_and_commit_are_scripted_not_physical\":true,",
                "\"drift_and_intention_labels_are_synthetic_only\":true,",
                "\"automatic_recenter_enabled\":false,",
                "\"real_device_qualified\":false,\"game_writes\":0,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.seed, cases, self.all_micro_preserved, self.all_synthetic_scenarios_helped
        )
    }
}
pub fn run(seed: u64, count: usize, out: &std::path::Path) -> io::Result<()> {
    let eval = evaluate(seed, count)?;
    crate::truth_lab::save_new(out, &eval.json())?;
    println!("V034 OPERATOR-APPROVED RECENTER: {count} synthetic frames, 4 scenarios");
    for (name, score) in SCENARIOS.iter().zip(eval.scenarios.iter()) {
        println!(
            " {name}: raw_rmse={:.6} assisted_rmse={:.6} relative_gain={:.2}%, micro_lost={}/{}",
            score.rmse_baseline(),
            score.rmse_assisted(),
            score.relative_improvement() * 100.0,
            score.assisted_micro_lost,
            score.micro_total
        );
    }
    println!("AUTOMATIC_LEARNING=OFF, PHYSICAL_GROUND_TRUTH=NONE, GAME_WRITES=0, PROD=BLOCKED");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_implicit_consent_and_explicit_release_phases() {
        let mut p = Recenter::default();
        assert!(!p.begin(5, false));
        assert_eq!(
            p.sample_live(Vec2::new(0.4, 0.1), 5, 0, 1),
            Vec2::new(0.4, 0.1)
        );
        assert!(p.begin(5, true));
        assert_eq!(p.screen(), Some(Screen::AskFirstRelease));
        assert!(p.finish(true).is_err());
        assert!(p.confirm_movement().is_err());
        assert!(p.finish(false).is_err());
    }
    #[test]
    fn severe_static_drift_reduced_only_with_scripted_operator_approval() {
        let p = evaluate(20261034, 6400).unwrap();
        for case in &p.scenarios[0..3] {
            assert!(
                case.relative_improvement() > 0.5,
                "{}",
                case.relative_improvement()
            );
            assert!(case.assisted_micro_lost <= case.baseline_micro_lost);
        }
        assert!(!p.real_device_qualified);
        assert!(p.json().contains("\"automatic_recenter_enabled\":false"));
    }
    #[test]
    fn disconnect_and_packet_fault_revoke_calibration() {
        let mut p = verify_scripted(Vec2::new(0.30, -0.12), 2, 1).unwrap();
        assert!(p.sample_live(Vec2::new(0.30, -0.12), 2, 1000, 1).norm() < 0.001);
        assert_eq!(
            p.sample_live(Vec2::new(0.31, -0.12), 3, 2000, 2),
            Vec2::new(0.31, -0.12)
        );
        assert_eq!(p.state(), State::LatchedFault);
        assert_eq!(
            p.sample_live(Vec2::new(0.31, -0.12), 3, 3000, 3),
            Vec2::new(0.31, -0.12)
        );
        assert!(!p.begin(4, false));
        p = verify_scripted(Vec2::new(0.30, -0.12), 4, 1).unwrap();
        let _ = p.sample_live(Vec2::new(0.30, -0.12), 4, 1000, 10);
        assert_eq!(
            p.sample_live(Vec2::new(0.30, -0.12), 4, 81000, 11),
            Vec2::new(0.30, -0.12)
        );
        assert_eq!(p.state(), State::LatchedFault);
        p.emergency_off();
        assert_eq!(p.state(), State::Off);
    }
    #[test]
    fn invalid_neutral_windows_and_no_excursion_cannot_commit() {
        let mut p = Recenter::default();
        p.begin(1, true);
        p.confirm_release().unwrap();
        for _ in 0..128 {
            p.sample_calibration(Vec2::new(0.2, 0.0), 1).unwrap();
        }
        for _ in 0..128 {
            p.sample_calibration(Vec2::new(0.2, 0.0), 1).unwrap();
        }
        assert!(p.confirm_movement().is_err());
        assert!(p.finish(true).is_err());
        p.begin(2, true);
        p.confirm_release().unwrap();
        for i in 0..128 {
            let raw = Vec2::new(if i % 2 == 0 { 0.2 } else { 0.5 }, 0.0);
            if p.sample_calibration(raw, 2).is_err() {
                break;
            }
        }
        assert_eq!(p.state(), State::LatchedFault);
    }
    #[test]
    fn disagreeing_releases_lock_the_session_until_reauthorized() {
        let mut p = Recenter::default();
        assert!(p.begin(7, true));
        p.confirm_release().unwrap();
        for _ in 0..128 {
            p.sample_calibration(Vec2::new(0.30, -0.10), 7).unwrap();
        }
        for _ in 0..128 {
            p.sample_calibration(Vec2::new(0.00, -0.10), 7).unwrap();
        }
        p.confirm_movement().unwrap();
        p.confirm_release().unwrap();
        for _ in 0..128 {
            p.sample_calibration(Vec2::new(0.40, -0.10), 7).unwrap();
        }
        assert!(p.finish(true).is_err());
        assert_eq!(p.state(), State::LatchedFault);
        assert_eq!(
            p.sample_live(Vec2::new(0.40, -0.10), 7, 1000, 2),
            Vec2::new(0.40, -0.10)
        );
        assert_eq!(p.commits(), 0);
    }
    #[test]
    fn mixed_case_thermal_drift_is_reported_not_hid() {
        let p = evaluate(20261034, 6400).unwrap();
        assert_eq!(p.scenarios[3].frames, 1600);
        assert!(p.scenarios[3].rmse_assisted() > 0.01);
        assert!(p.json().contains("\"post_calibration_thermal_shift\""));
    }
}
