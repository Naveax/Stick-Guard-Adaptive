//! v0.67 read-only strict CSV split hygiene on top of the v0.41 validator.
//! This NEVER trains on holdout, emits physical verification, or touches a
//! controller, virtual driver, active profile or game input.
use crate::{profile_store::Stick, reference_gate_v41::checked_split};
use std::{collections::HashMap, fs, io, path::Path};
const REPLAY_WINDOW: usize = 64;
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn identical_paths(paths: &[&Path]) -> io::Result<()> {
    let mut canonical = Vec::with_capacity(paths.len());
    for path in paths {
        let m = fs::symlink_metadata(path)?;
        if !m.is_file() || m.file_type().is_symlink() {
            return Err(invalid(
                "dataset source must be an ordinary non-symlinked file",
            ));
        }
        let absolute = fs::canonicalize(path)?;
        if canonical.iter().any(|p| p == &absolute) {
            return Err(invalid(
                "same canonical file reused across train, holdout or references",
            ));
        }
        canonical.push(absolute);
    }
    Ok(())
}
fn fnv_row(s: &str) -> u64 {
    let mut value = 0xcbf29ce484222325_u64;
    for byte in s.bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    value
}
fn raw_payload(path: &Path) -> io::Result<Vec<String>> {
    // The v41 validated CSV is bounded to 20 MB and <=100k rows.
    // Exclude sample, elapsed_us, packet and source when identifying a
    // repeated motion sequence; clocks/packet counters can be relabeled.
    let text = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols = line.split(',').collect::<Vec<_>>();
        if cols.len() != 11 {
            return Err(invalid("raw dataset changed during split analysis"));
        }
        out.push(cols[3..10].join(","));
    }
    Ok(out)
}
fn replay_blocks(train: &[String], holdout: &[String]) -> bool {
    if train.len() < REPLAY_WINDOW || holdout.len() < REPLAY_WINDOW {
        return false;
    }
    let train_hash = train.iter().map(|v| fnv_row(v)).collect::<Vec<_>>();
    let test_hash = holdout.iter().map(|v| fnv_row(v)).collect::<Vec<_>>();
    let base = 0x9E3779B185EBCA87_u64;
    let power = (0..REPLAY_WINDOW - 1).fold(1_u64, |x, _| x.wrapping_mul(base));
    fn scan(hashes: &[u64], power: u64, base: u64) -> Vec<u64> {
        let mut h = hashes[..REPLAY_WINDOW]
            .iter()
            .fold(0_u64, |acc, v| acc.wrapping_mul(base).wrapping_add(*v));
        let mut result = vec![h];
        for i in REPLAY_WINDOW..hashes.len() {
            h = h
                .wrapping_sub(hashes[i - REPLAY_WINDOW].wrapping_mul(power))
                .wrapping_mul(base)
                .wrapping_add(hashes[i]);
            result.push(h);
        }
        result
    }
    let mut lookup = HashMap::<u64, Vec<usize>>::new();
    for (start, digest) in scan(&train_hash, power, base).iter().enumerate() {
        lookup.entry(*digest).or_default().push(start);
    }
    for (start, digest) in scan(&test_hash, power, base).iter().enumerate() {
        if let Some(matches) = lookup.get(digest) {
            for &candidate in matches {
                // FNV and polynomial hash are only an acceleration method,
                // not cryptographic data provenance. Exact row equality is
                // ALWAYS required before a duplicate-contamination verdict.
                if train[candidate..candidate + REPLAY_WINDOW]
                    == holdout[start..start + REPLAY_WINDOW]
                {
                    return true;
                }
            }
        }
    }
    false
}
#[derive(Clone, Debug)]
pub struct Verdict {
    pub train_rows: usize,
    pub holdout_rows: usize,
    pub raw_source: &'static str,
    pub reference_source: &'static str,
    pub reference_present: bool,
}
pub fn inspect(
    train_raw: &Path,
    train_reference: Option<&Path>,
    holdout_raw: &Path,
    holdout_reference: Option<&Path>,
    stick: Stick,
) -> io::Result<Verdict> {
    if train_reference.is_some() != holdout_reference.is_some() {
        return Err(invalid(
            "train/holdout references must be both present or both absent",
        ));
    }
    let mut paths = vec![train_raw, holdout_raw];
    if let Some(path) = train_reference {
        paths.push(path);
    }
    if let Some(path) = holdout_reference {
        paths.push(path);
    }
    identical_paths(&paths)?;
    let train = checked_split(train_raw, train_reference, stick)?;
    let holdout = checked_split(holdout_raw, holdout_reference, stick)?;
    if train.raw_origin != holdout.raw_origin || train.reference_origin != holdout.reference_origin
    {
        return Err(invalid(
            "raw/reference provenance classes differ across splits",
        ));
    }
    if train.rows < 256 || holdout.rows < 256 {
        return Err(invalid("insufficient rows for isolated train/holdout"));
    }
    let training = raw_payload(train_raw)?;
    let test = raw_payload(holdout_raw)?;
    if training.len() != train.rows || test.len() != holdout.rows {
        return Err(invalid("input dataset changed during analysis"));
    }
    if replay_blocks(&training, &test) {
        return Err(invalid(
            "64 or more contiguous sensor/button/trigger frames reused across split",
        ));
    }
    Ok(Verdict {
        train_rows: train.rows,
        holdout_rows: holdout.rows,
        raw_source: train.raw_origin,
        reference_source: train.reference_origin,
        reference_present: train_reference.is_some(),
    })
}
pub fn run(
    train_raw: &Path,
    train_reference: Option<&Path>,
    holdout_raw: &Path,
    holdout_reference: Option<&Path>,
    stick: Stick,
    out: &Path,
) -> io::Result<()> {
    let v = inspect(
        train_raw,
        train_reference,
        holdout_raw,
        holdout_reference,
        stick,
    )?;
    // No row data, user path or untrusted textual input are emitted.
    let report = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v67_offline_csv_train_holdout_separation\",",
            "\"train_rows\":{},\"holdout_rows\":{},\"stick\":\"{}\",",
            "\"raw_provenance_class\":\"{}\",\"reference_provenance_class\":\"{}\",",
            "\"reference_present\":{},\"training_isolation\":\"SEPARATE_FILES_ONLY\",",
            "\"canonical_path_aliasing_denied\":true,",
            "\"matching_64_frame_payload_runs_denied\":true,",
            "\"timestamp_or_packet_relabel_bypass_denied\":true,",
            "\"external_sha256_pinning_required_for_trust\":true,",
            "\"cryptographic_integrity_attested_by_cli\":false,",
            "\"reference_truth_physically_authenticated\":false,",
            "\"physical_device_identity_verified\":false,",
            "\"accuracy_claimed\":false,",
            "\"model_training_performed\":false,\"model_promoted\":false,",
            "\"input_controller_connected\":false,\"driver_writes\":0,",
            "\"game_input_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        v.train_rows,
        v.holdout_rows,
        stick.name(),
        v.raw_source,
        v.reference_source,
        v.reference_present
    );
    crate::truth_lab::save_new(out, &report)?;
    println!(
        "V067_DATASET_SPLIT=PASS train={} holdout={} ref={} overlap_64=DENIED no_physical_truth=TRUE production=BLOCKED",
        v.train_rows,v.holdout_rows,v.reference_present
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    fn folder() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "ga-v67-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    fn write_pair(
        dir: &Path,
        name: &str,
        offset: f64,
        altered: bool,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let mut raw=String::from("sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source\n");
        let mut r = String::from("sample,elapsed_us,intended_x,intended_y,reference_source\n");
        for i in 0..384_usize {
            let intended = if i < 128 {
                0.0
            } else {
                (i as f64 * 0.13).sin() * 0.10
            };
            let x = offset + intended + ((i * 7 % 11) as f64 - 5.0) * 0.00001;
            let extra = if altered { i as u64 * 24 } else { 0 };
            raw.push_str(&format!(
                "{i},{},{},{x:.9},{:.9},0.0,0.0,0,8,9,synthetic_fixture\n",
                8000 * (i as u64 + 1) + extra,
                i / 2,
                -0.10 + offset
            ));
            r.push_str(&format!(
                "{i},{},{intended:.9},0.0,synthetic_fixture\n",
                8000 * (i as u64 + 1) + extra
            ));
        }
        let a = dir.join(format!("{name}-raw.csv"));
        let b = dir.join(format!("{name}-intent.csv"));
        fs::write(&a, raw).unwrap();
        fs::write(&b, r).unwrap();
        (a, b)
    }
    #[test]
    fn independent_synthetic_pair_is_untrusted_but_structurally_accepted() {
        let p = folder();
        let (a, ar) = write_pair(&p, "train", 0.02, false);
        let (b, br) = write_pair(&p, "holdout", 0.035, true);
        let v = inspect(&a, Some(&ar), &b, Some(&br), Stick::Left).unwrap();
        assert_eq!(v.train_rows, 384);
        assert!(v.reference_present);
        let receipt = p.join("proof.json");
        run(&a, Some(&ar), &b, Some(&br), Stick::Left, &receipt).unwrap();
        let bytes = fs::read(&receipt).unwrap();
        assert!(run(&a, Some(&ar), &b, Some(&br), Stick::Left, &receipt).is_err());
        assert_eq!(bytes, fs::read(receipt).unwrap());
        assert!(inspect(&a, None, &b, None, Stick::Left).is_ok());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn aliases_and_retimestamped_duplicate_data_are_blocked() {
        let p = folder();
        let (a, ar) = write_pair(&p, "train", 0.02, false);
        let (b, br) = write_pair(&p, "holdout", 0.02, true);
        // Timestamps and filenames differ, but all axis/control content is replayed.
        assert!(inspect(&a, Some(&ar), &b, Some(&br), Stick::Left).is_err());
        assert!(inspect(&a, None, &a, None, Stick::Left).is_err());
        assert!(inspect(&a, Some(&ar), &br, Some(&ar), Stick::Left).is_err());
        assert!(inspect(&a, Some(&ar), &b, None, Stick::Left).is_err());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn partial_64_frame_leak_across_renumbered_sessions_is_denied() {
        let p = folder();
        let (a, ar) = write_pair(&p, "train", 0.02, false);
        let (b, br) = write_pair(&p, "holdout", 0.035, true);
        let train = fs::read_to_string(&a)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut hold = fs::read_to_string(&b)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for row in 175..239 {
            let from = train[row + 1].split(',').collect::<Vec<_>>();
            let mut into = hold[row + 1]
                .split(',')
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for field in 3..10 {
                into[field] = from[field].to_string();
            }
            hold[row + 1] = into.join(",");
        }
        fs::write(&b, hold.join("\n") + "\n").unwrap();
        assert!(inspect(&a, Some(&ar), &b, Some(&br), Stick::Left).is_err());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn bad_reference_clock_and_nonfinite_axes_fail_before_report() {
        let p = folder();
        let (a, ar) = write_pair(&p, "train", 0.02, false);
        let (b, br) = write_pair(&p, "holdout", 0.03, true);
        let ref_text = fs::read_to_string(&br).unwrap();
        fs::write(
            &br,
            ref_text.replacen("synthetic_fixture", "physically_verified", 1),
        )
        .unwrap();
        assert!(inspect(&a, Some(&ar), &b, Some(&br), Stick::Left).is_err());
        fs::write(&br, ref_text).unwrap();
        let raw_text = fs::read_to_string(&b).unwrap();
        fs::write(&b, raw_text.replacen("0.030", "NaN", 1)).unwrap();
        assert!(inspect(&a, Some(&ar), &b, Some(&br), Stick::Left).is_err());
        assert!(!p.join("unpublished.json").exists());
        fs::remove_dir_all(p).unwrap();
    }
}
