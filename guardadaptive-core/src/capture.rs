//! Offline-first read-only capture. Streaming, bounded sample count,
//! fail-closed on disconnect/error. Partial recordings are never published
//! under the requested final filename.
use crate::xinput::{self, Report};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static TEMP_NUMBER: AtomicU64 = AtomicU64::new(0);

pub trait Reader {
    fn read(&mut self) -> io::Result<Option<Report>>;
}
pub struct XInputReader {
    pub slot: u32,
}
impl Reader for XInputReader {
    fn read(&mut self) -> io::Result<Option<Report>> {
        xinput::read_slot(self.slot)
    }
}
pub fn validated_limits(count: usize, period_ms: u64) -> io::Result<()> {
    if !(1..=100_000).contains(&count) || !(1..=100).contains(&period_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "sample count must be 1..100000 and period must be 1..100ms",
        ));
    }
    Ok(())
}
fn disconnected() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "controller disconnected / missing; recording aborted with no published CSV",
    )
}
/// Independent from native Win32 calls; tests use a deterministic fake reader.
/// Repeated identical XInput packet numbers are valid samples.
pub fn record<R: Reader>(
    reader: &mut R,
    count: usize,
    period_ms: u64,
    path: &Path,
    sleep: bool,
) -> io::Result<usize> {
    validated_limits(count, period_ms)?;
    // Probe first, BEFORE creating the destination or a temporary file.
    let first = reader.read()?.ok_or_else(disconnected)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing output filename"))?;
    let temp = path.with_file_name(format!(
        ".{}.{}.{}.capture-tmp",
        filename.to_string_lossy(),
        std::process::id(),
        TEMP_NUMBER.fetch_add(1, Ordering::Relaxed)
    ));
    let f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> io::Result<usize> {
        let mut w = io::BufWriter::new(f);
        writeln!(w,"sample,elapsed_us,packet,left_x,left_y,right_x,right_y,buttons,left_trigger,right_trigger,source")?;
        let started = Instant::now();
        for index in 0..count {
            let report = if index == 0 {
                first
            } else {
                reader.read()?.ok_or_else(disconnected)?
            };
            writeln!(
                w,
                "{index},{},{},{:.10},{:.10},{:.10},{:.10},{},{},{},xinput_readonly",
                started.elapsed().as_micros(),
                report.packet,
                report.left.x,
                report.left.y,
                report.right.x,
                report.right.y,
                report.buttons,
                report.left_trigger,
                report.right_trigger
            )?;
            if sleep && index + 1 < count {
                thread::sleep(Duration::from_millis(period_ms));
            }
        }
        w.flush()?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        // Same-directory atomic, non-replacing publish. No TOCTOU gap
        // between checking whether destination exists and moving the file.
        publish_new(&temp, path)?;
        Ok(count)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
#[cfg(windows)]
pub(crate) fn publish_new(temp: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileW(existing: *const u16, new_name: *const u16) -> i32;
    }
    let mut src: Vec<u16> = temp.as_os_str().encode_wide().collect();
    src.push(0);
    let mut dst: Vec<u16> = path.as_os_str().encode_wide().collect();
    dst.push(0);
    // MoveFileW on same volume never replaces an existing destination.
    let ok = unsafe { MoveFileW(src.as_ptr(), dst.as_ptr()) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub(crate) fn publish_new(temp: &Path, path: &Path) -> io::Result<()> {
    // Hard link creation is atomic and fails if destination already exists.
    fs::hard_link(temp, path)?;
    fs::remove_file(temp)
}
pub fn live(slot: u32, count: usize, period_ms: u64, path: &Path) -> io::Result<()> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "XInput slot must be 0..3",
        ));
    }
    let mut reader = XInputReader { slot };
    let captured = record(&mut reader, count, period_ms, path, true)?;
    println!(
        "CAPTURE COMPLETED: slot={slot} samples={captured} CSV={}",
        path.display()
    );
    println!(
        "XInput slot is ephemeral, NOT a stable hardware ID. No profile was created or activated."
    );
    Ok(())
}
pub fn devices() -> io::Result<()> {
    let devices = xinput::enumerate()?;
    println!("XINPUT READ-ONLY DEVICE SCAN: 4 slots (0..3)");
    if devices.is_empty() {
        println!("NO XINPUT CONTROLLER FOUND; this is a valid no-hardware result.");
    }
    for (slot, report) in devices {
        println!(
            "SLOT {slot}: packet={} left=({:+.6},{:+.6}) right=({:+.6},{:+.6}) buttons=0x{:04X}",
            report.packet,
            report.left.x,
            report.left.y,
            report.right.x,
            report.right.y,
            report.buttons
        );
    }
    println!("No device writes, driver installation, virtual controller or rumble occurred.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;
    struct Fake {
        reports: Vec<Report>,
        pos: usize,
    }
    impl Fake {
        fn new(count: usize) -> Self {
            Self {
                reports: (0..count)
                    .map(|i| Report {
                        packet: 7,
                        left: Vec2::new(i as f64 / 10000.0, 0.0),
                        right: Vec2::new(0.0, -0.0001),
                        buttons: 0,
                        left_trigger: 0,
                        right_trigger: 0,
                    })
                    .collect(),
                pos: 0,
            }
        }
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            let next = self.reports.get(self.pos).copied();
            self.pos += 1;
            Ok(next)
        }
    }
    fn path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "guardadaptive-capture-{tag}-{}.csv",
            std::process::id()
        ))
    }
    #[test]
    fn deterministic_reader_preserves_every_sample_including_same_packet() {
        let p = path("complete");
        let _ = fs::remove_file(&p);
        let mut r = Fake::new(128);
        assert_eq!(record(&mut r, 128, 8, &p, false).unwrap(), 128);
        let content = fs::read_to_string(&p).unwrap();
        assert_eq!(content.lines().count(), 129);
        assert!(content.contains("xinput_readonly"));
        assert!(content
            .lines()
            .skip(1)
            .all(|s| s.split(',').nth(2) == Some("7")));
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn no_hardware_never_creates_destination() {
        let p = path("missing");
        let _ = fs::remove_file(&p);
        let mut r = Fake::new(0);
        assert_eq!(
            record(&mut r, 128, 8, &p, false).unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        assert!(!p.exists());
    }
    #[test]
    fn mid_recording_unplug_never_publishes_partial_csv() {
        let p = path("unplug");
        let _ = fs::remove_file(&p);
        let mut r = Fake::new(23);
        assert_eq!(
            record(&mut r, 128, 8, &p, false).unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        assert!(!p.exists());
    }
    #[test]
    fn preexisting_evidence_cannot_be_overwritten() {
        let p = path("existing");
        fs::write(&p, b"PRIOR EVIDENCE").unwrap();
        let mut r = Fake::new(2);
        assert_eq!(
            record(&mut r, 2, 8, &p, false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&p).unwrap(), b"PRIOR EVIDENCE");
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn concurrent_captures_to_same_filename_never_replace_success() {
        let p = path("concurrent");
        let _ = fs::remove_file(&p);
        let a = p.clone();
        let b = p.clone();
        let left = std::thread::spawn(move || {
            let mut reader = Fake::new(3);
            record(&mut reader, 3, 8, &a, false)
        });
        let right = std::thread::spawn(move || {
            let mut reader = Fake::new(3);
            record(&mut reader, 3, 8, &b, false)
        });
        let x = left.join().unwrap();
        let y = right.join().unwrap();
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1, "x={x:?} y={y:?}");
        assert!(fs::read_to_string(&p).unwrap().lines().count() == 4);
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn resource_bounds_are_enforced() {
        for (n, p) in [(0, 8), (100001, 8), (100, 0), (100, 101)] {
            assert_eq!(
                validated_limits(n, p).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(validated_limits(100000, 100).is_ok());
    }
}
