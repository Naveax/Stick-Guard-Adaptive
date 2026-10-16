//! v0.29 bounded, allocation-free *in-memory* controller pipeline.
//! No XInput driver write, virtual controller bridge, or game injection.
//! A device transport remains explicitly unavailable until physical acceptance.
use crate::{
    profile_store::Stick,
    reacquire::VerifiedCenter,
    safety_preview::{PreviewMode, QualityReport, SafePreview},
    Vec2,
};
use std::{io, time::Instant};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub monotonic_us: u64,
    pub packet: u32,
    pub left: Vec2,
    pub right: Vec2,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
}
impl Frame {
    fn finite(self) -> bool {
        self.left.finite()
            && self.right.finite()
            && self.left.x.abs() <= 1.0
            && self.left.y.abs() <= 1.0
            && self.right.x.abs() <= 1.0
            && self.right.y.abs() <= 1.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Preview,
    LatchedFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Disabled,
    PreviewOnly,
    NoConsent,
    BadAxis,
    PacketRollback,
    StaleInput,
    PollDiscontinuity,
    EpochChanged,
    SinkFailure,
}
impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled_raw",
            Self::PreviewOnly => "preview_in_memory",
            Self::NoConsent => "no_consent_raw",
            Self::BadAxis => "bad_axis_fail_closed",
            Self::PacketRollback => "packet_reset_fail_closed",
            Self::StaleInput => "stale_poll_fail_closed",
            Self::PollDiscontinuity => "nonmonotonic_poll_fail_closed",
            Self::EpochChanged => "device_epoch_changed_raw",
            Self::SinkFailure => "sink_failure_fail_closed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Output {
    pub original: Frame,
    pub shadow: Frame,
    pub reason: Reason,
    pub delivered_to_game: bool,
}
/// Deliberately *not* a gamepad writer. The only implementation shipped
/// accepts an in-memory inspection; it cannot create a game device.
pub trait Inspector {
    fn inspect(&mut self, output: Output) -> io::Result<()>;
}
#[derive(Default)]
pub struct MemoryInspector {
    pub observed: u64,
    pub last: Option<Output>,
}
impl Inspector for MemoryInspector {
    fn inspect(&mut self, out: Output) -> io::Result<()> {
        self.observed += 1;
        self.last = Some(out);
        Ok(())
    }
}
pub struct DeniedDriver;
impl DeniedDriver {
    pub fn open() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "No authorized virtual gamepad transport built or hardware-qualified",
        ))
    }
}
/// Fixed bucket log2 latency sketch. No heap, filesystem, or lock on hot path.
/// Numbers represent core CPU wall-clock observations only, NOT end-to-end latency.
#[derive(Clone, Copy)]
pub struct Timing {
    counts: [u64; 64],
    samples: u64,
    total_ns: u128,
    min_ns: u64,
    max_ns: u64,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            counts: [0; 64],
            samples: 0,
            total_ns: 0,
            min_ns: u64::MAX,
            max_ns: 0,
        }
    }
}
impl Timing {
    pub fn observe(&mut self, ns: u64) {
        let bucket = if ns == 0 {
            0
        } else {
            (64 - ns.leading_zeros()) as usize - 1
        };
        self.counts[bucket] += 1;
        self.samples += 1;
        self.total_ns += u128::from(ns);
        self.min_ns = self.min_ns.min(ns);
        self.max_ns = self.max_ns.max(ns);
    }
    /// Conservative upper bucket endpoint; not an exact percentile.
    pub fn bucket_quantile_upper_ns(&self, p: u64) -> u64 {
        if self.samples == 0 {
            return 0;
        }
        let cutoff = self.samples.saturating_mul(p).div_ceil(100).max(1);
        let mut c = 0;
        for (i, v) in self.counts.iter().enumerate() {
            c += *v;
            if c >= cutoff {
                return if i == 63 {
                    u64::MAX
                } else {
                    (1u64 << (i + 1)) - 1
                };
            }
        }
        self.max_ns
    }
    pub fn json(self) -> String {
        format!(
            concat!(
                "{{\"samples\":{},\"min_core_ns\":{},\"max_core_ns\":{},",
                "\"mean_core_ns\":{:.3},\"p50_upper_bucket_ns\":{},",
                "\"p95_upper_bucket_ns\":{},\"p99_upper_bucket_ns\":{},",
                "\"approximate_histogram_percentiles\":true,",
                "\"hardware_to_game_latency_measured\":false}}"
            ),
            self.samples,
            if self.samples > 0 { self.min_ns } else { 0 },
            self.max_ns,
            if self.samples > 0 {
                self.total_ns as f64 / self.samples as f64
            } else {
                0.0
            },
            self.bucket_quantile_upper_ns(50),
            self.bucket_quantile_upper_ns(95),
            self.bucket_quantile_upper_ns(99)
        )
    }
}
pub struct Core {
    state: State,
    reason: Reason,
    profile: Option<SafePreview>,
    last: Option<(u64, u32)>,
    epoch: Option<u64>,
    stick: Stick,
    pub timing: Timing,
}
impl Default for Core {
    fn default() -> Self {
        Self {
            state: State::Off,
            reason: Reason::Disabled,
            profile: None,
            last: None,
            epoch: None,
            stick: Stick::Left,
            timing: Timing::default(),
        }
    }
}
impl Core {
    pub fn state(&self) -> State {
        self.state
    }
    pub fn fault(&self) -> Reason {
        self.reason
    }
    /// Human-acknowledged, quality-checked profile, explicit device session
    /// number. An epoch is NOT an authenticated USB physical device ID.
    pub fn arm(&mut self, center: VerifiedCenter, epoch: u64, consent: bool) -> bool {
        self.arm_stick(center, epoch, consent, Stick::Left)
    }
    pub fn arm_stick(
        &mut self,
        center: VerifiedCenter,
        epoch: u64,
        consent: bool,
        stick: Stick,
    ) -> bool {
        self.emergency_off();
        if !consent {
            self.reason = Reason::NoConsent;
            return false;
        }
        let quality = QualityReport::from_verified(center);
        if quality.mode != PreviewMode::Calibrated {
            self.reason = Reason::NoConsent;
            return false;
        }
        self.profile = Some(SafePreview::from_verified(center));
        self.stick = stick;
        self.epoch = Some(epoch);
        self.state = State::Preview;
        true
    }
    /// Permit a fresh timestamp baseline after an explicitly acknowledged
    /// operator-only neutral audit. Does not arm or replace a calibration.
    pub(crate) fn resume_after_operator_audit(&mut self) {
        if self.state == State::Preview {
            self.last = None;
        }
    }
    pub fn emergency_off(&mut self) {
        self.state = State::Off;
        self.reason = Reason::Disabled;
        self.profile = None;
        self.last = None;
        self.epoch = None;
    }
    pub fn disconnect(&mut self) {
        self.emergency_off();
        self.reason = Reason::EpochChanged;
    }
    fn fail(&mut self, cause: Reason) {
        self.state = State::LatchedFault;
        self.profile = None;
        self.reason = cause;
    }
    pub fn step(&mut self, frame: Frame, epoch: u64) -> Output {
        let tick = Instant::now();
        // Preserve raw packet in original unconditionally. Malformed analog
        // coordinates are never forwarded to a transport.
        let mut shadow = frame;
        let mut cause = self.reason;
        if !frame.finite() {
            self.fail(Reason::BadAxis);
            cause = Reason::BadAxis;
            shadow.left = Vec2::ZERO;
            shadow.right = Vec2::ZERO;
        } else {
            if let Some(bound) = self.epoch {
                if bound != epoch {
                    self.disconnect();
                    cause = Reason::EpochChanged;
                }
            }
            if self.state == State::Preview {
                if let Some((prev_time, prev_packet)) = self.last {
                    if frame.monotonic_us < prev_time {
                        self.fail(Reason::PollDiscontinuity)
                    } else if frame.monotonic_us - prev_time > 50_000 {
                        self.fail(Reason::StaleInput)
                    } else if frame.packet < prev_packet {
                        self.fail(Reason::PacketRollback)
                    }
                }
            }
            if self.state == State::Preview {
                if let Some(profile) = self.profile.as_mut() {
                    // Same-frame pass-through of all non-axis controls.
                    match self.stick {
                        Stick::Left => shadow.left = profile.process(frame.left),
                        Stick::Right => shadow.right = profile.process(frame.right),
                    }
                }
                cause = Reason::PreviewOnly;
            } else if self.state == State::LatchedFault {
                cause = self.reason;
            }
            self.last = Some((frame.monotonic_us, frame.packet));
        }
        self.timing
            .observe(tick.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        Output {
            original: frame,
            shadow,
            reason: cause,
            delivered_to_game: false,
        }
    }
    pub fn observe<I: Inspector>(
        &mut self,
        frame: Frame,
        epoch: u64,
        sink: &mut I,
    ) -> io::Result<Output> {
        let result = self.step(frame, epoch);
        if let Err(error) = sink.inspect(result) {
            self.fail(Reason::SinkFailure);
            return Err(error);
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn verified() -> VerifiedCenter {
        VerifiedCenter::restore_checked(Vec2::new(0.12, -0.04), Vec2::new(0.0001, 0.0001), 0.0002)
            .unwrap()
    }
    fn packet(seq: u32, time: u64) -> Frame {
        Frame {
            packet: seq,
            monotonic_us: time,
            left: Vec2::new(0.13, -0.04),
            right: Vec2::new(-0.9, 0.75),
            buttons: 0xA53C,
            left_trigger: 42,
            right_trigger: 255,
        }
    }
    #[test]
    fn right_stick_preview_preserves_left_stick_and_all_buttons() {
        let mut c = Core::default();
        assert!(c.arm_stick(verified(), 11, true, Stick::Right));
        let f = packet(1, 1000);
        let out = c.step(f, 11);
        assert_eq!(out.reason, Reason::PreviewOnly);
        assert_eq!(out.shadow.left, f.left);
        assert_ne!(out.shadow.right, f.right);
        assert_eq!(out.shadow.buttons, f.buttons);
        assert_eq!(out.shadow.left_trigger, f.left_trigger);
        assert_eq!(out.shadow.right_trigger, f.right_trigger);
        assert!(!out.delivered_to_game);
        let after = c.step(packet(0, 9000), 11);
        assert_eq!(after.reason, Reason::PacketRollback);
        assert_eq!(after.shadow, after.original);
    }
    #[test]
    fn nothing_is_armed_without_explicit_consent() {
        let mut c = Core::default();
        assert!(!c.arm(verified(), 1, false));
        let p = packet(2, 8000);
        let v = c.step(p, 1);
        assert_eq!(v.shadow, p);
        assert!(!v.delivered_to_game);
    }
    #[test]
    fn axis_only_correction_preserves_every_other_control() {
        let mut c = Core::default();
        assert!(c.arm(verified(), 7, true));
        let f = packet(3, 12000);
        let o = c.step(f, 7);
        assert_eq!(o.reason, Reason::PreviewOnly);
        assert!(o.shadow.left.x < 0.011);
        assert_eq!(o.shadow.right, f.right);
        assert_eq!(o.shadow.buttons, f.buttons);
        assert_eq!(o.shadow.left_trigger, f.left_trigger);
        assert_eq!(o.shadow.right_trigger, f.right_trigger);
        assert!(!o.delivered_to_game);
    }
    #[test]
    fn epoch_change_invalidates_active_center() {
        let mut c = Core::default();
        c.arm(verified(), 7, true);
        let _ = c.step(packet(3, 12000), 7);
        let o = c.step(packet(4, 16000), 8);
        assert_eq!(o.reason, Reason::EpochChanged);
        assert_eq!(o.shadow, o.original);
        assert_eq!(c.state(), State::Off);
    }
    #[test]
    fn stale_packet_and_rollback_latch_raw() {
        let mut c = Core::default();
        c.arm(verified(), 7, true);
        c.step(packet(6, 0), 7);
        assert_eq!(c.step(packet(7, 80000), 7).reason, Reason::StaleInput);
        assert_eq!(c.step(packet(8, 88000), 7).shadow, packet(8, 88000));
        assert_eq!(c.state(), State::LatchedFault);
        let mut b = Core::default();
        b.arm(verified(), 7, true);
        b.step(packet(12, 0), 7);
        assert_eq!(b.step(packet(11, 8000), 7).reason, Reason::PacketRollback);
    }
    #[test]
    fn emergency_off_invalidates_profile_and_never_auto_recovers() {
        let mut c = Core::default();
        c.arm(verified(), 7, true);
        c.emergency_off();
        let p = packet(4, 500);
        assert_eq!(c.step(p, 7).shadow, p);
        assert_eq!(c.state(), State::Off);
    }
    struct Broken;
    impl Inspector for Broken {
        fn inspect(&mut self, _: Output) -> io::Result<()> {
            Err(io::Error::other("injected inspector fault"))
        }
    }
    #[test]
    fn inspector_errors_latch_fail_safe() {
        let mut c = Core::default();
        c.arm(verified(), 7, true);
        assert!(c.observe(packet(10, 0), 7, &mut Broken).is_err());
        assert_eq!(c.state(), State::LatchedFault);
        assert_eq!(c.step(packet(11, 8000), 7).shadow, packet(11, 8000));
    }
    #[test]
    fn fixed_memory_timing_quantiles_have_upper_bounds() {
        let mut t = Timing::default();
        for value in [0, 1, 4, 8, 100, 1000, 10000] {
            t.observe(value);
        }
        assert!(t.bucket_quantile_upper_ns(95) >= 10000);
        assert!(t.bucket_quantile_upper_ns(50) >= 8);
        assert!(t
            .json()
            .contains("\"hardware_to_game_latency_measured\":false"));
    }
    #[test]
    fn unsupported_driver_cannot_be_opened() {
        assert_eq!(
            DeniedDriver::open().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
