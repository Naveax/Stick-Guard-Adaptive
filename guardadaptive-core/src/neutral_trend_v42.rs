//! v0.42 sequential *operator-declared* neutral-window evidence analysis.
//! Four consecutive 128-frame windows with a shared session epoch are
//! evaluated against a fixed center. No joystick-intent inference, output
//! write, center update, or automatic profile activation is permitted.
use crate::Vec2;
use std::{fs, io, path::Path};
const HEADER: &str =
    "window,sample,elapsed_us,packet,session_epoch,raw_x,raw_y,operator_release_label,source";
const WINDOW: usize = 128;
const WINDOWS: usize = 4;
const ROWS: usize = WINDOW * WINDOWS;
const MAX_BYTES: u64 = 120_000;
const SHIFT: f64 = 0.012;
const COHERENCE: f64 = 0.006;
const MAX_MAD: f64 = 0.002;
const MAX_SLOPE: f64 = 0.003;
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finding {
    BaselineInvalid,
    ConsistentDeclaredNeutral,
    PersistentDeclaredCenterDisagreement,
    OscillatingOrTransientDisagreement,
    UnstableDeclaredNeutral,
}
impl Finding {
    fn name(self) -> &'static str {
        match self {
            Self::BaselineInvalid => "baseline_unstable_manual_calibration_required",
            Self::ConsistentDeclaredNeutral => "declared_neutral_consistent_center_unchanged",
            Self::PersistentDeclaredCenterDisagreement => {
                "persistent_declared_neutral_disagreement_recalibration_suggested"
            }
            Self::OscillatingOrTransientDisagreement => {
                "nonstationary_or_ambiguous_neutral_retest_required"
            }
            Self::UnstableDeclaredNeutral => "declared_neutral_unstable_retest_required",
        }
    }
    fn must_revoke_preview(self) -> bool {
        self != Self::ConsistentDeclaredNeutral
    }
}
#[derive(Clone, Copy)]
struct Window {
    axes: [Vec2; WINDOW],
    count: usize,
}
impl Default for Window {
    fn default() -> Self {
        Self {
            axes: [Vec2::ZERO; WINDOW],
            count: 0,
        }
    }
}
#[derive(Clone, Copy)]
struct Statistic {
    median: Vec2,
    mad: Vec2,
    halves: f64,
    stable: bool,
}
fn robust(values: &[Vec2; WINDOW]) -> Statistic {
    let mut xs = std::array::from_fn::<_, WINDOW, _>(|i| values[i].x);
    let mut ys = std::array::from_fn::<_, WINDOW, _>(|i| values[i].y);
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let median = Vec2::new((xs[63] + xs[64]) * 0.5, (ys[63] + ys[64]) * 0.5);
    let mut dx = std::array::from_fn::<_, WINDOW, _>(|i| (values[i].x - median.x).abs());
    let mut dy = std::array::from_fn::<_, WINDOW, _>(|i| (values[i].y - median.y).abs());
    dx.sort_by(f64::total_cmp);
    dy.sort_by(f64::total_cmp);
    let mad = Vec2::new((dx[63] + dx[64]) * 0.7413, (dy[63] + dy[64]) * 0.7413);
    let half1 = values[0..64].iter().fold(Vec2::ZERO, |a, b| a + *b) * (1.0 / 64.0);
    let half2 = values[64..128].iter().fold(Vec2::ZERO, |a, b| a + *b) * (1.0 / 64.0);
    let halves = (half2 - half1).norm();
    Statistic {
        median,
        mad,
        halves,
        stable: mad.x <= MAX_MAD && mad.y <= MAX_MAD && halves <= MAX_SLOPE,
    }
}
#[derive(Clone, Copy)]
pub struct Summary {
    finding: Finding,
    stats: [Statistic; WINDOWS],
    epoch: u64,
    first_packet: u32,
    last_packet: u32,
    origin: &'static str,
}
impl Summary {
    pub fn finding(&self) -> Finding {
        self.finding
    }
    pub fn json(&self) -> String {
        let windows = self
            .stats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    concat!(
                        "{{\"window\":{},\"median_x\":{:.8},\"median_y\":{:.8},",
                        "\"mad_x\":{:.8},\"mad_y\":{:.8},\"half_window_shift\":{:.8},",
                        "\"declared_neutral_stable\":{}}}"
                    ),
                    i, s.median.x, s.median.y, s.mad.x, s.mad.y, s.halves, s.stable
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v42_operator_declared_temporal_neutral_audit\",",
                "\"samples\":512,\"windows\":4,\"session_epoch\":{},",
                "\"first_packet\":{},\"last_packet\":{},\"observation_origin\":\"{}\",",
                "\"finding\":\"{}\",\"revoke_preview\":{},",
                "\"window_statistics\":[{}],",
                "\"no_intent_inference_from_motion\":true,",
                "\"physical_release_verified\":false,",
                "\"physical_drift_confirmed\":false,",
                "\"automatic_center_update\":false,\"candidate_promoted\":false,",
                "\"driver_game_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.epoch,
            self.first_packet,
            self.last_packet,
            self.origin,
            self.finding.name(),
            self.finding.must_revoke_preview(),
            windows
        )
    }
}
fn parse(content: &str) -> io::Result<Summary> {
    let mut lines = content.lines();
    if lines.next() != Some(HEADER) {
        return Err(invalid("window schema mismatch"));
    }
    let mut chunks = [Window::default(); WINDOWS];
    let mut previous_time = None;
    let mut previous_packet = None;
    let mut epoch = None;
    let mut first_packet = None;
    let mut origin: Option<&'static str> = None;
    let mut total = 0usize;
    for line in lines {
        if total >= ROWS {
            return Err(invalid("too many neutral samples"));
        }
        let v = line.split(',').collect::<Vec<_>>();
        if v.len() != 9
            || v[0].parse::<usize>().ok() != Some(total / WINDOW)
            || v[1].parse::<usize>().ok() != Some(total % WINDOW)
        {
            return Err(invalid("unexpected window index or column count"));
        }
        let t = v[2].parse::<u64>().map_err(|_| invalid("bad clock"))?;
        let packet = v[3]
            .parse::<u32>()
            .map_err(|_| invalid("bad packet counter"))?;
        let this_epoch = v[4]
            .parse::<u64>()
            .map_err(|_| invalid("bad session epoch"))?;
        if epoch.is_some_and(|x| x != this_epoch) {
            return Err(invalid("device session changed"));
        }
        epoch = Some(this_epoch);
        let x = v[5]
            .parse::<f64>()
            .map_err(|_| invalid("invalid horizontal axis"))?;
        let y = v[6]
            .parse::<f64>()
            .map_err(|_| invalid("invalid vertical axis"))?;
        if !x.is_finite() || !y.is_finite() || x.abs() > 1.0 || y.abs() > 1.0 {
            return Err(invalid("out of range, nonfinite axis"));
        }
        if v[7] != "operator_declared_released" {
            return Err(invalid(
                "all neutral windows need explicit operator release label",
            ));
        }
        let source = match v[8] {
            "synthetic_fixture" => "synthetic_fixture",
            "xinput_readonly" => "xinput_readonly_device_identity_unverified",
            _ => return Err(invalid("unrecognized observation provenance")),
        };
        if origin.is_some_and(|prior| prior != source) {
            return Err(invalid("mixed observed and synthetic provenance"));
        }
        origin = Some(source);
        if let Some(previous) = previous_time {
            if t <= previous {
                return Err(invalid("clock nonmonotonic"));
            }
            if !total.is_multiple_of(WINDOW) && t - previous > 50_000 {
                return Err(invalid("sampling gap inside neutral window"));
            }
        }
        if previous_packet.is_some_and(|p| packet < p) {
            return Err(invalid("XInput packet counter rollback"));
        }
        first_packet.get_or_insert(packet);
        previous_time = Some(t);
        previous_packet = Some(packet);
        chunks[total / WINDOW].axes[total % WINDOW] = Vec2::new(x, y);
        chunks[total / WINDOW].count += 1;
        total += 1;
    }
    if total != ROWS || chunks.iter().any(|w| w.count != WINDOW) {
        return Err(invalid("missing operator-labelled neutral samples"));
    }
    let stats = chunks.map(|c| robust(&c.axes));
    let baseline = stats[0];
    let finding = if !baseline.stable {
        Finding::BaselineInvalid
    } else if stats[1..].iter().any(|s| !s.stable) {
        Finding::UnstableDeclaredNeutral
    } else {
        let shifts =
            std::array::from_fn::<_, 3, _>(|i| (stats[i + 1].median - baseline.median).norm());
        if shifts.iter().all(|d| *d <= SHIFT) {
            Finding::ConsistentDeclaredNeutral
        } else {
            let pairwise = [
                (stats[1].median - stats[2].median).norm(),
                (stats[2].median - stats[3].median).norm(),
                (stats[1].median - stats[3].median).norm(),
            ];
            if shifts.iter().all(|d| *d > SHIFT) && pairwise.iter().all(|d| *d <= COHERENCE) {
                Finding::PersistentDeclaredCenterDisagreement
            } else {
                Finding::OscillatingOrTransientDisagreement
            }
        }
    };
    Ok(Summary {
        finding,
        stats,
        epoch: epoch.unwrap_or(0),
        first_packet: first_packet.unwrap_or(0),
        last_packet: previous_packet.unwrap_or(0),
        origin: origin.unwrap_or("unknown"),
    })
}
pub fn analyze(path: &Path, out: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES {
        return Err(invalid("input must be a bounded ordinary CSV"));
    }
    let content = fs::read_to_string(path)?;
    let result = parse(&content)?;
    crate::truth_lab::save_new(out, &result.json())?;
    println!("V042 operator-declared temporal neutral: {} | no center auto-update | physical evidence BLOCKED",
        result.finding.name());
    Ok(())
}
pub const SCENARIOS: [&str; 7] = [
    "four_stable_windows",
    "persistent_step_drift",
    "transient_step_then_recover",
    "alternating_center_drift",
    "noisy_oscillatory_drift",
    "held_intention_falsely_labeled_released",
    "slow_center_ramp",
];
fn fixture(case: usize, seed: u64) -> String {
    use crate::resolution::Resolution;
    let mut rng = seed ^ 0xD1B54A32D192ED03;
    let mut text = String::from(HEADER);
    text.push('\n');
    for w in 0..WINDOWS {
        for i in 0..WINDOW {
            rng ^= rng >> 12;
            rng ^= rng << 25;
            rng ^= rng >> 27;
            let jitter = if rng & 1 == 0 { 0.00004 } else { -0.00004 };
            let baseline = Vec2::new(0.22, -0.13);
            let value = match case {
                0 => baseline,
                1 => {
                    baseline
                        + if w == 0 {
                            Vec2::ZERO
                        } else {
                            Vec2::new(0.07, -0.03)
                        }
                }
                2 => {
                    baseline
                        + if w == 1 {
                            Vec2::new(0.07, -0.03)
                        } else {
                            Vec2::ZERO
                        }
                }
                3 => {
                    baseline
                        + if w == 0 {
                            Vec2::ZERO
                        } else if w % 2 == 0 {
                            Vec2::new(-0.06, 0.02)
                        } else {
                            Vec2::new(0.07, -0.03)
                        }
                }
                4 => {
                    baseline
                        + if w == 0 {
                            Vec2::ZERO
                        } else {
                            Vec2::new(0.09 * ((i as f64) * 0.18).sin(), 0.0)
                        }
                }
                5 => {
                    baseline
                        + if w == 0 {
                            Vec2::ZERO
                        } else {
                            Vec2::new(0.27, 0.0)
                        }
                }
                _ => {
                    baseline
                        + if w == 0 {
                            Vec2::ZERO
                        } else {
                            Vec2::new(0.024 * w as f64 + 0.00017 * i as f64, 0.0)
                        }
                }
            };
            let raw = Resolution::XInput16.quantize((value + Vec2::new(jitter, -jitter)).clamp());
            let t = (w * WINDOW + i + 1) as u64 * 8_000;
            text.push_str(&format!(
                "{w},{i},{t},{},1,{:.8},{:.8},operator_declared_released,synthetic_fixture\n",
                (w * WINDOW + i) / 2,
                raw.x,
                raw.y
            ));
        }
    }
    text
}
pub fn suite(seed: u64, out: &Path) -> io::Result<()> {
    let findings = SCENARIOS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let s = parse(&fixture(i, seed))?;
            Ok::<_, io::Error>(format!(
                "\"{label}\":{{\"finding\":\"{}\",\"preview_revoked\":{}}}",
                s.finding.name(),
                s.finding.must_revoke_preview()
            ))
        })
        .collect::<io::Result<Vec<_>>>()?
        .join(",");
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v42_synthetic_neutral_uncertainty_suite\",",
            "\"seed\":{},\"scenarios\":{{{}}},",
            "\"independent_motion_intent_available\":false,",
            "\"false_operator_release_indistinguishable_from_drift\":true,",
            "\"automatic_center_change\":false,\"driver_game_writes\":0,",
            "\"physical_gate\":\"BLOCKED\"}}\n"
        ),
        seed, findings
    );
    crate::truth_lab::save_new(out, &report)?;
    println!("V042 synthetic 7 temporal neutral cases PASS, operator declaration is NOT physical proof, game writes=0, production BLOCKED");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seven_cases_are_correctly_distinguished_as_claims_not_truth() {
        let expected = [
            Finding::ConsistentDeclaredNeutral,
            Finding::PersistentDeclaredCenterDisagreement,
            Finding::OscillatingOrTransientDisagreement,
            Finding::OscillatingOrTransientDisagreement,
            Finding::UnstableDeclaredNeutral,
            Finding::PersistentDeclaredCenterDisagreement,
            Finding::UnstableDeclaredNeutral,
        ];
        for (i, answer) in expected.iter().enumerate() {
            assert_eq!(
                parse(&fixture(i, 20261013)).unwrap().finding(),
                *answer,
                "{}",
                SCENARIOS[i]
            );
        }
    }
    #[test]
    fn physically_held_stick_is_falsely_indistinguishable_from_declared_drift() {
        assert_eq!(
            parse(&fixture(1, 20261013)).unwrap().finding(),
            parse(&fixture(5, 20261013)).unwrap().finding()
        );
        let false_claim = parse(&fixture(5, 20261013)).unwrap().json();
        assert!(false_claim.contains("\"physical_drift_confirmed\":false"));
        assert!(false_claim.contains("\"automatic_center_update\":false"));
    }
    #[test]
    fn samples_must_be_ordered_and_session_consistent() {
        let original = fixture(0, 11);
        for altered in [
            original.replacen("0,0,8000,0,1,", "0,0,8000,0,2,", 1),
            original.replacen("1,0,1032000,64,1,", "1,0,1032000,64,2,", 1),
            original.replacen("0,1,16000,0,1,", "0,1,8000,0,1,", 1),
            original.replacen("0,3,32000,1,1,", "0,3,32000,0,1,", 1),
            original.replace("operator_declared_released", "not_confirmed"),
            original.replace("synthetic_fixture", "claimed_physically_verified"),
            original.replacen("0,2,24000,1,1,", "0,2,104000,1,1,", 1),
        ] {
            assert!(parse(&altered).is_err());
        }
        assert!(parse(
            original
                .lines()
                .take(300)
                .collect::<Vec<_>>()
                .join("\n")
                .as_str()
        )
        .is_err());
    }
    #[test]
    fn supports_identical_packet_counters_and_refuses_invalid_axes() {
        let original = fixture(0, 13);
        assert_eq!(
            parse(&original).unwrap().finding(),
            Finding::ConsistentDeclaredNeutral
        );
        assert!(parse(&original).is_ok());
        let bad = original.replacen("0,0,8000,0,1,", "0,0,8000,0,1,NaN,", 1);
        assert!(parse(&bad).is_err());
    }
    #[test]
    fn bounded_no_overwrite_and_all_outcomes_revoke_unless_consistent() {
        assert!(!Finding::ConsistentDeclaredNeutral.must_revoke_preview());
        for candidate in [
            Finding::BaselineInvalid,
            Finding::PersistentDeclaredCenterDisagreement,
            Finding::OscillatingOrTransientDisagreement,
            Finding::UnstableDeclaredNeutral,
        ] {
            assert!(candidate.must_revoke_preview());
        }
        let out = std::env::temp_dir().join(format!("ga-v42-suite-{}.json", std::process::id()));
        let _ = fs::remove_file(&out);
        suite(20261013, &out).unwrap();
        let report = fs::read_to_string(&out).unwrap();
        assert!(report.contains("\"false_operator_release_indistinguishable_from_drift\":true"));
        assert!(suite(20261013, &out).is_err());
        let _ = fs::remove_file(out);
    }
}
