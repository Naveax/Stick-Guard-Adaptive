//! XInput -> checked user profile -> MotionFirst -> read-only text preview.
//! No injection, rumble, driver, virtual device, or game interception.
use crate::{
    capture::{validated_limits, Reader, XInputReader},
    motion_first::MotionFirst,
    profile_store::{self, Identity, Stick},
    resolution::Resolution,
    safety_preview::SafePreview,
    xinput::Report,
    InputMode, Vec2,
};
use std::{
    io::{self, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn absent() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "XInput controller missing/disconnected; preview stopped",
    )
}
/// Pure unprivileged live transform for unit tests with deterministic reports.
pub fn map_report(engine: &mut MotionFirst, report: Report, stick: Stick) -> (Vec2, Vec2) {
    let raw = report.stick(stick);
    (raw, engine.process(raw, InputMode::Live))
}
pub fn run_stream<R: Reader, W: Write>(
    reader: &mut R,
    profile: &Path,
    identity: &Identity,
    count: usize,
    period_ms: u64,
    output: &mut W,
    sleep: bool,
) -> io::Result<()> {
    validated_limits(count, period_ms)?;
    if identity.resolution != Resolution::XInput16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native XInput reports must use an xinput16-matched profile",
        ));
    }
    // Never auto-activate an unverified or mismatched profile.
    let verified = profile_store::load(profile, identity)?;
    let mut engine = SafePreview::from_verified(verified);
    eprintln!(
        "PREVIEW SAFETY: mode={} neutral_mad_max={:.8} release_agreement={:.8}; read-only",
        engine.quality().mode.name(),
        engine.quality().max_axis_mad,
        engine.quality().release_disagreement
    );
    let first = reader.read()?.ok_or_else(absent)?;
    writeln!(
        output,
        "sample,elapsed_us,packet,raw_x,raw_y,corrected_x,corrected_y"
    )?;
    let started = Instant::now();
    for index in 0..count {
        let frame = if index == 0 {
            first
        } else {
            reader.read()?.ok_or_else(absent)?
        };
        let raw = frame.stick(identity.stick);
        let corrected = engine.process(raw);
        writeln!(
            output,
            "{index},{},{},{:.8},{:.8},{:.8},{:.8}",
            started.elapsed().as_micros(),
            frame.packet,
            raw.x,
            raw.y,
            corrected.x,
            corrected.y
        )?;
        if sleep && index + 1 < count {
            thread::sleep(Duration::from_millis(period_ms));
        }
    }
    output.flush()?;
    Ok(())
}
pub fn native(
    slot: u32,
    profile: &Path,
    identity: &Identity,
    count: usize,
    period_ms: u64,
) -> io::Result<()> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "slot must be 0..3",
        ));
    }
    let mut reader = XInputReader { slot };
    println!(
        "READ ONLY CORRECTION PREVIEW: XInput slot={slot} stick={} alias={}",
        identity.stick.name(),
        identity.device_id
    );
    println!("Alias is operator-supplied; XInput slot != stable hardware identity.");
    println!("No virtual gamepad output or in-game replacement occurs.");
    let stdout = io::stdout();
    let mut w = io::BufWriter::new(stdout.lock());
    run_stream(
        &mut reader,
        profile,
        identity,
        count,
        period_ms,
        &mut w,
        true,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{guided::GuidedWizard, xinput::Report};
    use std::fs;
    struct Fake {
        current: usize,
        limit: usize,
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            if self.current >= self.limit {
                return Ok(None);
            }
            self.current += 1;
            Ok(Some(Report {
                packet: 1,
                left: Vec2::new(0.45 + 0.002, -0.15),
                right: Vec2::ZERO,
                buttons: 0,
                left_trigger: 0,
                right_trigger: 0,
            }))
        }
    }
    fn profile_path() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "guardadaptive-preview-{}-{seq}.profile",
            std::process::id()
        ))
    }
    fn prepared() -> (std::path::PathBuf, Identity) {
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(Vec2::new(0.45, -0.15)).unwrap();
        }
        w.sample(Vec2::new(0.2, -0.15)).unwrap();
        w.confirm_movement_finished().unwrap();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(Vec2::new(0.45, -0.15)).unwrap();
        }
        let v = w.commit().unwrap();
        let path = profile_path();
        let _ = fs::remove_file(&path);
        let id = Identity::new("Preview-Device", Stick::Left, Resolution::XInput16).unwrap();
        profile_store::save(&path, &id, &v).unwrap();
        (path, id)
    }
    #[test]
    fn synthetic_input_profile_preview_preserves_small_movement() {
        let (path, id) = prepared();
        let mut reader = Fake {
            current: 0,
            limit: 16,
        };
        let mut buf = Vec::new();
        run_stream(&mut reader, &path, &id, 16, 8, &mut buf, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s.lines().count(), 17);
        let corrected = s
            .lines()
            .nth(1)
            .unwrap()
            .split(',')
            .nth(5)
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(corrected > 0.0019 && corrected <= 0.002);
        let _ = fs::remove_file(&path);
    }
    #[test]
    fn stored_but_uncertain_profile_streams_exact_raw_xinput_without_rewriting() {
        let profile = profile_path();
        let id = Identity::new("Uncertain-Device", Stick::Left, Resolution::XInput16).unwrap();
        let verified = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.45, -0.15),
            Vec2::new(0.006, 0.003),
            0.001,
        )
        .unwrap();
        let _ = fs::remove_file(&profile);
        profile_store::save(&profile, &id, &verified).unwrap();
        let mut reader = Fake {
            current: 0,
            limit: 8,
        };
        let mut output = Vec::new();
        run_stream(&mut reader, &profile, &id, 8, 8, &mut output, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Preserve the CSV contract: first line must be the column names.
        assert!(text.starts_with("sample,elapsed_us,packet,raw_x,raw_y,corrected_x,corrected_y\n"));
        assert_eq!(text.lines().count(), 9);
        for row in text.lines().skip(1) {
            let cols: Vec<_> = row.split(',').collect();
            assert_eq!(cols[3], cols[5], "uncertain center must not be subtracted");
            assert_eq!(cols[4], cols[6]);
        }
        let _ = fs::remove_file(&profile);
    }
    #[test]
    fn wrong_identity_rejected_before_reader_is_used() {
        let (path, _) = prepared();
        let id = Identity::new("OTHER-Device", Stick::Left, Resolution::XInput16).unwrap();
        let mut reader = Fake {
            current: 0,
            limit: 16,
        };
        let mut buf = Vec::new();
        assert_eq!(
            run_stream(&mut reader, &path, &id, 8, 8, &mut buf, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(reader.current, 0);
        let _ = fs::remove_file(&path);
    }
    #[test]
    fn disconnection_stops_preview() {
        let (path, id) = prepared();
        let mut reader = Fake {
            current: 0,
            limit: 3,
        };
        let mut buf = Vec::new();
        assert_eq!(
            run_stream(&mut reader, &path, &id, 16, 8, &mut buf, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotConnected
        );
        let _ = fs::remove_file(&path);
    }
}
