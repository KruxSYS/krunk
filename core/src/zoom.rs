//! Page zoom: the UI and settings use zoom factors (1.0 = 100 %); Chromium
//! uses zoom levels, where each level step is a factor of 1.2.

/// The factors the browser accepts (25 % to 500 %).
pub const MIN_FACTOR: f64 = 0.25;
pub const MAX_FACTOR: f64 = 5.0;

/// Chromium zoom level for a zoom factor (clamped to the allowed range;
/// non-finite input means 100 %).
pub fn factor_to_level(factor: f64) -> f64 {
    let f = if factor.is_finite() { factor.clamp(MIN_FACTOR, MAX_FACTOR) } else { 1.0 };
    f.ln() / 1.2f64.ln()
}

/// Zoom factor for a Chromium zoom level, rounded to 4 decimals so that a
/// factor survives the round trip exactly (110 % stays 1.1).
pub fn level_to_factor(level: f64) -> f64 {
    if !level.is_finite() {
        return 1.0;
    }
    let f = 1.2f64.powf(level).clamp(MIN_FACTOR, MAX_FACTOR);
    (f * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hundred_percent_is_level_zero() {
        assert_eq!(factor_to_level(1.0), 0.0);
        assert_eq!(level_to_factor(0.0), 1.0);
    }

    #[test]
    fn whole_levels_are_powers_of_1_2() {
        assert!((factor_to_level(1.2) - 1.0).abs() < 1e-12);
        assert!((factor_to_level(1.44) - 2.0).abs() < 1e-12);
        assert!((factor_to_level(1.0 / 1.2) + 1.0).abs() < 1e-12);
        assert_eq!(level_to_factor(1.0), 1.2);
        assert_eq!(level_to_factor(-1.0), 0.8333);
    }

    #[test]
    fn every_menu_step_round_trips() {
        for f in [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0] {
            assert_eq!(level_to_factor(factor_to_level(f)), f, "{f}");
        }
    }

    #[test]
    fn out_of_range_and_garbage() {
        assert_eq!(level_to_factor(factor_to_level(9.0)), MAX_FACTOR);
        assert_eq!(level_to_factor(factor_to_level(0.01)), MIN_FACTOR);
        assert_eq!(factor_to_level(f64::NAN), 0.0);
        assert_eq!(factor_to_level(-1.0), factor_to_level(MIN_FACTOR));
        assert_eq!(level_to_factor(f64::INFINITY), 1.0);
        assert_eq!(level_to_factor(100.0), MAX_FACTOR);
        assert_eq!(level_to_factor(-100.0), MIN_FACTOR);
    }
}
