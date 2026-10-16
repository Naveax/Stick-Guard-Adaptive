//! XInput read-only timing observations. Never interpret packet numbers as
//! physical frame clocks or claim timing here equals input-to-game latency.
//! No device identification, calibration learning, or controller writes.
use crate::{
    capture::{self, Reader, XInputReader},
    xinput::Report,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Audit {
    pub samples: usize,
    pub requested_period_ms: u64,
    pub elapsed_us: u64,
    pub repeated_packets: u64,
    pub changed_packets: u64,
    pub nonmonotonic_packets: u64,
    pub min_interval_us: u64,
    pub p95_interval_us: u64,
    pub max_interval_us: u64,
    pub avg_interval_us: f64,
    pub max_poll_duration_us: u64,
    pub left_min: [f64; 2],
    pub left_max: [f64; 2],
    pub right_min: [f64; 2],
    pub right_max: [f64; 2],
}
fn disconnected() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "XInput device missing or disconnected: NO diagnostic file published",
    )
}
fn invalid_report(r: Report) -> bool {
    !(r.left.finite() && r.right.finite())
        || r.left.x.abs() > 1.0
        || r.left.y.abs() > 1.0
        || r.right.x.abs() > 1.0
        || r.right.y.abs() > 1.0
}
/// Read-only acquisition is explicitly observational. The observer may be
/// called with a fake reader to test its temporal, abort, and integrity rules.
pub fn observe<R: Reader>(
    reader: &mut R,
    samples: usize,
    period_ms: u64,
    sleep: bool,
) -> io::Result<Audit> {
    capture::validated_limits(samples, period_ms)?;
    if samples < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "diagnose requires at least 2 samples to measure intervals",
        ));
    }
    let mut intervals = Vec::with_capacity(samples - 1);
    let mut previous: Option<(Instant, u32)> = None;
    let started = Instant::now();
    let mut repeat = 0_u64;
    let mut change = 0_u64;
    let mut backward = 0_u64;
    let mut left_min = [f64::INFINITY; 2];
    let mut left_max = [f64::NEG_INFINITY; 2];
    let mut right_min = [f64::INFINITY; 2];
    let mut right_max = [f64::NEG_INFINITY; 2];
    let mut max_read_us = 0;
    for i in 0..samples {
        let before = Instant::now();
        let raw = reader.read()?.ok_or_else(disconnected)?;
        let timestamp = Instant::now();
        max_read_us = max_read_us.max(as_us(timestamp.duration_since(before)));
        if invalid_report(raw) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "nonfinite or outside normalized XInput axis range",
            ));
        }
        if let Some((earlier, packet)) = previous {
            intervals.push(as_us(timestamp.duration_since(earlier)));
            if raw.packet == packet {
                repeat += 1
            } else {
                change += 1;
                if raw.packet < packet {
                    backward += 1;
                }
            }
        }
        previous = Some((timestamp, raw.packet));
        for (axis, val) in [raw.left.x, raw.left.y].into_iter().enumerate() {
            left_min[axis] = left_min[axis].min(val);
            left_max[axis] = left_max[axis].max(val);
        }
        for (axis, val) in [raw.right.x, raw.right.y].into_iter().enumerate() {
            right_min[axis] = right_min[axis].min(val);
            right_max[axis] = right_max[axis].max(val);
        }
        if sleep && i + 1 < samples {
            thread::sleep(Duration::from_millis(period_ms));
        }
    }
    intervals.sort_unstable();
    let n = intervals.len();
    let sum = intervals.iter().map(|v| *v as u128).sum::<u128>();
    Ok(Audit {
        samples,
        requested_period_ms: period_ms,
        elapsed_us: as_us(started.elapsed()),
        repeated_packets: repeat,
        changed_packets: change,
        nonmonotonic_packets: backward,
        min_interval_us: intervals[0],
        p95_interval_us: intervals[((n * 95).div_ceil(100)).saturating_sub(1)],
        max_interval_us: intervals[n - 1],
        avg_interval_us: sum as f64 / n as f64,
        max_poll_duration_us: max_read_us,
        left_min,
        left_max,
        right_min,
        right_max,
    })
}
fn as_us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}
impl Audit {
    pub fn json(&self, slot: u32) -> String {
        // Numeric-only JSON. Literal booleans explicitly block interpreting
        // polling timing as a controller identity or gameplay latency.
        format!(
            concat!(
                "{{\n  \"schema\": 1,\n  \"source\": \"XInput1_4_readonly_observation\",",
                "\n  \"slot\": {},\n  \"samples\": {},\n  \"requested_period_ms\": {},",
                "\n  \"elapsed_us\": {},\n  \"repeated_packets\": {},",
                "\n  \"changed_packets\": {},\n  \"nonmonotonic_packets\": {},",
                "\n  \"interval_min_us\": {},\n  \"interval_p95_us\": {},",
                "\n  \"interval_max_us\": {},\n  \"interval_avg_us\": {:.3},",
                "\n  \"max_read_call_us\": {},",
                "\n  \"left_min\": [{:.8}, {:.8}],\n  \"left_max\": [{:.8}, {:.8}],",
                "\n  \"right_min\": [{:.8}, {:.8}],\n  \"right_max\": [{:.8}, {:.8}],",
                "\n  \"hardware_identity_verified\": false,",
                "\n  \"neutral_release_confirmed\": false,",
                "\n  \"input_to_game_latency_measured\": false,",
                "\n  \"calibration_was_modified\": false,",
                "\n  \"production_accepted\": false\n}}\n"
            ),
            slot,
            self.samples,
            self.requested_period_ms,
            self.elapsed_us,
            self.repeated_packets,
            self.changed_packets,
            self.nonmonotonic_packets,
            self.min_interval_us,
            self.p95_interval_us,
            self.max_interval_us,
            self.avg_interval_us,
            self.max_poll_duration_us,
            self.left_min[0],
            self.left_min[1],
            self.left_max[0],
            self.left_max[1],
            self.right_min[0],
            self.right_min[1],
            self.right_max[0],
            self.right_max[1],
        )
    }
}
/// Write diagnostics atomically and without replacing any prior file. A
/// disconnected controller fails before even creating the final parent dir.
pub fn save(out: &Path, slot: u32, a: &Audit) -> io::Result<()> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "slot must be 0..3",
        ));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let stem = out
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing filename"))?;
    let temp = out.with_file_name(format!(
        ".{}.{}.{}.diagnostic-tmp",
        stem.to_string_lossy(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created_by_us = false;
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        created_by_us = true;
        file.write_all(a.json(slot).as_bytes())?;
        file.sync_all()?;
        drop(file);
        capture::publish_new(&temp, out)
    })();
    if result.is_err() && created_by_us {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn native(slot: u32, samples: usize, period_ms: u64, out: &Path) -> io::Result<()> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "slot must be 0..3",
        ));
    }
    let mut reader = XInputReader { slot };
    let summary = observe(&mut reader, samples, period_ms, true)?;
    save(out, slot, &summary)?;
    println!(
        "READ-ONLY XINPUT DIAGNOSTIC: {} ({} reports)",
        out.display(),
        samples
    );
    println!(
        "INTERVAL: min={}us p95={}us max={}us; max read call={}us",
        summary.min_interval_us,
        summary.p95_interval_us,
        summary.max_interval_us,
        summary.max_poll_duration_us
    );
    println!("NOT VERIFIED: hardware identity, neutral consent, gameplay latency, production acceptance.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;
    struct Fake {
        frames: Vec<Option<Report>>,
        index: usize,
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            let r = self.frames.get(self.index).copied().flatten();
            self.index += 1;
            Ok(r)
        }
    }
    fn frame(packet: u32, value: f64) -> Option<Report> {
        Some(Report {
            packet,
            left: Vec2::new(value, -value),
            right: Vec2::new(0.1, 0.2),
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        })
    }
    fn path(stem: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "guardadaptive-v17-{}-{}.json",
            stem,
            std::process::id()
        ))
    }
    #[test]
    fn identical_packet_is_not_a_missing_sample() {
        let mut fake = Fake {
            frames: vec![frame(3, 0.1), frame(3, 0.15), frame(4, 0.2), frame(2, 0.25)],
            index: 0,
        };
        let a = observe(&mut fake, 4, 8, false).unwrap();
        assert_eq!(a.repeated_packets, 1);
        assert_eq!(a.changed_packets, 2);
        assert_eq!(a.nonmonotonic_packets, 1);
        assert_eq!(a.samples, 4);
        assert_eq!(a.left_min, [0.1, -0.25]);
        assert_eq!(a.left_max, [0.25, -0.1]);
        let j = a.json(0);
        assert!(j.contains("\"hardware_identity_verified\": false"));
        assert!(j.contains("\"input_to_game_latency_measured\": false"));
        assert!(j.contains("\"production_accepted\": false"));
    }
    #[test]
    fn disconnection_does_not_publish_file() {
        let p = path("missing");
        let _ = fs::remove_file(&p);
        let mut fake = Fake {
            frames: vec![frame(1, 0.1), None, frame(3, 0.2)],
            index: 0,
        };
        let error = observe(&mut fake, 3, 8, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        assert!(!p.exists());
    }
    #[test]
    fn invalid_reports_are_rejected() {
        let mut fake = Fake {
            frames: vec![frame(1, f64::NAN), frame(2, 0.1)],
            index: 0,
        };
        assert_eq!(
            observe(&mut fake, 2, 8, false).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn report_commit_is_nonreplacing_and_valid_json_shape() {
        let p = path("nonreplace");
        let _ = fs::remove_file(&p);
        let mut fake = Fake {
            frames: vec![frame(1, 0.1), frame(2, 0.2)],
            index: 0,
        };
        let report = observe(&mut fake, 2, 8, false).unwrap();
        save(&p, 0, &report).unwrap();
        let first = fs::read(&p).unwrap();
        assert_eq!(first, report.json(0).as_bytes());
        assert!(save(&p, 0, &report).is_err());
        assert_eq!(fs::read(&p).unwrap(), first);
        let _ = fs::remove_file(p);
    }
    #[test]
    fn limits_are_checked_before_device_access() {
        let mut fake = Fake {
            frames: Vec::new(),
            index: 0,
        };
        assert_eq!(
            observe(&mut fake, 1, 8, false).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            observe(&mut fake, 2, 0, false).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fake.index, 0);
    }
}
