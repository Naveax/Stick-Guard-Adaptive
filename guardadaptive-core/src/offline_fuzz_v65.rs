//! v0.65 deterministic controller-free packet property fuzzing.
//! Uses the actual GuardAdaptive XInput encoder and permanently disabled
//! driver backend, never the separate Python demonstration filter.
use crate::{
    output_adapter::{DisabledBackend, VirtualGamepad, XboxPacket},
    runtime_core::Frame,
    Vec2,
};
use std::{io, path::Path};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn axis(&mut self) -> f64 {
        let x = self.next() >> 11;
        (x as f64 / (1_u64 << 53) as f64) * 2.0 - 1.0
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Evidence {
    pub frames: usize,
    pub valid_frames: usize,
    pub rejected_axis_frames: usize,
    pub disabled_driver_denials: usize,
    pub largest_axis_error: f64,
}
pub fn audit(frames: usize, seed: u64) -> io::Result<Evidence> {
    if !(1000..=1_048_576).contains(&frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fuzz frames must be 1000..1048576",
        ));
    }
    let mut rng = Rng(seed | 1);
    let mut evidence = Evidence {
        frames,
        valid_frames: 0,
        rejected_axis_frames: 0,
        disabled_driver_denials: 0,
        largest_axis_error: 0.0,
    };
    let mut unavailable = DisabledBackend;
    for i in 0..frames {
        let mut axes = [rng.axis(), rng.axis(), rng.axis(), rng.axis()];
        match i % 17 {
            0 => axes[0] = f64::NAN,
            1 => axes[1] = f64::INFINITY,
            2 => axes[2] = f64::NEG_INFINITY,
            3 => axes[3] = 1.001,
            4 => axes[0] = -1.001,
            5 => axes[1] = f64::MAX,
            6 => axes[2] = -f64::MAX,
            7 => axes[0] = 1.0,
            8 => axes[0] = -1.0,
            9 => axes[0] = -0.0,
            _ => {}
        }
        let f = Frame {
            monotonic_us: i as u64 * 8_000,
            packet: i as u32,
            left: Vec2::new(axes[0], axes[1]),
            right: Vec2::new(axes[2], axes[3]),
            buttons: rng.next() as u16,
            left_trigger: rng.next() as u8,
            right_trigger: rng.next() as u8,
        };
        let valid = axes
            .iter()
            .all(|v| v.is_finite() && (-1.0..=1.0).contains(v));
        match XboxPacket::encode(f) {
            Ok(packet) => {
                if !valid {
                    return Err(io::Error::other(
                        "nonfinite or out-of-range analog axis encoded",
                    ));
                }
                if packet.buttons != f.buttons
                    || packet.left_trigger != f.left_trigger
                    || packet.right_trigger != f.right_trigger
                {
                    return Err(io::Error::other("nontarget controls were mutated"));
                }
                let (l, r) = packet.decoded_axes();
                for (a, b) in [
                    (axes[0], l.x),
                    (axes[1], l.y),
                    (axes[2], r.x),
                    (axes[3], r.y),
                ] {
                    let err = (a - b).abs();
                    if !err.is_finite() || err > 0.000031 {
                        return Err(io::Error::other("analog axis quantization exceeded bound"));
                    }
                    evidence.largest_axis_error = evidence.largest_axis_error.max(err);
                }
                evidence.valid_frames += 1;
                if unavailable.send(packet).is_ok() {
                    return Err(io::Error::other(
                        "production-disabled backend accepted game output",
                    ));
                }
                evidence.disabled_driver_denials += 1;
            }
            Err(_) => {
                if valid {
                    return Err(io::Error::other("valid analog axis was rejected"));
                }
                evidence.rejected_axis_frames += 1;
            }
        }
    }
    if evidence.valid_frames + evidence.rejected_axis_frames != frames
        || evidence.rejected_axis_frames == 0
        || evidence.valid_frames == 0
        || evidence.disabled_driver_denials != evidence.valid_frames
    {
        return Err(io::Error::other("fuzz case coverage incomplete"));
    }
    Ok(evidence)
}
pub fn run(frames: usize, seed: u64, out: &Path) -> io::Result<()> {
    let e = audit(frames, seed)?;
    let json = format!(
        concat!(
        "{{\"schema\":1,\"source\":\"v65_rust_real_encoder_deterministic_property_fuzz\",",
        "\"frames\":{},\"seed\":{},\"valid_frames\":{},",
        "\"rejected_nonfinite_or_out_of_range_axes\":{},",
        "\"disabled_driver_send_denials\":{},\"largest_axis_roundtrip_error\":{},",
        "\"buttons_triggers_exact\":true,\"invalid_axes_fail_closed\":true,",
        "\"game_transport_permanently_disabled\":true,\"separate_python_reference_used\":false,",
        "\"actual_controller_used\":false,\"driver_writes\":0,\"game_input_writes\":0,",
        "\"physical_acceptance\":\"BLOCKED\",\"production_gate\":\"BLOCKED\"}}\n"),
        frames,
        seed,
        e.valid_frames,
        e.rejected_axis_frames,
        e.disabled_driver_denials,
        e.largest_axis_error
    );
    crate::truth_lab::save_new(out, &json)?;
    println!("V065_RUST_FUZZ=PASS frames={frames} valid={} rejected={} driver_writes=0 production=BLOCKED",
             e.valid_frames,e.rejected_axis_frames);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adversarial_property_inputs_real_encoder() {
        let e = audit(34000, 0xA5A51234).unwrap();
        assert_eq!(e.frames, 34000);
        assert_eq!(e.disabled_driver_denials, e.valid_frames);
        assert!(e.rejected_axis_frames > 12000);
        assert!(e.largest_axis_error <= 0.000031);
    }
    #[test]
    fn input_size_validation() {
        assert!(audit(0, 2).is_err());
        assert!(audit(999, 2).is_err());
        assert!(audit(1_048_577, 2).is_err());
    }
}
