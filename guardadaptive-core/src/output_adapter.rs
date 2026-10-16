//! v0.30: exact, testable XInput-compatible packet encoding interface.
//! Backend intentionally denied until driver/physical acceptance. Mapping a
//! packet is NOT publishing one to a Windows game or a virtual device.
use crate::{
    runtime_core::{Frame, Output},
    Vec2,
};
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XboxPacket {
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub buttons: u16,
}
fn axis(x: f64) -> io::Result<i16> {
    if !x.is_finite() || !(-1.0..=1.0).contains(&x) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid normalized analog axis",
        ));
    }
    // XInput axis endpoints are asymmetric: -32768 versus +32767.
    Ok((x * if x < 0.0 { 32768.0 } else { 32767.0 }).round() as i16)
}
impl XboxPacket {
    pub fn encode(frame: Frame) -> io::Result<Self> {
        Ok(Self {
            left_x: axis(frame.left.x)?,
            left_y: axis(frame.left.y)?,
            right_x: axis(frame.right.x)?,
            right_y: axis(frame.right.y)?,
            left_trigger: frame.left_trigger,
            right_trigger: frame.right_trigger,
            buttons: frame.buttons,
        })
    }
    pub fn from_shadow(output: Output) -> io::Result<Self> {
        // Even this conversion has no device handle and performs no I/O.
        Self::encode(output.shadow)
    }
    pub fn decoded_axes(self) -> (Vec2, Vec2) {
        let scale = crate::xinput::signed_axis;
        (
            Vec2::new(scale(self.left_x), scale(self.left_y)),
            Vec2::new(scale(self.right_x), scale(self.right_y)),
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendStatus {
    Unavailable,
    NoAuthorizedPhysicalDevice,
    CertificationRequired,
}
impl BackendStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unavailable => "NO_COMPATIBLE_DRIVER_BACKEND",
            Self::NoAuthorizedPhysicalDevice => "PHYSICAL_DEVICE_NOT_AUTHENTICATED",
            Self::CertificationRequired => "GAME_OUTPUT_NOT_QUALIFIED",
        }
    }
}
/// Caution: not a driver. The only shipping implementation refuses writes.
/// A real backend would require trusted device binding, an explicit user
/// action, a per-device kill switch and independent game-facing tests.
pub trait VirtualGamepad {
    fn status(&self) -> BackendStatus;
    fn send(&mut self, packet: XboxPacket) -> io::Result<()>;
    fn deactivate(&mut self) -> io::Result<()>;
}
#[derive(Default)]
pub struct DisabledBackend;
impl VirtualGamepad for DisabledBackend {
    fn status(&self) -> BackendStatus {
        BackendStatus::Unavailable
    }
    fn send(&mut self, _: XboxPacket) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "virtual gamepad not installed, authenticated or authorized",
        ))
    }
    fn deactivate(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_core::Reason;
    fn f(left: Vec2) -> Frame {
        Frame {
            monotonic_us: 10,
            packet: 2,
            left,
            right: Vec2::new(0.1, -0.1),
            buttons: 0xA55A,
            left_trigger: 255,
            right_trigger: 7,
        }
    }
    #[test]
    fn endpoint_and_quantization_roundtrip() {
        let p = XboxPacket::encode(f(Vec2::new(-1.0, 1.0))).unwrap();
        assert_eq!((p.left_x, p.left_y), (-32768, 32767));
        assert_eq!(
            (p.buttons, p.left_trigger, p.right_trigger),
            (0xA55A, 255, 7)
        );
        assert_eq!(p.decoded_axes().0, Vec2::new(-1.0, 1.0));
        assert_eq!(XboxPacket::encode(f(Vec2::ZERO)).unwrap().left_x, 0);
        for a in [-0.99, -0.3, -0.0001, 0.0001, 0.25, 0.8, 0.999] {
            let q = XboxPacket::encode(f(Vec2::new(a, -a))).unwrap();
            assert!((q.decoded_axes().0.x - a).abs() <= 1.0 / 32767.0);
            assert!((q.decoded_axes().0.y + a).abs() <= 1.0 / 32767.0);
        }
    }
    #[test]
    fn invalid_axes_fail_closed() {
        for v in [f64::NAN, f64::INFINITY, -1.1, 1.000001] {
            assert!(XboxPacket::encode(f(Vec2::new(v, 0.0))).is_err());
        }
    }
    #[test]
    fn unmodified_controls_pass_through_shadow_mapping() {
        let raw = f(Vec2::new(0.2, -0.3));
        let shadow = Frame {
            left: Vec2::new(0.19, -0.29),
            ..raw
        };
        let p = XboxPacket::from_shadow(Output {
            original: raw,
            shadow,
            reason: Reason::PreviewOnly,
            delivered_to_game: false,
        })
        .unwrap();
        assert_eq!(p.buttons, raw.buttons);
        assert_eq!(p.right_trigger, raw.right_trigger);
        assert_eq!(p.left_trigger, raw.left_trigger);
        assert_eq!(p.right_x, axis(raw.right.x).unwrap());
        assert_ne!(p.left_x, axis(raw.left.x).unwrap());
    }
    #[test]
    fn disabled_backend_never_accepts_device_packets() {
        let p = XboxPacket::encode(f(Vec2::ZERO)).unwrap();
        let mut backend = DisabledBackend;
        assert_eq!(backend.status(), BackendStatus::Unavailable);
        assert_eq!(
            backend.send(p).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(backend.deactivate().is_ok());
    }
}
