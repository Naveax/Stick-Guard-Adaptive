//! Quantization and saturation at the INPUT boundary, before any filter.
//! XInput16 models transport report granularity, not the physical stick ADC.
//! No dither is added: dithering would invent jitter and obscure motion fidelity.
use crate::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Ideal mathematical sample from prior v0.3 simulations.
    Ideal,
    /// Signed 16-bit XInput axis report: [-32768,32767].
    XInput16,
    /// Symmetric quantization to signed 12-bit-like integer representation.
    Signed12,
    /// Symmetric quantization to signed 8-bit-like integer representation.
    Signed8,
}

impl Resolution {
    pub const ALL: [Self; 4] = [Self::Ideal, Self::XInput16, Self::Signed12, Self::Signed8];
    pub fn name(self) -> &'static str {
        match self {
            Self::Ideal => "ideal",
            Self::XInput16 => "xinput16",
            Self::Signed12 => "signed12",
            Self::Signed8 => "signed8",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|r| r.name().eq_ignore_ascii_case(value))
    }
    /// One positive-axis code step, normalized [-1,+1].
    pub fn positive_lsb(self) -> f64 {
        match self {
            Self::Ideal => 0.0,
            Self::XInput16 => 1.0 / 32767.0,
            Self::Signed12 => 1.0 / 2047.0,
            Self::Signed8 => 1.0 / 127.0,
        }
    }
    pub fn quantize_axis(self, x: f64) -> f64 {
        if !x.is_finite() {
            return 0.0;
        }
        let x = x.clamp(-1.0, 1.0);
        match self {
            Self::Ideal => x,
            // Avoid asymmetry bugs: -32768 / 32768 and +32767 / 32767.
            Self::XInput16 => {
                if x >= 0.0 {
                    (x * 32767.0).round() / 32767.0
                } else {
                    (x * 32768.0).round() / 32768.0
                }
            }
            Self::Signed12 => {
                if x >= 0.0 {
                    (x * 2047.0).round() / 2047.0
                } else {
                    (x * 2048.0).round() / 2048.0
                }
            }
            Self::Signed8 => {
                if x >= 0.0 {
                    (x * 127.0).round() / 127.0
                } else {
                    (x * 128.0).round() / 128.0
                }
            }
        }
    }
    /// Analog X/Y are independently digitized. No radial renormalization:
    /// it would incorrectly modify true axis reporting and micro-movement.
    pub fn quantize(self, xy: Vec2) -> Vec2 {
        Vec2::new(self.quantize_axis(xy.x), self.quantize_axis(xy.y))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_exact_signed_extremes() {
        for r in Resolution::ALL {
            assert_eq!(r.quantize(Vec2::new(-1., 1.)), Vec2::new(-1., 1.));
            assert_eq!(r.quantize(Vec2::ZERO), Vec2::ZERO);
        }
    }
    #[test]
    fn tiny_001_percent_is_below_xinput16_half_lsb() {
        // 0.001% = 1e-5 in normalized units.
        let tiny = Vec2::new(0.00001, 0.0);
        assert_eq!(Resolution::XInput16.quantize(tiny), Vec2::ZERO);
        assert_eq!(Resolution::Ideal.quantize(tiny), tiny);
        assert!(Resolution::XInput16.positive_lsb() > 0.00003);
    }
    #[test]
    fn quantization_is_monotonic_bounded_idempotent() {
        for r in Resolution::ALL {
            let mut prev = -2.0;
            for i in -10000..=10000 {
                let x = i as f64 / 10000.0;
                let y = r.quantize_axis(x);
                assert!(y >= prev - 1e-15, "{:?} x={x} y={y}", r);
                assert!((-1.0..=1.0).contains(&y));
                let again = r.quantize_axis(y);
                assert!((again - y).abs() < 1e-15);
                prev = y;
            }
        }
    }
    #[test]
    fn no_nan_inf_input_is_exported() {
        for r in Resolution::ALL {
            for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(r.quantize_axis(x), 0.0);
            }
        }
    }
    #[test]
    fn input_differences_may_be_indistinguishable() {
        let a = Resolution::XInput16.quantize_axis(0.000001);
        let b = Resolution::XInput16.quantize_axis(0.000010);
        assert_eq!(a, b);
    }
}
