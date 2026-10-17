//! v0.85 offline candidate: reject contaminated neutral windows.
//! Observation is NOT proof of physical release or player intent.
//! This module is not wired into the active gamepad, web or driver path.
use crate::Vec2;

const MIN_SAMPLES: usize = 128;
const MAX_SAMPLES: usize = 4096;
const MAX_WINDOW_OUTLIER: f64 = 0.012;
const MAX_NOISE_RMS: f64 = 0.003;
const MAX_CENTER_DISAGREEMENT: f64 = 0.006;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    MissingIndependentReleaseConfirmation,
    InsufficientOrOversizeWindow,
    InvalidAxis,
    ContaminatedWindow,
    CentersDisagree,
}

#[derive(Clone, Copy, Debug)]
pub struct StableCenterCandidate {
    center: Vec2,
    /// Within-window measured residual, NOT a bound on true physical error.
    pub observed_noise_rms: f64,
    pub between_window_disagreement: f64,
}

impl StableCenterCandidate {
    pub fn center(&self) -> Vec2 {
        self.center
    }

    /// No deadzone or smoothing. Does NOT change a physical device or game.
    pub fn preview(&self, raw: Vec2) -> Option<Vec2> {
        if raw.finite() && raw.x.abs() <= 1.0 && raw.y.abs() <= 1.0 {
            Some((raw - self.center).saturate())
        } else {
            None
        }
    }
}

fn center_of(data: &[Vec2]) -> Result<(Vec2, f64), Rejection> {
    if !(MIN_SAMPLES..=MAX_SAMPLES).contains(&data.len()) {
        return Err(Rejection::InsufficientOrOversizeWindow);
    }
    if data
        .iter()
        .any(|v| !v.finite() || v.x.abs() > 1.0 || v.y.abs() > 1.0)
    {
        return Err(Rejection::InvalidAxis);
    }
    let mut xs: Vec<f64> = data.iter().map(|v| v.x).collect();
    let mut ys: Vec<f64> = data.iter().map(|v| v.y).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let median = |v: &[f64]| {
        let mid = v.len() / 2;
        if v.len().is_multiple_of(2) {
            (v[mid - 1] + v[mid]) * 0.5
        } else {
            v[mid]
        }
    };
    let c = Vec2::new(median(&xs), median(&ys));
    let mut sse = 0.0;
    for point in data {
        let error = (*point - c).norm();
        if error > MAX_WINDOW_OUTLIER {
            return Err(Rejection::ContaminatedWindow);
        }
        sse += error * error;
    }
    let rms = (sse / data.len() as f64).sqrt();
    if rms > MAX_NOISE_RMS {
        return Err(Rejection::ContaminatedWindow);
    }
    Ok((c, rms))
}

/// Both release acknowledgements must be external, separate and trustworthy.
/// Boolean acknowledgements are NEVER independently verified by this function.
pub fn review_two_neutral_windows(
    first: &[Vec2],
    second: &[Vec2],
    first_release_confirmed: bool,
    second_release_confirmed: bool,
) -> Result<StableCenterCandidate, Rejection> {
    if !first_release_confirmed || !second_release_confirmed {
        return Err(Rejection::MissingIndependentReleaseConfirmation);
    }
    let (first_center, a_noise) = center_of(first)?;
    let (second_center, b_noise) = center_of(second)?;
    let disagreement = (first_center - second_center).norm();
    if disagreement > MAX_CENTER_DISAGREEMENT {
        return Err(Rejection::CentersDisagree);
    }
    Ok(StableCenterCandidate {
        center: (first_center + second_center) * 0.5,
        observed_noise_rms: a_noise.max(b_noise),
        between_window_disagreement: disagreement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stable(center: Vec2) -> Vec<Vec2> {
        (0..128)
            .map(|i| {
                let noise = if i % 2 == 0 { 0.0002 } else { -0.0002 };
                center + Vec2::new(noise, -noise)
            })
            .collect()
    }

    #[test]
    fn signed_grid_with_distinct_confirmed_neutral_windows_preserves_micro() {
        let mut passed = 0;
        for x in -10..=10 {
            for y in -10..=10 {
                let offset = Vec2::new(x as f64 * 0.01, y as f64 * 0.01);
                let v = review_two_neutral_windows(&stable(offset), &stable(offset), true, true)
                    .unwrap();
                assert!(v.preview(offset).unwrap().norm() < 0.009);
                let intended = Vec2::new(0.003, -0.002);
                assert!((v.preview(offset + intended).unwrap() - intended).norm() < 0.0001);
                passed += 1;
            }
        }
        assert_eq!(passed, 441);
    }

    #[test]
    fn contaminated_neutral_window_is_rejected_instead_of_averaged() {
        let mut faulty = stable(Vec2::new(0.03, 0.02));
        faulty[49] = Vec2::new(0.9, 0.9);
        assert_eq!(
            review_two_neutral_windows(&faulty, &stable(Vec2::new(0.03, 0.02)), true, true)
                .unwrap_err(),
            Rejection::ContaminatedWindow
        );
    }

    #[test]
    fn unconfirmed_and_changed_neutral_are_rejected() {
        let clean = stable(Vec2::ZERO);
        assert_eq!(
            review_two_neutral_windows(&clean, &clean, false, true).unwrap_err(),
            Rejection::MissingIndependentReleaseConfirmation
        );
        assert_eq!(
            review_two_neutral_windows(&clean, &stable(Vec2::new(0.02, 0.0)), true, true)
                .unwrap_err(),
            Rejection::CentersDisagree
        );
    }

    #[test]
    fn false_release_can_fool_stable_window_even_when_both_agree() {
        let mislabeled = stable(Vec2::new(0.01, 0.0));
        let candidate = review_two_neutral_windows(&mislabeled, &mislabeled, true, true).unwrap();
        // The *real* neutral could have been (0,0), with 1% held intention.
        // That ambiguity cannot be resolved from sensor stability.
        assert!(candidate.preview(Vec2::ZERO).unwrap().norm() > 0.009);
    }
}
