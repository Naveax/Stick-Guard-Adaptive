//! Transactional, user-initiated neutral re-acquisition protocol.
//! This is NOT an automatic oracle-free drift detector. Physical user confirmation
//! remains essential. Two independent neutral windows must agree, with a genuine
//! observed excursion between them; else no calibration is committed.
use crate::Vec2;
const SAMPLES: usize = 128;
const MAX_MAD: f64 = 0.008;
const MAX_WINDOW_DISAGREEMENT: f64 = 0.015;
const MIN_EXCURSION: f64 = 0.075;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    FirstNeutral,
    Movement,
    SecondNeutral,
    Ready,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReacquireError {
    WrongStage,
    IncompleteWindow,
    NoExcursion,
    InvalidSample,
    NoisyNeutral,
    WindowDisagreement,
}
#[derive(Clone, Copy, Debug)]
pub struct VerifiedCenter {
    center: Vec2,
    pub neutral_mad: Vec2,
    pub agreement: f64,
}
impl VerifiedCenter {
    pub fn center(&self) -> Vec2 {
        self.center
    }
    /// Reconstruct a previously validated snapshot. Only profile_store may
    /// call this after device identity, schema, integrity, and bounds checks.
    pub(crate) fn restore_checked(center: Vec2, mad: Vec2, agreement: f64) -> Option<Self> {
        if !center.finite()
            || center.x.abs() > 1.0
            || center.y.abs() > 1.0
            || !mad.finite()
            || mad.x < 0.0
            || mad.y < 0.0
            || mad.x > MAX_MAD
            || mad.y > MAX_MAD
            || !agreement.is_finite()
            || !(0.0..=MAX_WINDOW_DISAGREEMENT).contains(&agreement)
        {
            return None;
        }
        Some(Self {
            center,
            neutral_mad: mad,
            agreement,
        })
    }
}
pub struct ReacquireSession {
    stage: Stage,
    first: [Vec2; SAMPLES],
    second: [Vec2; SAMPLES],
    count: usize,
    first_center: Vec2,
    saw_excursion: bool,
}
impl Default for ReacquireSession {
    fn default() -> Self {
        Self::new()
    }
}
impl ReacquireSession {
    pub fn new() -> Self {
        Self {
            stage: Stage::FirstNeutral,
            first: [Vec2::ZERO; SAMPLES],
            second: [Vec2::ZERO; SAMPLES],
            count: 0,
            first_center: Vec2::ZERO,
            saw_excursion: false,
        }
    }
    pub fn stage(&self) -> Stage {
        self.stage
    }
    pub fn samples_needed(&self) -> usize {
        SAMPLES.saturating_sub(self.count)
    }
    fn check(&mut self, s: Vec2) -> Result<(), ReacquireError> {
        if !s.finite() || s.x.abs() > 1.0 || s.y.abs() > 1.0 {
            self.stage = Stage::Rejected;
            return Err(ReacquireError::InvalidSample);
        }
        Ok(())
    }
    /// Only call while the operator is asked to RELEASE the stick.
    pub fn sample_released(&mut self, s: Vec2) -> Result<(), ReacquireError> {
        if !matches!(self.stage, Stage::FirstNeutral | Stage::SecondNeutral) {
            return Err(ReacquireError::WrongStage);
        }
        self.check(s)?;
        if self.count == SAMPLES {
            return Err(ReacquireError::WrongStage);
        }
        match self.stage {
            Stage::FirstNeutral => self.first[self.count] = s,
            Stage::SecondNeutral => self.second[self.count] = s,
            _ => unreachable!(),
        }
        self.count += 1;
        if self.count == SAMPLES {
            let (center, mad) = summarize(&self.window()?);
            if mad.x > MAX_MAD || mad.y > MAX_MAD {
                self.stage = Stage::Rejected;
                return Err(ReacquireError::NoisyNeutral);
            }
            if self.stage == Stage::FirstNeutral {
                self.first_center = center;
                self.stage = Stage::Movement;
                self.count = 0;
            } else {
                self.stage = Stage::Ready;
            }
        }
        Ok(())
    }
    fn window(&self) -> Result<[Vec2; SAMPLES], ReacquireError> {
        match self.stage {
            Stage::FirstNeutral => Ok(self.first),
            Stage::SecondNeutral => Ok(self.second),
            _ => Err(ReacquireError::WrongStage),
        }
    }
    /// Called while user follows guidance to move right, left, up, down.
    /// An excursion alone is not reliable proof of an intentional movement.
    pub fn sample_exercise(&mut self, s: Vec2) -> Result<(), ReacquireError> {
        if self.stage != Stage::Movement {
            return Err(ReacquireError::WrongStage);
        }
        self.check(s)?;
        if (s - self.first_center).norm() >= MIN_EXCURSION {
            self.saw_excursion = true;
        }
        Ok(())
    }
    /// Operator confirms RELEASE again, after visible movement was recorded.
    pub fn begin_second_release(&mut self) -> Result<(), ReacquireError> {
        if self.stage != Stage::Movement {
            return Err(ReacquireError::WrongStage);
        }
        if !self.saw_excursion {
            return Err(ReacquireError::NoExcursion);
        }
        self.stage = Stage::SecondNeutral;
        self.count = 0;
        Ok(())
    }
    /// Commit is an explicit user action, not an inferred property of input.
    pub fn finish(self) -> Result<VerifiedCenter, ReacquireError> {
        if self.stage != Stage::Ready {
            return Err(ReacquireError::IncompleteWindow);
        }
        let (center2, mad2) = summarize(&self.second);
        let (center1, mad1) = summarize(&self.first);
        let agreement = (center1 - center2).norm();
        if agreement > MAX_WINDOW_DISAGREEMENT {
            return Err(ReacquireError::WindowDisagreement);
        }
        let center = (center1 + center2) * 0.5;
        Ok(VerifiedCenter {
            center,
            neutral_mad: Vec2::new(mad1.x.max(mad2.x), mad1.y.max(mad2.y)),
            agreement,
        })
    }
}
fn median(data: &mut [f64; SAMPLES]) -> f64 {
    data.sort_by(|a, b| a.total_cmp(b));
    (data[SAMPLES / 2 - 1] + data[SAMPLES / 2]) * 0.5
}
fn summarize(data: &[Vec2; SAMPLES]) -> (Vec2, Vec2) {
    let mut x = std::array::from_fn(|i| data[i].x);
    let mut y = std::array::from_fn(|i| data[i].y);
    let mx = median(&mut x);
    let my = median(&mut y);
    let mut dx = std::array::from_fn(|i| (data[i].x - mx).abs());
    let mut dy = std::array::from_fn(|i| (data[i].y - my).abs());
    (
        Vec2::new(mx, my),
        Vec2::new(median(&mut dx) * 1.4826, median(&mut dy) * 1.4826),
    )
}
/// Paired synthetic recovery experiment. Only externally scripted and
/// explicitly released calibration windows authorize center updates.
/// Desired motion is evaluator truth, never passed to MotionFirst::process.
pub fn demo(out: &std::path::Path) -> std::io::Result<()> {
    use crate::{
        motion_first::{MotionFirst, MotionFirstConfig},
        resolution::Resolution,
        InputMode,
    };
    use std::{fs, io::Write};
    let cfg = MotionFirstConfig {
        input_lsb: Resolution::XInput16.positive_lsb(),
        softness_lsb: 0.5,
    };
    let mut blind = MotionFirst::new(cfg);
    let mut assisted = MotionFirst::new(cfg);
    for _ in 0..400 {
        blind.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        assisted.process(Vec2::ZERO, InputMode::CalibrateNeutral);
    }
    let drift = Vec2::new(0.55, -0.2);
    let mut protocol = ReacquireSession::new();
    for t in 0..128 {
        let noise = if t % 2 == 0 { 0.00003 } else { -0.00003 };
        let raw = Resolution::XInput16.quantize(drift + Vec2::new(noise, -noise));
        protocol
            .sample_released(raw)
            .expect("scripted first released window");
        blind.process(raw, InputMode::Live);
        assisted.process(raw, InputMode::Live);
    }
    for t in 0..128 {
        let intentional = Vec2::new(-0.20 * (t as f64 / 127.0).sin(), 0.0);
        let raw = Resolution::XInput16.quantize(drift + intentional);
        protocol
            .sample_exercise(raw)
            .expect("scripted movement exercise");
        blind.process(raw, InputMode::Live);
        assisted.process(raw, InputMode::Live);
    }
    protocol
        .begin_second_release()
        .expect("scripted excursion was observed");
    for t in 0..128 {
        let noise = if t % 2 == 0 { 0.00003 } else { -0.00003 };
        let raw = Resolution::XInput16.quantize(drift + Vec2::new(noise, -noise));
        protocol
            .sample_released(raw)
            .expect("scripted second released window");
        blind.process(raw, InputMode::Live);
        assisted.process(raw, InputMode::Live);
    }
    let verification = protocol.finish().expect("stable two-release calibration");
    let verified_center = verification.center();
    assisted.accept_reacquisition(verification);

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = std::io::BufWriter::new(fs::File::create(out)?);
    writeln!(file,"sample,desired_x,desired_y,raw_x,raw_y,blind_x,blind_y,assisted_x,assisted_y,blind_error,assisted_error")?;
    let mut count = 0_u64;
    let mut blind_ss = 0.0;
    let mut assisted_ss = 0.0;
    let inputs = [
        Vec2::ZERO,
        Vec2::new(0.0001, 0.0),
        Vec2::new(-0.02, 0.03),
        Vec2::new(0.4, -0.2),
    ];
    for i in 0..4000 {
        let desired = inputs[(i / 250) % inputs.len()];
        let noise = 0.00003 * ((i as f64) * 1.17).sin();
        let raw = Resolution::XInput16.quantize(drift + desired + Vec2::new(noise, -noise));
        let output_blind = blind.process(raw, InputMode::Live);
        let output_assisted = assisted.process(raw, InputMode::Live);
        let be = (output_blind - desired).norm();
        let ae = (output_assisted - desired).norm();
        blind_ss += be * be;
        assisted_ss += ae * ae;
        count += 1;
        writeln!(
            file,
            "{},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8}",
            i,
            desired.x,
            desired.y,
            raw.x,
            raw.y,
            output_blind.x,
            output_blind.y,
            output_assisted.x,
            output_assisted.y,
            be,
            ae
        )?;
    }
    file.flush()?;
    let blind_rmse = (blind_ss / count as f64).sqrt();
    let assisted_rmse = (assisted_ss / count as f64).sqrt();
    println!("REACQUIRE DEMO: 2 * 128 simulated operator-confirmed neutral reports, separated by observed movement");
    println!(
        "Verified center=({:.6},{:.6}) drift truth=({:.6},{:.6})",
        verified_center.x, verified_center.y, drift.x, drift.y
    );
    println!(
        "4000 evaluation frames: blind_rmse={:.8} assisted_rmse={:.8} trace={}",
        blind_rmse,
        assisted_rmse,
        out.display()
    );
    println!("Assistance works here because user release is externally confirmed; this is NOT autonomous drift detection.");
    if assisted_rmse >= blind_rmse * 0.1 {
        return Err(std::io::Error::other(
            "calibrated candidate did not improve scripted stable drift enough",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn release(s: &mut ReacquireSession, center: Vec2) {
        for i in 0..SAMPLES {
            let jitter = if i % 2 == 0 { 0.0003 } else { -0.0003 };
            s.sample_released((center + Vec2::new(jitter, -jitter)).clamp())
                .unwrap();
        }
    }
    #[test]
    fn stable_two_window_recovery_of_90_percent_drift() {
        let mut s = ReacquireSession::new();
        let offset = Vec2::new(0.9, -0.05);
        release(&mut s, offset);
        assert_eq!(s.stage(), Stage::Movement);
        assert_eq!(s.begin_second_release(), Err(ReacquireError::NoExcursion));
        s.sample_exercise(offset + Vec2::new(-0.25, 0.0)).unwrap();
        s.begin_second_release().unwrap();
        release(&mut s, offset);
        let good = s.finish().unwrap();
        assert!((good.center() - offset).norm() < 1e-8);
        assert!(good.neutral_mad.x < 0.001);
    }
    #[test]
    fn changing_center_rejected_without_commit() {
        let mut s = ReacquireSession::new();
        release(&mut s, Vec2::new(0.35, 0.0));
        s.sample_exercise(Vec2::new(0.0, 0.0)).unwrap();
        s.begin_second_release().unwrap();
        release(&mut s, Vec2::new(0.50, 0.0));
        assert!(matches!(
            s.finish(),
            Err(ReacquireError::WindowDisagreement)
        ));
    }
    #[test]
    fn movement_disguised_as_neutral_is_rejected_if_unstable() {
        let mut s = ReacquireSession::new();
        for i in 0..SAMPLES {
            let candidate = Vec2::new(i as f64 / SAMPLES as f64 * 0.2, 0.0);
            let result = s.sample_released(candidate);
            if i == SAMPLES - 1 {
                assert_eq!(result, Err(ReacquireError::NoisyNeutral));
            }
        }
        assert_eq!(s.stage(), Stage::Rejected);
    }
    #[test]
    fn invalid_samples_fail_closed() {
        let mut s = ReacquireSession::new();
        assert_eq!(
            s.sample_released(Vec2::new(f64::NAN, 0.)),
            Err(ReacquireError::InvalidSample)
        );
        assert_eq!(s.stage(), Stage::Rejected);
        assert!(matches!(s.finish(), Err(ReacquireError::IncompleteWindow)));
    }
    #[test]
    fn neutral_reacquisition_covers_amplitude_and_direction_envelope() {
        for magnitude in [0.00001, 0.0001, 0.001, 0.01, 0.1, 0.5, 0.9, 1.0] {
            for octant in 0..16 {
                let theta = std::f64::consts::TAU * octant as f64 / 16.0;
                let offset = Vec2::new(magnitude * theta.cos(), magnitude * theta.sin());
                let offset = Vec2::new(offset.x.clamp(-1.0, 1.0), offset.y.clamp(-1.0, 1.0));
                let mut s = ReacquireSession::new();
                release(&mut s, offset);
                s.sample_exercise(
                    (offset + Vec2::new(if offset.x >= 0.0 { -0.1 } else { 0.1 }, 0.0)).clamp(),
                )
                .unwrap();
                s.begin_second_release().unwrap();
                release(&mut s, offset);
                let v = s.finish().unwrap();
                assert!(
                    (v.center() - offset).norm() < 0.0006,
                    "amp={magnitude} octant={octant} measured={:?}",
                    v.center()
                );
            }
        }
    }
    #[test]
    fn rejected_two_release_transaction_never_modifies_filter_center() {
        use crate::{
            motion_first::{MotionFirst, MotionFirstConfig},
            InputMode,
        };
        let mut guard = MotionFirst::new(MotionFirstConfig::default());
        for _ in 0..256 {
            guard.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        let old = guard.center();
        let mut s = ReacquireSession::new();
        release(&mut s, Vec2::new(0.5, 0.0));
        s.sample_exercise(Vec2::ZERO).unwrap();
        s.begin_second_release().unwrap();
        release(&mut s, Vec2::new(0.65, 0.0));
        assert!(s.finish().is_err());
        assert_eq!(old, guard.center());
    }
    #[test]
    fn stable_held_wrong_position_cannot_be_disambiguated() {
        // Both stable neutral and intentionally HELD stick can be indistinguishable.
        // We rely on user-confirmed release and excursion evidence; not a guarantee.
        let mut s = ReacquireSession::new();
        release(&mut s, Vec2::new(0.5, 0.0));
        s.sample_exercise(Vec2::new(0.8, 0.0)).unwrap();
        s.begin_second_release().unwrap();
        release(&mut s, Vec2::new(0.5, 0.0));
        assert!(s.finish().is_ok());
    }
}
