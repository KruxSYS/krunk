//! DIP ↔ physical pixel conversion in one place (0.15.4).
//!
//! Krunk's layout constants are in DIP (device-independent pixels, 1/96 inch; the same unit
//! as a CSS pixel of the shell at zoom 1). A window's scale is its monitor's DPI / 96:
//! 1.0 at 100 %, 1.25, 1.5, 1.75, 2.0, 2.5 and so on. Rules for code that places things:
//!
//! - take the scale from the window the thing belongs to, at the moment it is placed
//!   (`Window::scale()`), never from a cached value or the system DPI;
//! - convert with [`Scale::px`] (round to nearest) unless a different rounding is wanted on
//!   purpose, and then say so with [`Scale::px_floor`] or [`Scale::px_ceil`];
//! - edges that must meet are computed as positions and subtracted, not as two rounded
//!   sizes added up (round(a) + round(b) is not round(a + b) at 125 % and 175 %).

/// Physical pixels per DIP of one window (1.0 = 96 DPI).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Scale(f64);

impl Default for Scale {
    fn default() -> Self {
        Scale::ONE
    }
}

impl Scale {
    pub const ONE: Scale = Scale(1.0);
    /// The DPI of scale 1.0.
    pub const BASE_DPI: u32 = 96;

    /// From a factor. Anything that is not a usable scale (not finite, below 50 % or above
    /// 800 %) counts as 100 %.
    pub fn new(factor: f64) -> Scale {
        if factor.is_finite() && (0.5..=8.0).contains(&factor) {
            Scale(factor)
        } else {
            Scale::ONE
        }
    }

    /// From a DPI value as `GetDpiForWindow` gives it (0 when the window is gone).
    pub fn from_dpi(dpi: u32) -> Scale {
        Scale::new(f64::from(dpi) / f64::from(Scale::BASE_DPI))
    }

    pub fn factor(self) -> f64 {
        self.0
    }

    pub fn dpi(self) -> u32 {
        (self.0 * f64::from(Scale::BASE_DPI)).round() as u32
    }

    /// DIP to pixels, rounded to the nearest pixel.
    pub fn px(self, dip: f64) -> i32 {
        to_i32((dip * self.0).round())
    }

    /// DIP to pixels, rounded down (a point inside something).
    pub fn px_floor(self, dip: f64) -> i32 {
        to_i32((dip * self.0).floor())
    }

    /// DIP to pixels, rounded up (something that must be at least this big).
    pub fn px_ceil(self, dip: f64) -> i32 {
        to_i32((dip * self.0).ceil())
    }

    /// Pixels to DIP.
    pub fn dip(self, px: i32) -> f64 {
        f64::from(px) / self.0
    }

    /// A length in pixels at scale `from`, at this scale (a saved window size shown on a
    /// monitor with another scale).
    pub fn rescale(self, px: i32, from: Scale) -> i32 {
        if self == from {
            px
        } else {
            to_i32((f64::from(px) * self.0 / from.0).round())
        }
    }
}

fn to_i32(v: f64) -> i32 {
    if v.is_finite() {
        v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scales Windows offers up to 250 %, as DPI values.
    const DPIS: [u32; 8] = [96, 120, 144, 168, 192, 216, 240, 288];

    #[test]
    fn from_dpi_and_back() {
        for dpi in DPIS {
            let s = Scale::from_dpi(dpi);
            assert_eq!(s.dpi(), dpi);
            assert!((s.factor() - f64::from(dpi) / 96.0).abs() < 1e-12);
        }
        assert_eq!(Scale::from_dpi(0), Scale::ONE, "a window that is gone");
        assert_eq!(Scale::from_dpi(10), Scale::ONE);
        assert_eq!(Scale::from_dpi(96 * 9), Scale::ONE);
        assert_eq!(Scale::new(f64::NAN), Scale::ONE);
        assert_eq!(Scale::new(f64::INFINITY), Scale::ONE);
        assert_eq!(Scale::new(-1.5), Scale::ONE);
        assert_eq!(Scale::default(), Scale::ONE);
    }

    #[test]
    fn at_100_percent_pixels_are_dips() {
        let s = Scale::ONE;
        for dip in [0.0, 1.0, 28.0, 36.0, 76.0, 104.0, 480.0, 1280.0] {
            assert_eq!(s.px(dip), dip as i32);
            assert_eq!(s.px_floor(dip), dip as i32);
            assert_eq!(s.px_ceil(dip), dip as i32);
            assert_eq!(s.dip(dip as i32), dip);
        }
    }

    #[test]
    fn rounding_is_a_choice() {
        let s = Scale::from_dpi(120); // 125 %
        assert_eq!(s.px(76.0), 95);
        assert_eq!(s.px(5.0), 6, "6.25 rounds to 6");
        assert_eq!(s.px_floor(5.0), 6);
        assert_eq!(s.px_ceil(5.0), 7);
        assert_eq!(s.px(2.0), 3, "2.5 rounds away from zero");
        assert_eq!(s.px_floor(2.0), 2);
        let s = Scale::from_dpi(168); // 175 %
        assert_eq!(s.px(76.0), 133);
        assert_eq!(s.px(28.0), 49);
        assert_eq!(s.px(104.0), 182, "the sum, not 133 + 49 by luck: positions are rounded once");
        assert_eq!(s.px(1.0), 2);
        assert_eq!(s.px_floor(1.0), 1);
        assert_eq!(s.px(-8.0), -14);
    }

    #[test]
    fn dip_of_px_round_trips_within_a_pixel() {
        for dpi in DPIS {
            let s = Scale::from_dpi(dpi);
            for px in [0, 1, 7, 95, 133, 1279, 3840] {
                assert!((s.px(s.dip(px)) - px).abs() <= 0, "{dpi} dpi, {px} px");
            }
        }
    }

    #[test]
    fn rescale_keeps_the_dip_size() {
        let (a, b) = (Scale::from_dpi(96), Scale::from_dpi(144));
        assert_eq!(b.rescale(1280, a), 1920);
        assert_eq!(a.rescale(1920, b), 1280);
        assert_eq!(b.rescale(861, b), 861, "same scale: untouched, no rounding");
        let c = Scale::from_dpi(168);
        assert_eq!(c.rescale(1280, a), 2240);
        assert_eq!(a.rescale(c.rescale(1001, a), c), 1001);
    }

    #[test]
    fn huge_values_do_not_wrap() {
        let s = Scale::from_dpi(240);
        assert_eq!(s.px(1e12), i32::MAX);
        assert_eq!(s.px(f64::NAN), 0);
    }
}
