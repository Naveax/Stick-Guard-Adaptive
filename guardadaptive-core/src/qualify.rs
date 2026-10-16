//! Conservative release gate for red-team CSV. Never conflates neutral score
//! with player-intended movement fidelity. Gate failures block deployment.
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};
#[derive(Default)]
pub struct Summary {
    pub cases: u64,
    pub neutral_worse: u64,
    pub movement_worse: u64,
    pub combined_worse: u64,
    pub micro_lost_total: u64,
    pub reversed_total: u64,
    pub high_error_total: u64,
    pub worst_combined: f64,
    pub precision_better_motion: u64,
    pub precision_worse_neutral: u64,
    pub precision_saved_micro: u64,
    pub precision_micro_lost_total: u64,
    pub precision_reversed_total: u64,
    pub precision_cases: u64,
    pub report_resolution: Option<String>,
    pub below_report_resolution_total: u64,
    pub quantization_changed_total: u64,
    pub resolution_cases: u64,
    pub guard_micro_lost_above_resolution: u64,
    pub guard_micro_lost_below_resolution: u64,
    pub precision_micro_lost_above_resolution: u64,
    pub precision_micro_lost_below_resolution: u64,
    pub partition_cases: u64,
    pub motion_first_cases: u64,
    pub motion_first_better_motion: u64,
    pub motion_first_worse_neutral: u64,
    pub motion_first_worse_than_fixed_motion: u64,
    pub motion_first_worse_than_fixed_neutral: u64,
    pub motion_first_worse_than_fixed_joint: u64,
    pub motion_first_micro_lost: u64,
    pub motion_first_wrong_direction: u64,
    pub motion_first_loss_reportable: u64,
    pub motion_first_loss_sub_lsb: u64,
    pub noise_guard_cases: u64,
    pub noise_better_neutral_than_motionfirst: u64,
    pub noise_worse_motion_than_motionfirst: u64,
    pub noise_worse_neutral_than_fixed: u64,
    pub noise_worse_motion_than_fixed: u64,
    pub noise_micro_lost_total: u64,
    pub noise_wrong_direction_total: u64,
    pub noise_lost_above_resolution: u64,
    pub noise_lost_below_resolution: u64,
    pub hybrid_cases: u64,
    pub hybrid_better_neutral_vs_motionfirst: u64,
    pub hybrid_better_motion_vs_motionfirst: u64,
    pub hybrid_more_micro_loss_cases: u64,
    pub hybrid_micro_lost: u64,
    pub hybrid_wrong_direction: u64,
    pub hybrid_resolvable_micro_loss: u64,
    pub hybrid_unresolvable_micro_loss: u64,
    pub direction_cases: u64,
    pub direction_micro_lost: u64,
    pub direction_wrong_direction: u64,
    pub direction_resolvable_micro_loss: u64,
    pub direction_unresolvable_micro_loss: u64,
    pub direction_more_micro_than_hybrid_cases: u64,
    pub direction_fewer_wrong_than_hybrid_cases: u64,
    pub direction_more_wrong_than_hybrid_cases: u64,
    pub sign_opposite_cases: u64,
    pub hybrid_sign_opposite: u64,
    pub direction_sign_opposite: u64,
    pub confidence_cases: u64,
    pub confidence_micro_lost: u64,
    pub confidence_wrong_direction: u64,
    pub confidence_sign_opposite: u64,
    pub confidence_micro_more_than_direction_cases: u64,
    pub confidence_sign_more_than_direction_cases: u64,
    pub confidence_micro_resolvable: u64,
    pub confidence_micro_unresolvable: u64,
}
impl Summary {
    pub fn passed(&self) -> bool {
        self.cases > 0
            && self.neutral_worse == 0
            && self.movement_worse == 0
            && self.combined_worse == 0
            && self.micro_lost_total == 0
            && self.reversed_total == 0
            && self.high_error_total == 0
    }
}
const SWEEP_HEADER:&str="case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score,precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction,resolution,report_lsb,below_report_resolution,quantization_changed,guard_micro_lost_above_resolution,guard_micro_lost_below_resolution,precision_micro_lost_above_resolution,precision_micro_lost_below_resolution,motionfirst_neutral_rmse,motionfirst_motion_rmse,motionfirst_micro_lost,motionfirst_wrong_direction,motionfirst_micro_lost_above_resolution,motionfirst_micro_lost_below_resolution,noiseguard_neutral_rmse,noiseguard_motion_rmse,noiseguard_micro_lost,noiseguard_wrong_direction,noiseguard_micro_lost_above_resolution,noiseguard_micro_lost_below_resolution,hybrid_neutral_rmse,hybrid_motion_rmse,hybrid_micro_lost,hybrid_wrong_direction,hybrid_micro_lost_above_resolution,hybrid_micro_lost_below_resolution,direction_neutral_rmse,direction_motion_rmse,direction_micro_lost,direction_wrong_direction,direction_micro_lost_above_resolution,direction_micro_lost_below_resolution,hybrid_sign_opposite,direction_sign_opposite,confidence_neutral_rmse,confidence_motion_rmse,confidence_micro_lost,confidence_wrong_direction,confidence_micro_lost_above_resolution,confidence_micro_lost_below_resolution,confidence_sign_opposite";

fn count_metric(columns: &[&str], index: usize) -> io::Result<u64> {
    columns
        .get(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing counter"))?
        .parse::<u64>()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid nonnegative integer counter",
            )
        })
}
fn metric(columns: &[&str], index: usize) -> io::Result<f64> {
    let s = columns
        .get(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing CSV column"))?;
    let v = s
        .parse::<f64>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid CSV number"))?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "non-finite CSV number",
        ))
    }
}
pub fn analyze(input: &Path) -> io::Result<Summary> {
    let file = File::open(input)?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty report"))?;
    let headers: Vec<_> = header.split(',').collect();
    const WIDTHS: [usize; 10] = [21, 25, 29, 33, 39, 45, 51, 57, 59, 66];
    let official: Vec<_> = SWEEP_HEADER.split(',').collect();
    if !WIDTHS.contains(&headers.len()) || headers != official[..headers.len()] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a GuardAdaptive sweep report",
        ));
    }
    let mut s = Summary::default();
    for line in lines {
        let l = line?;
        if l.trim().is_empty() {
            continue;
        }
        let f: Vec<_> = l.split(',').collect();
        if f.len() != headers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "row width disagrees with CSV schema",
            ));
        }
        let gn = metric(&f, 8)?;
        let gm = metric(&f, 9)?;
        let fn_ = metric(&f, 12)?;
        let fm = metric(&f, 13)?;
        let micro = count_metric(&f, 15)?;
        let wrong = count_metric(&f, 16)?;
        let high = count_metric(&f, 18)?;
        let score = metric(&f, 20)?;
        s.cases += 1;
        if gn > fn_ + 0.000001 {
            s.neutral_worse += 1;
        }
        if gm > fm + 0.000001 {
            s.movement_worse += 1;
        }
        if gn + 2.0 * gm > fn_ + 2.0 * fm + 0.000001 {
            s.combined_worse += 1;
        }
        s.micro_lost_total += micro;
        s.reversed_total += wrong;
        s.high_error_total += high;
        s.worst_combined = s.worst_combined.max(score);
        // Backward compatible with v0.2 CSV (21 columns). Extended v0.3 CSV
        // has precision candidate metrics appended without changing v0.2 columns.
        if f.len() >= 25 {
            let pn = metric(&f, 21)?;
            let pm = metric(&f, 22)?;
            let pl = count_metric(&f, 23)?;
            let pr = count_metric(&f, 24)?;
            s.precision_cases += 1;
            if pm + 1e-6 < gm {
                s.precision_better_motion += 1;
            }
            if pn > gn + 1e-6 {
                s.precision_worse_neutral += 1;
            }
            let saved = micro.saturating_sub(pl);
            s.precision_saved_micro += saved;
            s.precision_micro_lost_total += pl;
            s.precision_reversed_total += pr;
        }
        if f.len() >= 29 {
            let resolution = f[25];
            if !["ideal", "xinput16", "signed12", "signed8"].contains(&resolution) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid input resolution model",
                ));
            }
            if let Some(prior) = &s.report_resolution {
                if prior != resolution {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "mixed report resolution models",
                    ));
                }
            } else {
                s.report_resolution = Some(resolution.to_owned());
            }
            let lsb = metric(&f, 26)?;
            let below = count_metric(&f, 27)?;
            let changed = count_metric(&f, 28)?;
            if !(0.0..=1.0).contains(&lsb) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid resolution metrics",
                ));
            }
            s.below_report_resolution_total += below;
            s.quantization_changed_total += changed;
            s.resolution_cases += 1;
        } else if f.len() > 25 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete extended resolution columns",
            ));
        }
        if f.len() >= 33 {
            let ga = count_metric(&f, 29)?;
            let gb = count_metric(&f, 30)?;
            let pa = count_metric(&f, 31)?;
            let pb = count_metric(&f, 32)?;
            if ga.saturating_add(gb) != micro || pa.saturating_add(pb) != count_metric(&f, 23)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid micro observability partition",
                ));
            }
            s.guard_micro_lost_above_resolution += ga;
            s.guard_micro_lost_below_resolution += gb;
            s.precision_micro_lost_above_resolution += pa;
            s.precision_micro_lost_below_resolution += pb;
            s.partition_cases += 1;
        } else if f.len() > 29 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete micro observability partition",
            ));
        }
        if f.len() >= 39 {
            let mn = metric(&f, 33)?;
            let mm = metric(&f, 34)?;
            let ml = count_metric(&f, 35)?;
            let mr = count_metric(&f, 36)?;
            let ma = count_metric(&f, 37)?;
            let mb = count_metric(&f, 38)?;
            if ma.saturating_add(mb) != ml {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid MotionFirst counters",
                ));
            }
            s.motion_first_cases += 1;
            if mm + 1e-6 < gm {
                s.motion_first_better_motion += 1;
            }
            if mn > gn + 1e-6 {
                s.motion_first_worse_neutral += 1;
            }
            if mm > fm + 1e-6 {
                s.motion_first_worse_than_fixed_motion += 1;
            }
            if mn > fn_ + 1e-6 {
                s.motion_first_worse_than_fixed_neutral += 1;
            }
            if mn + 2.0 * mm > fn_ + 2.0 * fm + 1e-6 {
                s.motion_first_worse_than_fixed_joint += 1;
            }
            s.motion_first_micro_lost += ml;
            s.motion_first_wrong_direction += mr;
            s.motion_first_loss_reportable += ma;
            s.motion_first_loss_sub_lsb += mb;
        } else if f.len() > 33 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete MotionFirst metrics",
            ));
        }
        if f.len() >= 45 {
            let nn = metric(&f, 39)?;
            let nm = metric(&f, 40)?;
            let nl = count_metric(&f, 41)?;
            let nr = count_metric(&f, 42)?;
            let na = count_metric(&f, 43)?;
            let nb = count_metric(&f, 44)?;
            if na.checked_add(nb) != Some(nl) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid NoiseGuard observability counter partition",
                ));
            }
            s.noise_guard_cases += 1;
            if nn + 1e-6 < metric(&f, 33)? {
                s.noise_better_neutral_than_motionfirst += 1;
            }
            if nm > metric(&f, 34)? + 1e-6 {
                s.noise_worse_motion_than_motionfirst += 1;
            }
            if nn > fn_ + 1e-6 {
                s.noise_worse_neutral_than_fixed += 1;
            }
            if nm > fm + 1e-6 {
                s.noise_worse_motion_than_fixed += 1;
            }
            s.noise_micro_lost_total += nl;
            s.noise_wrong_direction_total += nr;
            s.noise_lost_above_resolution += na;
            s.noise_lost_below_resolution += nb;
        }
        if f.len() >= 51 {
            let hn = metric(&f, 45)?;
            let hm = metric(&f, 46)?;
            let hl = count_metric(&f, 47)?;
            let hr = count_metric(&f, 48)?;
            let ha = count_metric(&f, 49)?;
            let hb = count_metric(&f, 50)?;
            if ha.checked_add(hb) != Some(hl) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Hybrid observability partition",
                ));
            }
            s.hybrid_cases += 1;
            if hn + 1e-6 < metric(&f, 33)? {
                s.hybrid_better_neutral_vs_motionfirst += 1;
            }
            if hm + 1e-6 < metric(&f, 34)? {
                s.hybrid_better_motion_vs_motionfirst += 1;
            }
            if hl > count_metric(&f, 35)? {
                s.hybrid_more_micro_loss_cases += 1;
            }
            s.hybrid_micro_lost += hl;
            s.hybrid_wrong_direction += hr;
            s.hybrid_resolvable_micro_loss += ha;
            s.hybrid_unresolvable_micro_loss += hb;
        }
        if f.len() >= 57 {
            let dn = metric(&f, 51)?;
            let dm = metric(&f, 52)?;
            let dl = count_metric(&f, 53)?;
            let dr = count_metric(&f, 54)?;
            let da = count_metric(&f, 55)?;
            let db = count_metric(&f, 56)?;
            if dn < 0.0 || dm < 0.0 || da.checked_add(db) != Some(dl) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid DirectionGuard metrics / resolution partition",
                ));
            }
            s.direction_cases += 1;
            if dl > count_metric(&f, 47)? {
                s.direction_more_micro_than_hybrid_cases += 1;
            }
            let baseline_wrong = count_metric(&f, 48)?;
            if dr < baseline_wrong {
                s.direction_fewer_wrong_than_hybrid_cases += 1;
            }
            if dr > baseline_wrong {
                s.direction_more_wrong_than_hybrid_cases += 1;
            }
            s.direction_micro_lost += dl;
            s.direction_wrong_direction += dr;
            s.direction_resolvable_micro_loss += da;
            s.direction_unresolvable_micro_loss += db;
        }
        if f.len() >= 59 {
            let h = count_metric(&f, 57)?;
            let d = count_metric(&f, 58)?;
            if h < count_metric(&f, 48)? || d < count_metric(&f, 54)? {
                return Err(io::Error::new(io::ErrorKind::InvalidData,
                    "sign-only direction counter cannot be less than thresholded wrong-direction counter"));
            }
            s.sign_opposite_cases += 1;
            s.hybrid_sign_opposite += h;
            s.direction_sign_opposite += d;
        }
        if f.len() == 66 {
            let cn = metric(&f, 59)?;
            let cm = metric(&f, 60)?;
            let cl = count_metric(&f, 61)?;
            let cw = count_metric(&f, 62)?;
            let ca = count_metric(&f, 63)?;
            let cb = count_metric(&f, 64)?;
            let cs = count_metric(&f, 65)?;
            if cn < 0.0 || cm < 0.0 || ca.checked_add(cb) != Some(cl) || cs < cw {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid ConfidenceDirection diagnostic metrics",
                ));
            }
            s.confidence_cases += 1;
            s.confidence_micro_lost += cl;
            s.confidence_wrong_direction += cw;
            s.confidence_sign_opposite += cs;
            s.confidence_micro_resolvable += ca;
            s.confidence_micro_unresolvable += cb;
            if cl > count_metric(&f, 53)? {
                s.confidence_micro_more_than_direction_cases += 1;
            }
            if cs > count_metric(&f, 58)? {
                s.confidence_sign_more_than_direction_cases += 1;
            }
        }
    }
    Ok(s)
}
pub fn run(input: &Path) -> io::Result<bool> {
    let r = analyze(input)?;
    println!("GUARDADAPTIVE RELEASE QUALIFICATION {}", input.display());
    println!(
        "cases={} neutral_worse={} movement_worse={} combined_worse={}",
        r.cases, r.neutral_worse, r.movement_worse, r.combined_worse
    );
    println!(
        "micro_lost_total={} reversed_total={} severe_error_samples={} worst_score={:.8}",
        r.micro_lost_total, r.reversed_total, r.high_error_total, r.worst_combined
    );
    if let Some(model) = &r.report_resolution {
        println!("REPORT RESOLUTION: {} cases={} below_report_resolution_total={} quantization_changed_samples={}",
          model,r.resolution_cases,r.below_report_resolution_total,r.quantization_changed_total);
        println!("below_report_resolution counts ideal intended movement that independently rounds to zero without sensor noise or bias; it is not a physical sensor measurement.");
    }
    if r.partition_cases > 0 {
        println!("MICRO LOSS PARTITION: {} cases", r.partition_cases);
        println!(
            "GUARD: resolvable_in_isolation={} below_report_resolution={}",
            r.guard_micro_lost_above_resolution, r.guard_micro_lost_below_resolution
        );
        println!(
            "PRECISION: resolvable_in_isolation={} below_report_resolution={}",
            r.precision_micro_lost_above_resolution, r.precision_micro_lost_below_resolution
        );
        println!("The partition is computed from isolated zero-bias/no-noise quantization; it is NOT certain recovery of movement in the actual corrupted stream.");
    }
    if r.motion_first_cases > 0 {
        println!("MOTIONFIRST RESEARCH CANDIDATE: cases={} better_motion_vs_legacy={} worse_neutral_vs_legacy={}",
            r.motion_first_cases,r.motion_first_better_motion,r.motion_first_worse_neutral);
        println!("MOTIONFIRST vs FIXED5%: worse_neutral={} worse_motion={} worse_joint={} suppressed_micro={} wrong_direction={} (reportable_micro_loss={} sub_lsb_micro_loss={})",
            r.motion_first_worse_than_fixed_neutral,r.motion_first_worse_than_fixed_motion,r.motion_first_worse_than_fixed_joint,
            r.motion_first_micro_lost,r.motion_first_wrong_direction,r.motion_first_loss_reportable,r.motion_first_loss_sub_lsb);
        println!("MOTIONFIRST RELEASE GATE: BLOCKED unless neutral, motion and safety metrics meet independent thresholds; candidate remains experimental.");
    }
    if r.noise_guard_cases > 0 {
        println!("NOISEGUARD EXPERIMENT: cases={} better_neutral_vs_motionfirst={} worse_motion_vs_motionfirst={}",
            r.noise_guard_cases,r.noise_better_neutral_than_motionfirst,r.noise_worse_motion_than_motionfirst);
        println!("NOISEGUARD: worse_neutral_vs_fixed5={} worse_motion_vs_fixed5={} suppressed_micro={} reversed_total={} reportable_micro_lost={} sub_lsb_micro_lost={}",
            r.noise_worse_neutral_than_fixed,r.noise_worse_motion_than_fixed,r.noise_micro_lost_total,
            r.noise_wrong_direction_total,r.noise_lost_above_resolution,r.noise_lost_below_resolution);
        println!("NOISEGUARD RELEASE GATE: BLOCKED until independently qualified on synthetic AND hardware data.");
    }
    if r.sign_opposite_cases > 0 {
        println!("ANGLE-ONLY DIAGNOSTIC (magnitude-independent sign): cases={} hybrid_opposite={} direction_opposite={}",
            r.sign_opposite_cases,r.hybrid_sign_opposite,r.direction_sign_opposite);
        println!("WARNING: thresholded wrong-direction counters can improve when output is simply shrunken. Inspect sign-only counter.");
    }
    if r.confidence_cases > 0 {
        println!("CONFIDENCE DIRECTION (research only): cases={} micro_lost={} wrong_direction={} sign_opposite={}",
            r.confidence_cases,r.confidence_micro_lost,r.confidence_wrong_direction,r.confidence_sign_opposite);
        println!("CONFIDENCE vs DIRECTIONGUARD: micro_regression_cases={} sign_opposite_regression_cases={} resolvable_micro_loss={} sub_lsb_loss={}",
            r.confidence_micro_more_than_direction_cases,r.confidence_sign_more_than_direction_cases,
            r.confidence_micro_resolvable,r.confidence_micro_unresolvable);
        println!(
            "CONFIDENCE RELEASE GATE: BLOCKED. Historical baseline & physical testing required."
        );
    }
    if r.direction_cases > 0 {
        println!("DIRECTIONGUARD RESEARCH: cases={} wrong_direction_total={} vs hybrid={} improved_direction_cases={} worse_direction_cases={}",
            r.direction_cases,r.direction_wrong_direction,r.hybrid_wrong_direction,
            r.direction_fewer_wrong_than_hybrid_cases,r.direction_more_wrong_than_hybrid_cases);
        println!("DIRECTIONGUARD: micro_loss={} vs hybrid={} worse_micro_cases={} reportable_loss={} sub_lsb_loss={}",
            r.direction_micro_lost,r.hybrid_micro_lost,r.direction_more_micro_than_hybrid_cases,
            r.direction_resolvable_micro_loss,r.direction_unresolvable_micro_loss);
        println!("DIRECTIONGUARD RELEASE: BLOCKED; latency and motion distortion unvalidated on hardware.");
    }
    if r.hybrid_cases > 0 {
        println!("HYBRID EXPERIMENT: cases={} better_neutral_vs_motionfirst={} better_motion_vs_motionfirst={} more_micro_loss_cases={}",
            r.hybrid_cases,r.hybrid_better_neutral_vs_motionfirst,r.hybrid_better_motion_vs_motionfirst,r.hybrid_more_micro_loss_cases);
        println!("HYBRID: suppressed_micro={} reversed_total={} reportable_micro_loss={} sub_lsb_micro_loss={}",
            r.hybrid_micro_lost,r.hybrid_wrong_direction,r.hybrid_resolvable_micro_loss,r.hybrid_unresolvable_micro_loss);
        println!(
            "HYBRID RELEASE GATE: BLOCKED unless independently validated and safe on hardware."
        );
    }
    if r.precision_cases > 0 {
        println!("PRECISION EXPERIMENT: {} cases; better_motion={} worse_neutral={} saved_micro_samples={} remaining_micro_lost={} reversed_total={}",
          r.precision_cases,r.precision_better_motion,r.precision_worse_neutral,
          r.precision_saved_micro,r.precision_micro_lost_total,r.precision_reversed_total);
        println!("Precision metrics are diagnostic, NOT a substitute for release qualification.");
    }
    if r.passed() {
        println!("QUALIFICATION PASS (on the checked synthetic corpus only; real hardware remains unverified).");
    } else {
        println!("QUALIFICATION BLOCKED: regression or unresolved fidelity failure; do NOT ship a default controller filter.");
    }
    Ok(r.passed())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_and_extended_reports_have_known_width() {
        use std::{env, fs};
        let mut f = env::temp_dir();
        f.push(format!("guardadaptive-qualify-{}.csv", std::process::id()));
        let header="case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score";
        fs::write(
            &f,
            format!(
                "{header}\n0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1\n"
            ),
        )
        .unwrap();
        let old = analyze(&f).unwrap();
        assert_eq!(old.cases, 1);
        assert_eq!(old.precision_cases, 0);
        fs::write(&f,format!("{header},precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction\n0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1,0.003,0.1,20,0\n")).unwrap();
        let extended = analyze(&f).unwrap();
        assert_eq!(extended.precision_cases, 1);
        assert_eq!(extended.precision_saved_micro, 30);
        assert_eq!(extended.precision_better_motion, 1);
        assert_eq!(extended.precision_worse_neutral, 1);
        let _ = fs::remove_file(&f);
    }
    #[test]
    fn new_resolution_metadata_is_parsed_without_leaking_into_release_pass() {
        use std::{env, fs};
        let path = env::temp_dir().join(format!(
            "guardadaptive-resolution-{}-test.csv",
            std::process::id()
        ));
        let h="case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score,precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction,resolution,report_lsb,below_report_resolution,quantization_changed";
        let row="0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1,0.003,0.1,20,0,xinput16,0.0000305185,128,160";
        fs::write(&path, format!("{h}\n{row}\n")).unwrap();
        let r = analyze(&path).unwrap();
        assert_eq!(r.report_resolution.as_deref(), Some("xinput16"));
        assert_eq!(r.below_report_resolution_total, 128);
        assert_eq!(r.quantization_changed_total, 160);
        assert_eq!(r.precision_cases, 1);
        assert!(!r.passed());
        let _ = fs::remove_file(&path);
    }
    #[test]
    fn partition_columns_must_sum_to_existing_micro_loss_metrics() {
        use std::{env, fs};
        let p = env::temp_dir().join(format!(
            "guardadaptive-partition-{}.csv",
            std::process::id()
        ));
        let header="case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score,precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction,resolution,report_lsb,below_report_resolution,quantization_changed,guard_micro_lost_above_resolution,guard_micro_lost_below_resolution,precision_micro_lost_above_resolution,precision_micro_lost_below_resolution";
        let row="0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1,0.003,0.1,20,0,xinput16,0.0000305185,128,160,10,40,5,15";
        fs::write(&p, format!("{header}\n{row}\n")).unwrap();
        let report = analyze(&p).unwrap();
        assert_eq!(report.guard_micro_lost_above_resolution, 10);
        assert_eq!(report.guard_micro_lost_below_resolution, 40);
        assert_eq!(report.precision_micro_lost_above_resolution, 5);
        assert_eq!(report.precision_micro_lost_below_resolution, 15);
        assert_eq!(report.partition_cases, 1);
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn motionfirst_new_columns_are_scored_separately() {
        use std::{env, fs};
        let path = env::temp_dir().join(format!(
            "guardadaptive-motion-first-{}.csv",
            std::process::id()
        ));
        let header="case_id,seed,source,amplitude_pct,direction_deg,transition,intent,noise_pct,guard_neutral_rmse,guard_motion_rmse,raw_neutral_rmse,raw_motion_rmse,fixed_neutral_rmse,fixed_motion_rmse,oracle_motion_rmse,micro_lost,wrong_direction,false_positive,high_error,auto_updates,score,precision_neutral_rmse,precision_motion_rmse,precision_micro_lost,precision_wrong_direction,resolution,report_lsb,below_report_resolution,quantization_changed,guard_micro_lost_above_resolution,guard_micro_lost_below_resolution,precision_micro_lost_above_resolution,precision_micro_lost_below_resolution,motionfirst_neutral_rmse,motionfirst_motion_rmse,motionfirst_micro_lost,motionfirst_wrong_direction,motionfirst_micro_lost_above_resolution,motionfirst_micro_lost_below_resolution";
        let row="0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1,0.003,0.1,20,0,xinput16,0.0000305185,128,160,10,40,5,15,0.004,0.2,12,3,9,3";
        fs::write(&path, format!("{header}\n{row}\n")).unwrap();
        let r = analyze(&path).unwrap();
        assert_eq!(r.motion_first_cases, 1);
        assert_eq!(r.motion_first_better_motion, 1);
        assert_eq!(r.motion_first_worse_neutral, 1);
        assert_eq!(r.motion_first_worse_than_fixed_motion, 0);
        assert_eq!(r.motion_first_worse_than_fixed_neutral, 1);
        assert_eq!(r.motion_first_micro_lost, 12);
        assert_eq!(r.motion_first_loss_reportable, 9);
        assert_eq!(r.motion_first_loss_sub_lsb, 3);
        assert_eq!(r.motion_first_wrong_direction, 3);
        let _ = fs::remove_file(&path);
    }
    #[test]
    fn v10_noiseguard_report_requires_exact_schema_and_integral_counters() {
        use std::{env, fs};
        let path = env::temp_dir().join(format!(
            "guardadaptive-v10-schema-{}.csv",
            std::process::id()
        ));
        let head = SWEEP_HEADER
            .split(',')
            .take(45)
            .collect::<Vec<_>>()
            .join(",");
        let row="0,1,grid,0.001,0,0,1,0,0.002,0.5,0.1,0.6,0.001,0.49,0,50,0,0,0,0,1,0.003,0.1,20,0,xinput16,0.0000305185,128,160,10,40,5,15,0.004,0.2,12,3,9,3,0.001,0.25,10,2,8,2";
        fs::write(&path, format!("{head}\n{row}\n")).unwrap();
        let report = analyze(&path).unwrap();
        assert_eq!(report.noise_guard_cases, 1);
        assert_eq!(report.noise_better_neutral_than_motionfirst, 1);
        assert_eq!(report.noise_worse_motion_than_motionfirst, 1);
        assert_eq!(report.noise_micro_lost_total, 10);
        assert_eq!(report.noise_wrong_direction_total, 2);
        assert_eq!(report.noise_lost_above_resolution, 8);
        assert!(!report.passed());
        // NoiseGuard candidate metadata is diagnostic only; not release grant.
        fs::write(
            &path,
            format!(
                "{head}\n{}\n",
                row.replacen(",50,0,0,0,0,1", ",50.2,0,0,0,0,1", 1)
            ),
        )
        .unwrap();
        assert!(
            analyze(&path).is_err(),
            "fractional micro counts must be rejected"
        );
        fs::write(
            &path,
            format!(
                "{head}\n{}\n",
                row.replacen(",0.001,0.25,10,2,8,2", ",0.001,0.25,10,2,8,1", 1)
            ),
        )
        .unwrap();
        assert!(analyze(&path).is_err(), "micro partitions must add up");
        let bad_header = head.replace("noiseguard_motion_rmse", "unauthorized_alt_metric");
        fs::write(&path, format!("{bad_header}\n{row}\n")).unwrap();
        assert!(
            analyze(&path).is_err(),
            "header substitution must be rejected"
        );
        fs::write(
            &path,
            format!(
                "{head}\n{}\n",
                row.split(',').take(39).collect::<Vec<_>>().join(",")
            ),
        )
        .unwrap();
        assert!(analyze(&path).is_err(), "truncated row must be rejected");
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                head.split(',').take(39).collect::<Vec<_>>().join(","),
                row.split(',').take(39).collect::<Vec<_>>().join(",")
            ),
        )
        .unwrap();
        assert_eq!(
            analyze(&path).unwrap().motion_first_cases,
            1,
            "v0.9 39-column reports must remain backwards compatible"
        );
        let _ = fs::remove_file(&path);
    }
    #[test]
    fn hybrid_51_column_report_is_strict_and_cannot_become_release_pass() {
        use std::{env, fs};
        let p = env::temp_dir().join(format!(
            "guardadaptive-hybrid-schema-{}.csv",
            std::process::id()
        ));
        let header51 = SWEEP_HEADER
            .split(',')
            .take(51)
            .collect::<Vec<_>>()
            .join(",");
        let mut f = vec!["0"; 51];
        f[1] = "1";
        f[2] = "grid";
        f[15] = "12";
        f[23] = "8";
        f[25] = "xinput16";
        f[26] = "0.0000305185";
        f[29] = "10";
        f[30] = "2";
        f[31] = "6";
        f[32] = "2";
        f[35] = "7";
        f[37] = "5";
        f[38] = "2";
        f[41] = "8";
        f[43] = "5";
        f[44] = "3";
        f[47] = "4";
        f[48] = "2";
        f[49] = "3";
        f[50] = "1";
        fs::write(&p, format!("{header51}\n{}\n", f.join(","))).unwrap();
        let report = analyze(&p).unwrap();
        assert_eq!(report.hybrid_cases, 1);
        assert_eq!(report.hybrid_micro_lost, 4);
        assert_eq!(report.hybrid_resolvable_micro_loss, 3);
        assert_eq!(report.hybrid_unresolvable_micro_loss, 1);
        assert_eq!(report.hybrid_wrong_direction, 2);
        assert!(!report.passed(), "candidate cannot replace release gate");
        let same = f.iter().copied().take(45).collect::<Vec<_>>().join(",");
        let old = SWEEP_HEADER
            .split(',')
            .take(45)
            .collect::<Vec<_>>()
            .join(",");
        fs::write(&p, format!("{old}\n{same}\n")).unwrap();
        assert_eq!(analyze(&p).unwrap().noise_guard_cases, 1);
        assert_eq!(analyze(&p).unwrap().hybrid_cases, 0);
        f[50] = "2";
        fs::write(&p, format!("{header51}\n{}\n", f.join(","))).unwrap();
        assert!(
            analyze(&p).is_err(),
            "sum of hybrid partitions must equal hybrid losses"
        );
        f[50] = "1";
        f[47] = "4.25";
        fs::write(&p, format!("{header51}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&p).is_err(), "fractional counters rejected");
        let _ = fs::remove_file(p);
    }
    #[test]
    fn direction_57_column_metrics_are_strict_and_never_unlock_release() {
        use std::{env, fs};
        let path = env::temp_dir().join(format!("guardadaptive-v13-{}.csv", std::process::id()));
        let header57 = SWEEP_HEADER
            .split(',')
            .take(57)
            .collect::<Vec<_>>()
            .join(",");
        let mut f = vec!["0"; 57];
        f[1] = "1";
        f[2] = "grid";
        f[15] = "12";
        f[23] = "8";
        f[25] = "xinput16";
        f[26] = "0.0000305185";
        f[29] = "10";
        f[30] = "2";
        f[31] = "6";
        f[32] = "2";
        f[35] = "7";
        f[37] = "5";
        f[38] = "2";
        f[41] = "8";
        f[43] = "5";
        f[44] = "3";
        f[47] = "4";
        f[48] = "4";
        f[49] = "3";
        f[50] = "1";
        f[53] = "5";
        f[54] = "2";
        f[55] = "4";
        f[56] = "1";
        fs::write(&path, format!("{header57}\n{}\n", f.join(","))).unwrap();
        let r = analyze(&path).unwrap();
        assert_eq!(r.direction_cases, 1);
        assert_eq!(r.direction_micro_lost, 5);
        assert_eq!(r.direction_wrong_direction, 2);
        assert_eq!(r.direction_more_micro_than_hybrid_cases, 1);
        assert_eq!(r.direction_fewer_wrong_than_hybrid_cases, 1);
        assert_eq!(r.direction_resolvable_micro_loss, 4);
        assert!(
            !r.passed(),
            "diagnostic candidate does not grant release approval"
        );
        // Old 51-column reports must remain accepted.
        let head51 = SWEEP_HEADER
            .split(',')
            .take(51)
            .collect::<Vec<_>>()
            .join(",");
        fs::write(&path, format!("{head51}\n{}\n", f[..51].join(","))).unwrap();
        assert_eq!(analyze(&path).unwrap().hybrid_cases, 1);
        assert_eq!(analyze(&path).unwrap().direction_cases, 0);
        f[56] = "2";
        fs::write(&path, format!("{header57}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&path).is_err(), "broken partitions rejected");
        f[56] = "1";
        f[53] = "5.1";
        fs::write(&path, format!("{header57}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&path).is_err(), "fractional counters rejected");
        let _ = fs::remove_file(path);
    }
    #[test]
    fn v14_sign_only_diagnostic_cannot_be_gamed_by_shrinking_output() {
        use std::{env, fs};
        let p = env::temp_dir().join(format!("guardadaptive-v14-sign-{}.csv", std::process::id()));
        let header59 = SWEEP_HEADER
            .split(',')
            .take(59)
            .collect::<Vec<_>>()
            .join(",");
        let mut f = vec!["0"; 59];
        f[1] = "1";
        f[2] = "grid";
        f[15] = "12";
        f[23] = "8";
        f[25] = "xinput16";
        f[26] = "0.0000305185";
        f[29] = "10";
        f[30] = "2";
        f[31] = "6";
        f[32] = "2";
        f[35] = "7";
        f[37] = "5";
        f[38] = "2";
        f[41] = "8";
        f[43] = "5";
        f[44] = "3";
        f[47] = "4";
        f[48] = "3";
        f[49] = "3";
        f[50] = "1";
        f[53] = "5";
        f[54] = "2";
        f[55] = "4";
        f[56] = "1";
        f[57] = "12";
        f[58] = "14";
        fs::write(&p, format!("{header59}\n{}\n", f.join(","))).unwrap();
        let result = analyze(&p).unwrap();
        assert_eq!(result.sign_opposite_cases, 1);
        assert_eq!(result.hybrid_sign_opposite, 12);
        assert_eq!(result.direction_sign_opposite, 14);
        assert!(!result.passed());
        f[58] = "1"; // Thresholded wrong_direction is 2; sign-only cannot be 1.
        fs::write(&p, format!("{header59}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&p).is_err());
        f[58] = "14";
        f[57] = "2"; // same constraint on baseline Hybrid.
        fs::write(&p, format!("{header59}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&p).is_err());
        // v0.13 57-column data retains exactly its prior interpretation.
        let h57 = SWEEP_HEADER
            .split(',')
            .take(57)
            .collect::<Vec<_>>()
            .join(",");
        fs::write(&p, format!("{h57}\n{}\n", f[..57].join(","))).unwrap();
        assert_eq!(analyze(&p).unwrap().direction_cases, 1);
        assert_eq!(analyze(&p).unwrap().sign_opposite_cases, 0);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn confidence_66_column_schema_is_strict_and_v14_remains_readable() {
        use std::{env, fs};
        let path = env::temp_dir().join(format!(
            "guardadaptive-v15-schema-{}.csv",
            std::process::id()
        ));
        let mut f = vec!["0"; 66];
        f[1] = "1";
        f[2] = "grid";
        f[15] = "12";
        f[23] = "8";
        f[25] = "xinput16";
        f[26] = "0.0000305185";
        f[29] = "10";
        f[30] = "2";
        f[31] = "6";
        f[32] = "2";
        f[35] = "7";
        f[37] = "5";
        f[38] = "2";
        f[41] = "8";
        f[43] = "5";
        f[44] = "3";
        f[47] = "4";
        f[48] = "3";
        f[49] = "3";
        f[50] = "1";
        f[53] = "5";
        f[54] = "2";
        f[55] = "4";
        f[56] = "1";
        f[57] = "12";
        f[58] = "14";
        f[61] = "5";
        f[62] = "2";
        f[63] = "4";
        f[64] = "1";
        f[65] = "15";
        fs::write(&path, format!("{SWEEP_HEADER}\n{}\n", f.join(","))).unwrap();
        let r = analyze(&path).unwrap();
        assert_eq!(r.confidence_cases, 1);
        assert_eq!(r.confidence_micro_lost, 5);
        assert_eq!(r.confidence_sign_opposite, 15);
        assert_eq!(r.confidence_micro_more_than_direction_cases, 0);
        assert_eq!(r.confidence_sign_more_than_direction_cases, 1);
        assert!(!r.passed());
        let header59 = SWEEP_HEADER
            .split(',')
            .take(59)
            .collect::<Vec<_>>()
            .join(",");
        fs::write(&path, format!("{header59}\n{}\n", f[..59].join(","))).unwrap();
        assert_eq!(analyze(&path).unwrap().sign_opposite_cases, 1);
        assert_eq!(analyze(&path).unwrap().confidence_cases, 0);
        f[64] = "0";
        fs::write(&path, format!("{SWEEP_HEADER}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&path).is_err(), "micro partition must match");
        f[64] = "1";
        f[65] = "1";
        fs::write(&path, format!("{SWEEP_HEADER}\n{}\n", f.join(","))).unwrap();
        assert!(analyze(&path).is_err(), "sign counter cannot undercount");
        let _ = fs::remove_file(path);
    }
    #[test]
    fn empty_report_is_not_pass() {
        assert!(!Summary::default().passed());
    }
    #[test]
    fn motion_regression_blocks_release_even_if_neutral_is_perfect() {
        let r = Summary {
            cases: 100,
            movement_worse: 1,
            ..Default::default()
        };
        assert!(!r.passed());
    }
}
