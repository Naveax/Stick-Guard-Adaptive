//! UI-independent, consent-gated calibration journey.
//! A future GUI/console captures acknowledgement and reports each raw sample.
//! This API never declares the stick "released" solely from a static signal.
use crate::{
    reacquire::{ReacquireError, ReacquireSession, Stage, VerifiedCenter},
    Vec2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    AskFirstRelease,
    SampleFirst,
    AskMovement,
    AskSecondRelease,
    SampleSecond,
    Review,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardError {
    WrongScreen,
    Reacquire(ReacquireError),
}
impl From<ReacquireError> for WizardError {
    fn from(e: ReacquireError) -> Self {
        Self::Reacquire(e)
    }
}
pub struct GuidedWizard {
    screen: Screen,
    inner: ReacquireSession,
}
impl Default for GuidedWizard {
    fn default() -> Self {
        Self::new()
    }
}
impl GuidedWizard {
    pub fn new() -> Self {
        Self {
            screen: Screen::AskFirstRelease,
            inner: ReacquireSession::new(),
        }
    }
    pub fn screen(&self) -> Screen {
        self.screen
    }
    /// Text is suitable for future UI prompts; it is not an assertion about
    /// what the human has actually done.
    pub fn instruction(&self) -> &'static str {
        match self.screen {
            Screen::AskFirstRelease => "Analogu serbest birak. Hazir oldugunda ilk olcumu baslat.",
            Screen::SampleFirst => "Analoga dokunma. Ilk notr penceresi olculuyor.",
            Screen::AskMovement => "Analogu saga/sola veya yukari/asagi belirgin hareket ettir.",
            Screen::AskSecondRelease => {
                "Analogu tekrar tamamen serbest birak; ikinci olcumu onayla."
            }
            Screen::SampleSecond => "Analoga dokunma. Ikinci notr penceresi olculuyor.",
            Screen::Review => "Iki olcum tamamlandi. Merkez ancak SEN onaylarsan kaydedilecek.",
            Screen::Cancelled => "Kalibrasyon iptal edildi. Mevcut profil degismedi.",
        }
    }
    pub fn samples_remaining(&self) -> usize {
        match self.screen {
            Screen::SampleFirst | Screen::SampleSecond => self.inner.samples_needed(),
            _ => 0,
        }
    }
    pub fn confirm_release(&mut self) -> Result<(), WizardError> {
        match self.screen {
            Screen::AskFirstRelease => self.screen = Screen::SampleFirst,
            Screen::AskSecondRelease => {
                self.screen = Screen::SampleSecond;
            }
            _ => return Err(WizardError::WrongScreen),
        }
        Ok(())
    }
    /// Only call with the actual stick axis report; no labels or simulator
    /// truth enter the correction engine.
    pub fn sample(&mut self, raw: Vec2) -> Result<(), WizardError> {
        match self.screen {
            Screen::SampleFirst | Screen::SampleSecond => {
                let was_first = self.screen == Screen::SampleFirst;
                if let Err(e) = self.inner.sample_released(raw) {
                    self.screen = Screen::Cancelled;
                    return Err(e.into());
                }
                if self.inner.stage() == Stage::Movement && was_first {
                    self.screen = Screen::AskMovement;
                } else if self.inner.stage() == Stage::Ready {
                    self.screen = Screen::Review;
                }
                Ok(())
            }
            Screen::AskMovement => {
                if let Err(e) = self.inner.sample_exercise(raw) {
                    self.screen = Screen::Cancelled;
                    return Err(e.into());
                }
                Ok(())
            }
            _ => Err(WizardError::WrongScreen),
        }
    }
    pub fn confirm_movement_finished(&mut self) -> Result<(), WizardError> {
        if self.screen != Screen::AskMovement {
            return Err(WizardError::WrongScreen);
        }
        // The phase advances only if the previous raw stream exhibited
        // sufficient excursion relative to first confirmed-neutral center.
        // User release confirmation is still a separate action.
        // Probe through inner using begin_second_release here, not later,
        // so the second release prompt is only offered once eligible.
        self.inner.begin_second_release()?;
        self.screen = Screen::AskSecondRelease;
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.screen = Screen::Cancelled;
    }
    pub fn commit(self) -> Result<VerifiedCenter, WizardError> {
        if self.screen != Screen::Review {
            return Err(WizardError::WrongScreen);
        }
        Ok(self.inner.finish()?)
    }
}
/// Offline end-to-end demo of the UI protocol, profile persistence and
/// restoration. All raw reports are synthetic. No hardware is accessed.
pub fn demo(out: &std::path::Path) -> std::io::Result<()> {
    use crate::{
        motion_first::{MotionFirst, MotionFirstConfig},
        profile_store::{self, Identity, Stick},
        resolution::Resolution,
        InputMode,
    };
    let identity = Identity::new("SYNTHETIC-DEMO-001", Stick::Left, Resolution::XInput16)?;
    let mut wizard = GuidedWizard::new();
    println!(
        "GUIDED DEMO: {} | no real controller connected",
        wizard.instruction()
    );
    wizard
        .confirm_release()
        .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    let bias = Vec2::new(0.55, -0.2);
    for i in 0..128 {
        let jitter = if i % 2 == 0 { 0.00003 } else { -0.00003 };
        let raw = Resolution::XInput16.quantize(bias + Vec2::new(jitter, -jitter));
        wizard
            .sample(raw)
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    }
    println!("STEP: {}", wizard.instruction());
    for i in 0..128 {
        let raw = Resolution::XInput16.quantize(bias + Vec2::new(-(i as f64 / 127.0) * 0.3, 0.0));
        wizard
            .sample(raw)
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    }
    wizard
        .confirm_movement_finished()
        .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    println!("STEP: {}", wizard.instruction());
    wizard
        .confirm_release()
        .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    for i in 0..128 {
        let jitter = if i % 2 == 0 { 0.00003 } else { -0.00003 };
        let raw = Resolution::XInput16.quantize(bias + Vec2::new(jitter, -jitter));
        wizard
            .sample(raw)
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    }
    println!("STEP: {}", wizard.instruction());
    let profile = wizard
        .commit()
        .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    profile_store::save(out, &identity, &profile)?;
    let loaded = profile_store::load(out, &identity)?;
    let mut corrected = MotionFirst::new(MotionFirstConfig {
        input_lsb: Resolution::XInput16.positive_lsb(),
        softness_lsb: 0.5,
    });
    // No other profile is auto-loaded. The matching device/stick must be
    // selected by the future controller acquisition layer.
    corrected.accept_reacquisition(loaded);
    let out_neutral = corrected.process(Resolution::XInput16.quantize(bias), InputMode::Live);
    let out_micro = corrected.process(
        Resolution::XInput16.quantize(bias + Vec2::new(0.002, 0.)),
        InputMode::Live,
    );
    if out_neutral.norm() > 0.0001 || out_micro.x < 0.0019 {
        return Err(std::io::Error::other(
            "synthetic guided-profile roundtrip failed behavioral acceptance",
        ));
    }
    println!(
        "GUIDED DEMO PASS: profile={} neutral_output=({:.8},{:.8}) micro_output=({:.8},{:.8})",
        out.display(),
        out_neutral.x,
        out_neutral.y,
        out_micro.x,
        out_micro.y
    );
    println!(
        "IMPORTANT: identity is synthetic, and stored checksum detects accidental corruption only."
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fill(w: &mut GuidedWizard, c: Vec2) {
        for _ in 0..128 {
            w.sample(c).unwrap();
        }
    }
    #[test]
    fn cannot_collect_or_commit_before_consent() {
        let mut w = GuidedWizard::new();
        assert_eq!(w.sample(Vec2::ZERO), Err(WizardError::WrongScreen));
        assert!(matches!(w.commit(), Err(WizardError::WrongScreen)));
    }
    #[test]
    fn second_release_requires_observed_movement_and_acknowledgement() {
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        fill(&mut w, Vec2::new(0.55, -0.2));
        assert_eq!(w.screen(), Screen::AskMovement);
        assert_eq!(
            w.confirm_movement_finished(),
            Err(WizardError::Reacquire(ReacquireError::NoExcursion))
        );
        w.sample(Vec2::new(0.3, -0.2)).unwrap();
        w.confirm_movement_finished().unwrap();
        assert_eq!(w.screen(), Screen::AskSecondRelease);
        assert_eq!(
            w.sample(Vec2::new(0.55, -0.2)),
            Err(WizardError::WrongScreen)
        );
        w.confirm_release().unwrap();
        fill(&mut w, Vec2::new(0.55, -0.2));
        assert_eq!(w.screen(), Screen::Review);
        assert!((w.commit().unwrap().center() - Vec2::new(0.55, -0.2)).norm() < 1e-10);
    }
    #[test]
    fn cancel_prevents_approval() {
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        w.cancel();
        assert_eq!(w.screen(), Screen::Cancelled);
        assert_eq!(w.sample(Vec2::ZERO), Err(WizardError::WrongScreen));
        assert!(w.commit().is_err());
    }
    #[test]
    fn invalid_neutral_sample_aborts_without_profile() {
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        assert_eq!(
            w.sample(Vec2::new(f64::NAN, 0.0)),
            Err(WizardError::Reacquire(ReacquireError::InvalidSample))
        );
        assert_eq!(w.screen(), Screen::Cancelled);
    }
}
