//! v0.68 offline train-derived fixed-center comparisons.
//! All candidate parameters come from TRAIN. HOLDOUT is only evaluated.
//! Any supplied "intent" CSV remains an unverified declaration, not truth.
use crate::{
    dataset_split_v67,
    profile_store::Stick,
    reference_gate_v41::{comparison_frames, ComparisonFrames},
    Vec2,
};
use std::{fs, io, path::Path};

fn invalid(s: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
#[derive(Debug, Clone)]
pub struct Distribution {
    pub frames: usize,
    pub mean: Vec2,
    pub axis_rms: f64,
    pub max_magnitude: f64,
}
fn distribution(frames: &[Vec2]) -> Distribution {
    let mut mean = Vec2::ZERO;
    let mut squared = 0.0;
    let mut max = 0.0_f64;
    for &v in frames {
        mean = mean + v;
        squared += v.length_squared();
        max = max.max(v.norm());
    }
    let n = frames.len().max(1) as f64;
    Distribution {
        frames: frames.len(),
        mean: mean * (1.0 / n),
        axis_rms: (squared / n).sqrt(),
        max_magnitude: max,
    }
}
#[derive(Debug, Clone)]
pub struct Scores {
    pub evaluated_frames: usize,
    pub raw_rmse: f64,
    pub fixed_train_center_rmse: f64,
    pub declared_micro_frames: usize,
    pub raw_micro_lost: usize,
    pub fixed_micro_lost: usize,
    pub raw_wrong_direction: usize,
    pub fixed_wrong_direction: usize,
}
fn scores(data: &ComparisonFrames, center: Vec2) -> io::Result<Scores> {
    let truth = data
        .declared_intentions
        .as_ref()
        .ok_or_else(|| invalid("declared truth absent"))?;
    if truth.len() != data.axes.len() || truth.len() <= 128 {
        return Err(invalid("unmatched reference count"));
    }
    let mut raw_sse = 0.0;
    let mut fixed_sse = 0.0;
    let mut result = Scores {
        evaluated_frames: 0,
        raw_rmse: 0.0,
        fixed_train_center_rmse: 0.0,
        declared_micro_frames: 0,
        raw_micro_lost: 0,
        fixed_micro_lost: 0,
        raw_wrong_direction: 0,
        fixed_wrong_direction: 0,
    };
    // First 128 entries are reserved for reported neutral-calibration
    // preconditions in each split, not scored as held-motion metrics.
    for (&raw, &intent) in data.axes.iter().zip(truth.iter()).skip(128) {
        let fixed = (raw - center).saturate();
        raw_sse += (raw - intent).length_squared();
        fixed_sse += (fixed - intent).length_squared();
        if intent.norm() > 0.0 && intent.norm() <= 0.01 {
            result.declared_micro_frames += 1;
            if raw.norm() < intent.norm() * 0.5 {
                result.raw_micro_lost += 1;
            }
            if fixed.norm() < intent.norm() * 0.5 {
                result.fixed_micro_lost += 1;
            }
        }
        if intent.norm() > 0.0001 {
            if raw.x * intent.x + raw.y * intent.y < 0.0 {
                result.raw_wrong_direction += 1;
            }
            if fixed.x * intent.x + fixed.y * intent.y < 0.0 {
                result.fixed_wrong_direction += 1;
            }
        }
        result.evaluated_frames += 1;
    }
    let n = result.evaluated_frames.max(1) as f64;
    result.raw_rmse = (raw_sse / n).sqrt();
    result.fixed_train_center_rmse = (fixed_sse / n).sqrt();
    if !result.raw_rmse.is_finite() || !result.fixed_train_center_rmse.is_finite() {
        return Err(invalid("nonfinite score"));
    }
    Ok(result)
}
#[derive(Debug, Clone)]
pub struct Analysis {
    pub train: Distribution,
    pub holdout: Distribution,
    pub train_center: Option<Vec2>,
    pub train_scores: Option<Scores>,
    pub holdout_scores: Option<Scores>,
    pub reference_present: bool,
}
pub fn compare(
    train_raw: &Path,
    train_ref: Option<&Path>,
    hold_raw: &Path,
    hold_ref: Option<&Path>,
    stick: Stick,
) -> io::Result<Analysis> {
    let mut files = vec![train_raw, hold_raw];
    if let Some(path) = train_ref {
        files.push(path);
    }
    if let Some(path) = hold_ref {
        files.push(path);
    }
    // A bounded in-memory before/after identity check detects changes during
    // the audit in ordinary conditions. It is not cryptographic provenance.
    for file in &files {
        let meta = fs::symlink_metadata(file)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 20_000_000 {
            return Err(invalid("nonregular, redirected, or oversized dataset"));
        }
    }
    let before = files.iter().map(fs::read).collect::<io::Result<Vec<_>>>()?;
    let v = dataset_split_v67::inspect(train_raw, train_ref, hold_raw, hold_ref, stick)?;
    let train = comparison_frames(train_raw, train_ref, stick)?;
    let holdout = comparison_frames(hold_raw, hold_ref, stick)?;
    if train.axes.len() != v.train_rows || holdout.axes.len() != v.holdout_rows {
        return Err(invalid("dataset changed between validation and metrics"));
    }
    let (train_scores, holdout_scores) = if let Some(center) = train.declared_neutral_center {
        // IMPORTANT: deliberately ignore holdout.declared_neutral_center
        // for candidate parameters, so the holdout cannot tune the center.
        (
            Some(scores(&train, center)?),
            Some(scores(&holdout, center)?),
        )
    } else {
        (None, None)
    };
    let after = files.iter().map(fs::read).collect::<io::Result<Vec<_>>>()?;
    if before != after {
        return Err(invalid("input bytes mutated during metrics calculation"));
    }
    Ok(Analysis {
        train: distribution(&train.axes),
        holdout: distribution(&holdout.axes),
        train_center: train.declared_neutral_center,
        train_scores,
        holdout_scores,
        reference_present: v.reference_present,
    })
}
fn json_distribution(x: &Distribution) -> String {
    format!(
        "{{\"frames\":{},\"mean_x\":{:.10},\"mean_y\":{:.10},\"rms_magnitude\":{:.10},\"max_magnitude\":{:.10}}}",
        x.frames,x.mean.x,x.mean.y,x.axis_rms,x.max_magnitude
    )
}
fn json_scores(x: &Scores) -> String {
    format!(
        concat!(
            "{{\"evaluated_frames\":{},\"raw_rmse_declared_intent\":{:.10},",
            "\"fixed_train_center_rmse_declared_intent\":{:.10},",
            "\"declared_micro_frames\":{},\"raw_micro_lost\":{},",
            "\"fixed_train_center_micro_lost\":{},",
            "\"raw_wrong_direction\":{},\"fixed_train_center_wrong_direction\":{}}}"
        ),
        x.evaluated_frames,
        x.raw_rmse,
        x.fixed_train_center_rmse,
        x.declared_micro_frames,
        x.raw_micro_lost,
        x.fixed_micro_lost,
        x.raw_wrong_direction,
        x.fixed_wrong_direction
    )
}
pub fn run(
    train_raw: &Path,
    train_ref: Option<&Path>,
    hold_raw: &Path,
    hold_ref: Option<&Path>,
    stick: Stick,
    out: &Path,
) -> io::Result<()> {
    let c = compare(train_raw, train_ref, hold_raw, hold_ref, stick)?;
    let mut report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v68_untrusted_record_fixed_center_comparison\",",
            "\"stick\":\"{}\",\"mode\":\"{}\",",
            "\"train\":{},\"holdout\":{},",
            "\"training_only_parameter_source\":true,",
            "\"holdout_parameters_never_fit\":true,",
            "\"input_bytes_unchanged_before_after_analysis\":true,",
            "\"input_bytes_cryptographically_attested\":false,",
            "\"physical_player_intent_authenticated\":false,",
            "\"raw_xinput_origin_authenticated\":false,",
            "\"model_trained\":false,\"algorithm_promotion\":false,"
        ),
        stick.name(),
        if c.reference_present {
            "DECLARED_REFERENCE_UNVERIFIED"
        } else {
            "RAW_ONLY_NO_ACCURACY"
        },
        json_distribution(&c.train),
        json_distribution(&c.holdout)
    );
    if let Some(center) = c.train_center {
        let train = c
            .train_scores
            .as_ref()
            .ok_or_else(|| invalid("train scores missing"))?;
        let hold = c
            .holdout_scores
            .as_ref()
            .ok_or_else(|| invalid("holdout scores missing"))?;
        report.push_str(&format!(
            "\"declared_train_neutral_center_x\":{:.10},\"declared_train_neutral_center_y\":{:.10},",
            center.x,center.y
        ));
        report.push_str(&format!(
            "\"train_declared_accuracy\":{},\"holdout_declared_accuracy\":{},",
            json_scores(train),
            json_scores(hold)
        ));
        report.push_str("\"reference_metrics_only_not_physically_verified\":true,");
    } else {
        report.push_str("\"accuracy_scoring_available\":false,");
    }
    report.push_str(
      "\"real_controller_validation\":false,\"device_driver_abi_verified\":false,\"driver_writes\":0,\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}\n"
    );
    crate::truth_lab::save_new(out, &report)?;
    println!("V068_RECORD_COMPARE=PASS train={} holdout={} reference={} model_promoted=FALSE production=BLOCKED",
             c.train.frames,c.holdout.frames,c.reference_present);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ID: AtomicUsize = AtomicUsize::new(0);
    fn fixtures() -> (std::path::PathBuf, [std::path::PathBuf; 4]) {
        let dir = std::env::temp_dir().join(format!(
            "ga-v68-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&dir).unwrap();
        let files = [
            dir.join("train.csv"),
            dir.join("train-intent.csv"),
            dir.join("holdout.csv"),
            dir.join("holdout-intent.csv"),
        ];
        for split in 0..2usize {
            let offset = if split == 0 { 0.02 } else { 0.08 };
            let mut raw=String::from("sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source\n");
            let mut ref_csv =
                String::from("sample,elapsed_us,intended_x,intended_y,reference_source\n");
            for i in 0..384usize {
                let intended = if i < 128 {
                    0.0
                } else {
                    ((i + split * 31) as f64 * 0.13).sin() * 0.04
                };
                let x = offset + intended + ((i * 7 % 11) as f64 - 5.0) * 0.00001;
                let timestamp = (i as u64 + 1) * 8000;
                raw.push_str(&format!(
                    "{i},{timestamp},{},{x:.9},0.0,0.0,0.0,0,8,9,synthetic_fixture\n",
                    i / 2
                ));
                ref_csv.push_str(&format!(
                    "{i},{timestamp},{intended:.9},0.0,synthetic_fixture\n"
                ));
            }
            fs::write(&files[split * 2], raw).unwrap();
            fs::write(&files[split * 2 + 1], ref_csv).unwrap();
        }
        (dir, files)
    }
    #[test]
    fn train_only_center_holdout_never_fits_own_baseline() {
        let (dir, files) = fixtures();
        let v = compare(
            &files[0],
            Some(&files[1]),
            &files[2],
            Some(&files[3]),
            Stick::Left,
        )
        .unwrap();
        let center = v.train_center.unwrap();
        assert!((center.x - 0.02).abs() < 0.0001);
        assert_eq!(v.train_scores.as_ref().unwrap().evaluated_frames, 256);
        // Deliberately changed holdout offset: applying the training center
        // cannot spuriously report an independently fitted holdout baseline.
        assert!(v.holdout_scores.as_ref().unwrap().fixed_train_center_rmse > 0.05);
        assert!(
            v.holdout_scores.as_ref().unwrap().fixed_train_center_rmse
                < v.holdout_scores.as_ref().unwrap().raw_rmse
        );
        let out = dir.join("evidence.json");
        run(
            &files[0],
            Some(&files[1]),
            &files[2],
            Some(&files[3]),
            Stick::Left,
            &out,
        )
        .unwrap();
        let bytes = fs::read(&out).unwrap();
        assert!(run(
            &files[0],
            Some(&files[1]),
            &files[2],
            Some(&files[3]),
            Stick::Left,
            &out
        )
        .is_err());
        assert_eq!(bytes, fs::read(out).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn raw_only_omits_reference_accuracy_and_neutral_center() {
        let (dir, files) = fixtures();
        let c = compare(&files[0], None, &files[2], None, Stick::Right).unwrap();
        assert!(c.train_center.is_none());
        assert!(c.holdout_scores.is_none());
        let out = dir.join("rawonly.json");
        run(&files[0], None, &files[2], None, Stick::Right, &out).unwrap();
        let bytes = fs::read_to_string(out).unwrap();
        assert!(bytes.contains("\"accuracy_scoring_available\":false"));
        assert!(!bytes.contains("raw_rmse_declared_intent"));
        assert!(!bytes.contains("declared_train_neutral_center"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn split_leak_and_false_physical_provenance_are_rejected() {
        let (dir, files) = fixtures();
        assert!(compare(&files[0], None, &files[0], None, Stick::Left).is_err());
        let refbytes = fs::read_to_string(&files[3]).unwrap();
        fs::write(
            &files[3],
            refbytes.replace("synthetic_fixture", "physically_verified"),
        )
        .unwrap();
        assert!(compare(
            &files[0],
            Some(&files[1]),
            &files[2],
            Some(&files[3]),
            Stick::Left
        )
        .is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
