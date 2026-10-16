//! v0.30 read-only XInput stream continuity and core timing instrumentation.
//! Never auto-arm calibrations. XInput slot is NOT a stable physical identity.
use crate::{
    capture::{self, Reader, XInputReader},
    runtime_core::{Core, Frame, MemoryInspector, Reason, Timing},
    xinput::Report,
};
use std::{
    io,
    path::Path,
    thread,
    time::{Duration, Instant},
};
const MAX_SAMPLES: usize = 100000;
const MAX_GAP_US: u64 = 50_000;
#[derive(Default)]
pub struct MonitorReport {
    pub total: usize,
    pub repeated_packet: usize,
    pub changed_packet: usize,
    pub longest_gap_us: u64,
    pub packet_reset_count: usize,
    pub stale_gap_count: usize,
    pub first_packet: u32,
    pub last_packet: u32,
    pub original_buttons_changed: usize,
    pub core: Timing,
}
impl MonitorReport {
    pub fn json(&self, slot: u32, period_ms: u64) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"readonly_xinput_poll_monitor\",",
                "\"slot\":{},\"requested_period_ms\":{},\"samples\":{},",
                "\"packet_first\":{},\"packet_last\":{},\"repeated_packets\":{},",
                "\"changed_packets\":{},\"largest_poll_gap_us\":{},",
                "\"packet_resets\":{},\"over_50ms_poll_gaps\":{},",
                "\"button_transition_count\":{},\"core_timing\":{},",
                "\"physical_identity_verified\":false,",
                "\"physical_stick_release_proven\":false,",
                "\"calibration_enabled\":false,\"game_output_enabled\":false,",
                "\"core_only_not_input_to_game_latency\":true,",
                "\"production_qualified\":false}}\n"
            ),
            slot,
            period_ms,
            self.total,
            self.first_packet,
            self.last_packet,
            self.repeated_packet,
            self.changed_packet,
            self.longest_gap_us,
            self.packet_reset_count,
            self.stale_gap_count,
            self.original_buttons_changed,
            self.core.json()
        )
    }
}
fn not_connected() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "no controller, or controller disconnected; no output published",
    )
}
fn to_frame(report: Report, elapsed_us: u64) -> Frame {
    Frame {
        packet: report.packet,
        monotonic_us: elapsed_us,
        left: report.left,
        right: report.right,
        buttons: report.buttons,
        left_trigger: report.left_trigger,
        right_trigger: report.right_trigger,
    }
}
/// The complete sequence must be connected and well-formed. A brief disconnect
/// fails closed and does not create/publish a partial valid report.
pub fn measure<R: Reader>(
    reader: &mut R,
    frames: usize,
    period_ms: u64,
    sleep: bool,
) -> io::Result<MonitorReport> {
    if !(1..=MAX_SAMPLES).contains(&frames) || !(1..=100).contains(&period_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "samples 1..100000 and period 1..100ms required",
        ));
    }
    let initial = reader.read()?.ok_or_else(not_connected)?;
    let mut last = None::<(u64, u32, u16)>;
    let mut report = MonitorReport {
        first_packet: initial.packet,
        ..MonitorReport::default()
    };
    let mut core = Core::default();
    let mut inspector = MemoryInspector::default();
    let start = Instant::now();
    for index in 0..frames {
        let raw = if index == 0 {
            initial
        } else {
            reader.read()?.ok_or_else(not_connected)?
        };
        let elapsed = start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        if let Some((time, packet, buttons)) = last {
            let gap = elapsed.saturating_sub(time);
            report.longest_gap_us = report.longest_gap_us.max(gap);
            if gap > MAX_GAP_US {
                report.stale_gap_count += 1;
            }
            if raw.packet < packet {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "XInput packet counter decreased; possible reset/device swap",
                ));
            }
            if raw.packet == packet {
                report.repeated_packet += 1;
            } else {
                report.changed_packet += 1;
            }
            if raw.buttons != buttons {
                report.original_buttons_changed += 1;
            }
        }
        let frame = to_frame(raw, elapsed);
        let out = core.observe(frame, 0, &mut inspector)?;
        if out.shadow != out.original || out.delivered_to_game || out.reason != Reason::Disabled {
            return Err(io::Error::other(
                "read-only monitor unexpectedly modified or delivered a controller frame",
            ));
        }
        report.total += 1;
        report.last_packet = raw.packet;
        last = Some((elapsed, raw.packet, raw.buttons));
        if sleep && index + 1 < frames {
            thread::sleep(Duration::from_millis(period_ms));
        }
    }
    report.core = core.timing;
    Ok(report)
}
pub fn run(slot: u32, frames: usize, period_ms: u64, out: &Path) -> io::Result<()> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "XInput slot must be 0..3",
        ));
    }
    if out.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target report exists",
        ));
    }
    let mut reader = XInputReader { slot };
    let observation = measure(&mut reader, frames, period_ms, true)?;
    capture::validated_limits(frames, period_ms)?;
    crate::truth_lab::save_new(out, &observation.json(slot, period_ms))?;
    println!(
        "XINPUT READ ONLY MONITOR slot={slot} samples={} longest_poll_gap={}us repeats={}",
        observation.total, observation.longest_gap_us, observation.repeated_packet
    );
    println!("DEVICE_ID=UNVERIFIED GAME_LATENCY=UNVERIFIED DRIVER_WRITES=0 CALIBRATION=OFF PRODUCTION=BLOCKED");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec2;
    struct Fake {
        sample: usize,
        fail_at: usize,
        reset_at: usize,
    }
    impl Reader for Fake {
        fn read(&mut self) -> io::Result<Option<Report>> {
            let i = self.sample;
            self.sample += 1;
            if i == self.fail_at {
                return Ok(None);
            }
            Ok(Some(Report {
                packet: if i >= self.reset_at {
                    1
                } else {
                    (i / 2) as u32
                },
                left: Vec2::new(0.15, -0.05),
                right: Vec2::new(-0.1, 0.3),
                buttons: (i / 9) as u16,
                left_trigger: 3,
                right_trigger: 200,
            }))
        }
    }
    #[test]
    fn fake_reader_permits_packet_repeats_and_preserves_raw() {
        let mut fake = Fake {
            sample: 0,
            fail_at: usize::MAX,
            reset_at: usize::MAX,
        };
        let m = measure(&mut fake, 256, 8, false).unwrap();
        assert_eq!(m.total, 256);
        assert_eq!(m.repeated_packet, 128);
        assert_eq!(m.changed_packet, 127);
        assert!(m.original_buttons_changed > 0);
        assert!(m.json(0, 8).contains("\"game_output_enabled\":false"));
    }
    #[test]
    fn missing_and_midstream_disconnect_fail_without_claiming_data() {
        for fail in [0, 11, 255] {
            let mut fake = Fake {
                sample: 0,
                fail_at: fail,
                reset_at: usize::MAX,
            };
            assert_eq!(
                measure(&mut fake, 256, 8, false).err().unwrap().kind(),
                io::ErrorKind::NotConnected
            );
        }
    }
    #[test]
    fn packet_reset_must_never_be_accepted_as_new_hardware_identity() {
        let mut fake = Fake {
            sample: 0,
            fail_at: usize::MAX,
            reset_at: 40,
        };
        assert_eq!(
            measure(&mut fake, 200, 8, false).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn invalid_limits_rejected_before_hardware_poll() {
        let mut fake = Fake {
            sample: 0,
            fail_at: usize::MAX,
            reset_at: usize::MAX,
        };
        assert!(measure(&mut fake, 0, 8, false).is_err());
        assert!(measure(&mut fake, 100001, 8, false).is_err());
        assert!(measure(&mut fake, 128, 0, false).is_err());
        assert_eq!(fake.sample, 0);
    }
}
