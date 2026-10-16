//! v0.35: connect the explicitly approved v0.34 calibration transaction
//! to the v0.29 in-memory Frame/Core engine. Optional neutral audit is
//! operator-declared, NEVER a blind inference that a stick was released.
//! No HID access, driver opening, auto-center updates or game writes.
use crate::{
    guided::Screen,
    operator_recenter::Recenter,
    profile_store::Stick,
    reacquire::VerifiedCenter,
    runtime_core::{Core, Frame, Reason, State as CoreState},
    Vec2,
};
use std::{io, path::Path};
pub(crate) const AUDIT_SAMPLES: usize = 128;
pub(crate) const MIN_CENTER_SHIFT: f64 = 0.012;
pub(crate) const MAX_AUDIT_MAD: f64 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Off,
    Calibrating,
    Preview,
    Auditing,
    Warning,
    Fault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alert {
    None,
    OperatorReleaseRequired,
    SignificantNeutralDisagreement,
    UnstableNeutralWindow,
    InvalidSample,
    StreamDiscontinuity,
    DeviceSessionChanged,
}
impl Alert {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OperatorReleaseRequired => "operator_release_not_confirmed",
            Self::SignificantNeutralDisagreement => {
                "operator_declared_neutral_shift_recalibration_recommended"
            }
            Self::UnstableNeutralWindow => {
                "operator_declared_neutral_unstable_recalibration_recommended"
            }
            Self::InvalidSample => "invalid_sample_fail_closed",
            Self::StreamDiscontinuity => "packet_or_clock_discontinuity",
            Self::DeviceSessionChanged => "device_session_changed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observed {
    pub raw: Frame,
    pub shadow: Frame,
    pub phase: Phase,
    pub alert: Alert,
    pub delivered_to_game: bool,
}
pub(crate) struct Audit {
    samples: [Vec2; AUDIT_SAMPLES],
    count: usize,
}
impl Audit {
    pub(crate) fn new() -> Self {
        Self {
            samples: [Vec2::ZERO; AUDIT_SAMPLES],
            count: 0,
        }
    }
    pub(crate) fn count(&self) -> usize {
        self.count
    }
    pub(crate) fn push(&mut self, s: Vec2) -> Option<(Vec2, Vec2)> {
        self.samples[self.count] = s;
        self.count += 1;
        if self.count == AUDIT_SAMPLES {
            let mut xs: [f64; AUDIT_SAMPLES] = std::array::from_fn(|i| self.samples[i].x);
            let mut ys: [f64; AUDIT_SAMPLES] = std::array::from_fn(|i| self.samples[i].y);
            xs.sort_by(f64::total_cmp);
            ys.sort_by(f64::total_cmp);
            let mid = Vec2::new((xs[63] + xs[64]) * 0.5, (ys[63] + ys[64]) * 0.5);
            let mut dx: [f64; AUDIT_SAMPLES] =
                std::array::from_fn(|i| (self.samples[i].x - mid.x).abs());
            let mut dy: [f64; AUDIT_SAMPLES] =
                std::array::from_fn(|i| (self.samples[i].y - mid.y).abs());
            dx.sort_by(f64::total_cmp);
            dy.sort_by(f64::total_cmp);
            let mad = Vec2::new(
                (dx[63] + dx[64]) * 0.5 * 1.4826,
                (dy[63] + dy[64]) * 0.5 * 1.4826,
            );
            Some((mid, mad))
        } else {
            None
        }
    }
}
pub struct Bridge {
    transaction: Recenter,
    core: Core,
    phase: Phase,
    alert: Alert,
    stick: Stick,
    epoch: Option<u64>,
    last: Option<(u64, u32)>,
    center: Option<Vec2>,
    audit: Option<Audit>,
    pub seen: u64,
    pub preview_frames: u64,
    pub audit_frames: u64,
    pub raw_frames: u64,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            transaction: Recenter::default(),
            core: Core::default(),
            phase: Phase::Off,
            alert: Alert::None,
            stick: Stick::Left,
            epoch: None,
            last: None,
            center: None,
            audit: None,
            seen: 0,
            preview_frames: 0,
            audit_frames: 0,
            raw_frames: 0,
        }
    }
}
impl Bridge {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn alert(&self) -> Alert {
        self.alert
    }
    pub fn screen(&self) -> Option<Screen> {
        self.transaction.screen()
    }
    pub fn center(&self) -> Option<Vec2> {
        self.center
    }
    fn revoke(&mut self, alert: Alert, phase: Phase) {
        self.core.emergency_off();
        self.transaction.disconnect();
        self.audit = None;
        self.center = None;
        self.epoch = None;
        self.phase = phase;
        self.alert = alert;
        self.last = None;
    }
    pub fn emergency_off(&mut self) {
        self.revoke(Alert::None, Phase::Off)
    }
    pub fn disconnect(&mut self) {
        self.revoke(Alert::DeviceSessionChanged, Phase::Fault)
    }
    pub fn begin(&mut self, epoch: u64, stick: Stick, operator_consent: bool) -> bool {
        self.emergency_off();
        if !operator_consent {
            self.alert = Alert::OperatorReleaseRequired;
            return false;
        }
        self.stick = stick;
        self.epoch = Some(epoch);
        if !self.transaction.begin(epoch, true) {
            return false;
        }
        self.phase = Phase::Calibrating;
        true
    }
    pub fn confirm_release(&mut self) -> io::Result<()> {
        if self.phase != Phase::Calibrating {
            return Err(io::Error::other("not calibrating"));
        }
        self.transaction.confirm_release()
    }
    pub fn confirm_movement(&mut self) -> io::Result<()> {
        if self.phase != Phase::Calibrating {
            return Err(io::Error::other("not calibrating"));
        }
        self.transaction.confirm_movement()
    }
    pub fn finish(&mut self, explicit_commit: bool) -> io::Result<()> {
        if self.phase != Phase::Calibrating {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "calibration not active",
            ));
        }
        if !explicit_commit {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "operator must commit",
            ));
        }
        let trusted = match self.transaction.finish(true) {
            Ok(value) => value,
            Err(e) => {
                self.revoke(Alert::UnstableNeutralWindow, Phase::Fault);
                return Err(e);
            }
        };
        if !self.core.arm_stick(
            trusted,
            self.epoch.expect("active calibration epoch"),
            true,
            self.stick,
        ) {
            self.revoke(Alert::UnstableNeutralWindow, Phase::Fault);
            return Err(io::Error::other("calibration not quality-approved"));
        }
        self.center = Some(trusted.center());
        self.phase = Phase::Preview;
        self.last = None;
        Ok(())
    }
    /// Requires the human to explicitly say the stick is released.
    /// Sampling is NOT driven by low apparent velocity or position.
    /// While auditing every frame remains raw/pass-through in memory.
    pub fn begin_operator_neutral_audit(&mut self, confirmed_released: bool) -> bool {
        if self.phase != Phase::Preview || !confirmed_released {
            self.alert = Alert::OperatorReleaseRequired;
            return false;
        }
        self.audit = Some(Audit::new());
        self.phase = Phase::Auditing;
        self.alert = Alert::None;
        self.last = None;
        true
    }
    fn chosen(&self, f: Frame) -> Vec2 {
        match self.stick {
            Stick::Left => f.left,
            Stick::Right => f.right,
        }
    }
    fn valid(f: Frame) -> bool {
        [f.left.x, f.left.y, f.right.x, f.right.y]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0)
    }
    fn raw_output(&mut self, frame: Frame) -> Observed {
        self.raw_frames += 1;
        Observed {
            raw: frame,
            shadow: frame,
            phase: self.phase,
            alert: self.alert,
            delivered_to_game: false,
        }
    }
    pub fn observe(&mut self, frame: Frame, epoch: u64) -> Observed {
        self.seen += 1;
        if !Self::valid(frame) {
            self.revoke(Alert::InvalidSample, Phase::Fault);
            let mut safe = frame;
            safe.left = Vec2::ZERO;
            safe.right = Vec2::ZERO;
            self.raw_frames += 1;
            return Observed {
                raw: frame,
                shadow: safe,
                phase: self.phase,
                alert: self.alert,
                delivered_to_game: false,
            };
        }
        if matches!(
            self.phase,
            Phase::Calibrating | Phase::Preview | Phase::Auditing
        ) {
            if self.epoch != Some(epoch) {
                self.revoke(Alert::DeviceSessionChanged, Phase::Fault);
                return self.raw_output(frame);
            }
            if let Some((prev_time, prev_packet)) = self.last {
                if frame.monotonic_us < prev_time
                    || frame.packet < prev_packet
                    || frame.monotonic_us - prev_time > 50_000
                {
                    self.revoke(Alert::StreamDiscontinuity, Phase::Fault);
                    return self.raw_output(frame);
                }
            }
            self.last = Some((frame.monotonic_us, frame.packet));
        }
        match self.phase {
            Phase::Calibrating => {
                if matches!(
                    self.screen(),
                    Some(Screen::SampleFirst | Screen::AskMovement | Screen::SampleSecond)
                ) && self
                    .transaction
                    .sample_calibration(self.chosen(frame), epoch)
                    .is_err()
                {
                    self.revoke(Alert::UnstableNeutralWindow, Phase::Fault);
                }
                self.raw_output(frame)
            }
            Phase::Preview => {
                let out = self.core.step(frame, epoch);
                if out.reason != Reason::PreviewOnly || self.core.state() != CoreState::Preview {
                    self.revoke(Alert::StreamDiscontinuity, Phase::Fault);
                    return self.raw_output(frame);
                }
                self.preview_frames += 1;
                Observed {
                    raw: frame,
                    shadow: out.shadow,
                    phase: self.phase,
                    alert: self.alert,
                    delivered_to_game: false,
                }
            }
            Phase::Auditing => {
                let signal = self.chosen(frame);
                self.audit_frames += 1;
                let result = self.audit.as_mut().and_then(|a| a.push(signal));
                if let Some((median, mad)) = result {
                    let shift =
                        (median - self.center.expect("audit requires verified center")).norm();
                    if mad.x > MAX_AUDIT_MAD || mad.y > MAX_AUDIT_MAD {
                        self.revoke(Alert::UnstableNeutralWindow, Phase::Warning);
                    } else if shift > MIN_CENTER_SHIFT {
                        self.revoke(Alert::SignificantNeutralDisagreement, Phase::Warning);
                    } else {
                        // Audit was human-labeled; low disagreement is not proof
                        // of being released, nor does it update the center.
                        self.phase = Phase::Preview;
                        self.audit = None;
                        self.last = None;
                        self.core.resume_after_operator_audit();
                    }
                }
                self.raw_output(frame)
            }
            _ => self.raw_output(frame),
        }
    }
    pub fn status_json(&self) -> String {
        format!(
            concat!(
                "{{\"phase\":\"{:?}\",\"alert\":\"{}\",\"seen\":{},",
                "\"preview_frames\":{},\"audit_frames\":{},\"raw_frames\":{},",
                "\"operator_release_is_assumed_not_verified\":true,",
                "\"automatically_learned_center\":false,",
                "\"game_output_enabled\":false,",
                "\"production\":\"BLOCKED\"}}"
            ),
            self.phase,
            self.alert.name(),
            self.seen,
            self.preview_frames,
            self.audit_frames,
            self.raw_frames
        )
    }
}
pub fn synthetic_center() -> VerifiedCenter {
    VerifiedCenter::restore_checked(Vec2::new(0.24, -0.18), Vec2::new(0.0001, 0.0001), 0.0001)
        .expect("bounded fixed synthetic fixture")
}
pub fn frame(t: u64, packet: u32, left: Vec2, right: Vec2) -> Frame {
    Frame {
        monotonic_us: t,
        packet,
        left,
        right,
        buttons: 0xA35F,
        left_trigger: 37,
        right_trigger: 249,
    }
}
/// Exact same scripted two-release transaction; raw frames now flow through
/// the full Frame/Core integration instead of the isolated v0.34 axis lab.
pub fn scripted_ready(stick: Stick, epoch: u64, offset: Vec2) -> io::Result<(Bridge, u64, u32)> {
    let mut b = Bridge::default();
    if !b.begin(epoch, stick, true) {
        return Err(io::Error::other("synthetic begin refused"));
    }
    b.confirm_release()?;
    let mut time = 0;
    let mut packet = 0;
    let mut poll = |b: &mut Bridge, input: Vec2| {
        time += 8000;
        packet += 1;
        let f = match stick {
            Stick::Left => frame(time, packet, input, Vec2::new(-0.5, 0.4)),
            Stick::Right => frame(time, packet, Vec2::new(-0.5, 0.4), input),
        };
        b.observe(f, epoch)
    };
    for i in 0..128 {
        let noise = if i % 2 == 0 { 0.00003 } else { -0.00003 };
        poll(&mut b, offset + Vec2::new(noise, -noise));
    }
    for i in 0..128 {
        poll(&mut b, offset + Vec2::new(-0.3 * (i as f64 / 127.0), 0.0));
    }
    b.confirm_movement()?;
    b.confirm_release()?;
    for i in 0..128 {
        let noise = if i % 2 == 0 { 0.00003 } else { -0.00003 };
        poll(&mut b, offset + Vec2::new(noise, -noise));
    }
    b.finish(true)?;
    Ok((b, time, packet))
}
pub fn run(samples: usize, out: &Path) -> io::Result<()> {
    if !(1024..=1_000_000).contains(&samples) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frames must be 1024..1000000",
        ));
    }
    let mut processed = 0u64;
    let mut lost = 0u64;
    let mut micro_frames = 0u64;
    let mut micro_lost = 0u64;
    let mut wrong_direction = 0u64;
    let mut warning = 0u64;
    let mut healthy = 0u64;
    for (index, stick) in [Stick::Left, Stick::Right].into_iter().enumerate() {
        let offset = Vec2::new(0.24, -0.18);
        let (mut bridge, mut stamp, mut pkt) = scripted_ready(stick, index as u64 + 1, offset)?;
        for i in 0..samples / 2 {
            stamp += 8000;
            pkt += 1;
            let micro = i % 4 == 0;
            let magnitude = if micro { 0.00012 } else { 0.07 };
            let pad = if index == 0 {
                Vec2::new(if i % 8 < 4 { magnitude } else { -magnitude }, 0.0)
            } else {
                Vec2::new(0.0, if i % 8 < 4 { -magnitude } else { magnitude })
            };
            let f = match stick {
                Stick::Left => frame(stamp, pkt, offset + pad, Vec2::new(-0.5, 0.4)),
                Stick::Right => frame(stamp, pkt, Vec2::new(-0.5, 0.4), offset + pad),
            };
            let res = bridge.observe(f, index as u64 + 1);
            if res.delivered_to_game {
                return Err(io::Error::other("unexpected game output"));
            }
            if res.phase != Phase::Preview {
                return Err(io::Error::other("preview unexpectedly revoked"));
            }
            processed += 1;
            let selected = if index == 0 {
                res.shadow.left
            } else {
                res.shadow.right
            };
            if (selected - pad).norm() > if micro { 0.00008 } else { 0.001 } {
                lost += 1;
            }
            if micro {
                micro_frames += 1;
                if selected.norm() < pad.norm() * 0.5 {
                    micro_lost += 1;
                }
            }
            if selected.x * pad.x + selected.y * pad.y < 0.0 {
                wrong_direction += 1;
            }
            if res.shadow.buttons != f.buttons
                || res.shadow.left_trigger != f.left_trigger
                || res.shadow.right_trigger != f.right_trigger
            {
                return Err(io::Error::other("non-stick control was modified"));
            }
            if (index == 0 && res.shadow.right != f.right)
                || (index == 1 && res.shadow.left != f.left)
            {
                return Err(io::Error::other("non-selected stick was modified"));
            }
        }
        if !bridge.begin_operator_neutral_audit(true) {
            return Err(io::Error::other("operator synthetic audit start failed"));
        }
        for i in 0..128 {
            stamp += 8000;
            pkt += 1;
            let shift = if index == 0 {
                Vec2::new(0.09, 0.0)
            } else {
                Vec2::ZERO
            };
            let r = offset + shift + Vec2::new(if i % 2 == 0 { 0.00003 } else { -0.00003 }, 0.0);
            let f = match stick {
                Stick::Left => frame(stamp, pkt, r, Vec2::new(-0.5, 0.4)),
                Stick::Right => frame(stamp, pkt, Vec2::new(-0.5, 0.4), r),
            };
            let out = bridge.observe(f, index as u64 + 1);
            if out.delivered_to_game {
                return Err(io::Error::other("audit emitted game input"));
            }
        }
        if index == 0 {
            if bridge.phase() != Phase::Warning
                || bridge.alert() != Alert::SignificantNeutralDisagreement
            {
                return Err(io::Error::other(
                    "shifted operator-declared neutral failed warning",
                ));
            }
            warning += 1;
        } else {
            if bridge.phase() != Phase::Preview || bridge.center().is_none() {
                return Err(io::Error::other(
                    "stable audit should preserve approved center",
                ));
            }
            healthy += 1;
            stamp += 8_000;
            pkt += 1;
            let f = frame(
                stamp,
                pkt,
                Vec2::new(-0.5, 0.4),
                offset + Vec2::new(0.002, 0.0),
            );
            if bridge.observe(f, index as u64 + 1).phase != Phase::Preview {
                return Err(io::Error::other("stable audit failed to resume preview"));
            }
        }
    }
    let json = format!(
        concat!(
            "{{\"schema\":1,\"source\":\"v35_scripted_full_frame_recenter_bridge\",",
            "\"requested_frames_per_two_sticks\":{},\"preview_frames\":{},",
            "\"micro_or_intended_motion_damaged\":{},\"micro_frames\":{},",
            "\"micro_lost\":{},\"wrong_direction\":{},\"operator_declared_shift_warnings\":{},",
            "\"stable_operator_declared_audits\":{},",
            "\"physical_release_verified\":false,",
            "\"automatic_drift_detection_claim\":false,",
            "\"game_output_writes\":0,\"production_gate\":\"BLOCKED\"}}\n"
        ),
        samples, processed, lost, micro_frames, micro_lost, wrong_direction, warning, healthy
    );
    crate::truth_lab::save_new(out, &json)?;
    println!("V035 MEMORY ONLY: preview={} errors={} micro_lost={}/{} wrong_direction={} warnings={} stable={} GAME_WRITES=0",
        processed,lost,micro_lost,micro_frames,wrong_direction,warning,healthy);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operator_approved_calibration_arm_and_full_frame_passthrough() {
        for (epoch, stick) in [(1, Stick::Left), (2, Stick::Right)] {
            let (mut b, t, n) = scripted_ready(stick, epoch, Vec2::new(0.24, -0.18)).unwrap();
            let f = frame(
                t + 8000,
                n + 1,
                Vec2::new(0.27, -0.18),
                Vec2::new(-0.5, 0.4),
            );
            let out = b.observe(f, epoch);
            assert_eq!(out.phase, Phase::Preview);
            assert_eq!(
                (
                    out.shadow.buttons,
                    out.shadow.left_trigger,
                    out.shadow.right_trigger
                ),
                (f.buttons, f.left_trigger, f.right_trigger)
            );
            if stick == Stick::Left {
                assert_eq!(out.shadow.right, f.right);
            } else {
                assert_eq!(out.shadow.left, f.left);
            }
            assert!(!out.delivered_to_game);
        }
    }
    #[test]
    fn no_consent_no_calibration_and_never_game_output() {
        let mut b = Bridge::default();
        assert!(!b.begin(1, Stick::Left, false));
        assert_eq!(b.alert(), Alert::OperatorReleaseRequired);
        let f = frame(8000, 1, Vec2::new(0.24, -0.18), Vec2::ZERO);
        assert_eq!(b.observe(f, 1).shadow, f);
        assert!(b.finish(true).is_err());
    }
    #[test]
    fn operator_declared_shift_warns_and_revokes_preview() {
        let (mut b, mut t, mut p) = scripted_ready(Stick::Left, 1, Vec2::new(0.24, -0.18)).unwrap();
        assert!(!b.begin_operator_neutral_audit(false));
        assert_eq!(b.phase(), Phase::Preview);
        assert!(b.begin_operator_neutral_audit(true));
        for _ in 0..128 {
            t += 8000;
            p += 1;
            let o = b.observe(frame(t, p, Vec2::new(0.35, -0.18), Vec2::ZERO), 1);
            assert!(!o.delivered_to_game);
        }
        assert_eq!(b.phase(), Phase::Warning);
        assert_eq!(b.alert(), Alert::SignificantNeutralDisagreement);
        assert!(b.center().is_none());
        let raw = frame(t + 8000, p + 1, Vec2::new(0.35, -0.18), Vec2::ZERO);
        assert_eq!(b.observe(raw, 1).shadow, raw);
    }
    #[test]
    fn noisy_operator_declared_window_cannot_recenter() {
        let (mut b, mut t, mut p) =
            scripted_ready(Stick::Right, 7, Vec2::new(0.24, -0.18)).unwrap();
        assert!(b.begin_operator_neutral_audit(true));
        for i in 0..128 {
            t += 8000;
            p += 1;
            let raw = Vec2::new(0.24 + if i % 2 == 0 { 0.025 } else { -0.025 }, -0.18);
            b.observe(frame(t, p, Vec2::ZERO, raw), 7);
        }
        assert_eq!(b.phase(), Phase::Warning);
        assert_eq!(b.alert(), Alert::UnstableNeutralWindow);
        assert!(b.center().is_none());
    }
    #[test]
    fn stable_human_labeled_audit_preserves_original_center() {
        let (mut b, mut t, mut p) = scripted_ready(Stick::Left, 6, Vec2::new(0.24, -0.18)).unwrap();
        let center = b.center();
        assert!(b.begin_operator_neutral_audit(true));
        for _ in 0..128 {
            t += 8000;
            p += 1;
            b.observe(frame(t, p, Vec2::new(0.24, -0.18), Vec2::ZERO), 6);
        }
        assert_eq!(b.phase(), Phase::Preview);
        assert_eq!(b.alert(), Alert::None);
        assert_eq!(b.center(), center);
    }
    #[test]
    fn disconnect_and_packet_rollback_latch_fault() {
        let (mut b, t, p) = scripted_ready(Stick::Left, 1, Vec2::new(0.24, -0.18)).unwrap();
        let f = frame(t + 8000, p + 1, Vec2::new(0.24, -0.18), Vec2::ZERO);
        b.observe(f, 2);
        assert_eq!(b.phase(), Phase::Fault);
        assert_eq!(b.alert(), Alert::DeviceSessionChanged);
        assert!(b.center().is_none());
        let (mut b, t, p) = scripted_ready(Stick::Left, 1, Vec2::new(0.24, -0.18)).unwrap();
        b.observe(
            frame(t + 8000, p + 1, Vec2::new(0.24, -0.18), Vec2::ZERO),
            1,
        );
        b.observe(frame(t + 16000, p, Vec2::new(0.24, -0.18), Vec2::ZERO), 1);
        assert_eq!(b.phase(), Phase::Fault);
        assert_eq!(b.alert(), Alert::StreamDiscontinuity);
    }
    #[test]
    fn malformed_signal_can_never_continue_preview() {
        let (mut b, t, p) = scripted_ready(Stick::Left, 1, Vec2::new(0.24, -0.18)).unwrap();
        let o = b.observe(
            frame(t + 8000, p + 1, Vec2::new(f64::NAN, 0.0), Vec2::ZERO),
            1,
        );
        assert_eq!(o.phase, Phase::Fault);
        assert_eq!(o.shadow.left, Vec2::ZERO);
        assert!(o.shadow.left.finite());
        assert!(!o.delivered_to_game);
    }
    #[test]
    fn cli_synthetic_pair_has_exact_status_and_no_physical_claims() {
        let tmp = std::env::temp_dir().join(format!("v35-{}-{}.json", std::process::id(), 33));
        if tmp.exists() {
            std::fs::remove_file(&tmp).unwrap();
        }
        run(4096, &tmp).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        assert!(body.contains("\"operator_declared_shift_warnings\":1"));
        assert!(body.contains("\"game_output_writes\":0"));
        assert!(body.contains("\"physical_release_verified\":false"));
        assert!(run(4096, &tmp).is_err());
        let _ = std::fs::remove_file(tmp);
    }
}
