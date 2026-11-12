//! Battery indicator: charge level colors and sizes, in logical pixels.

/// How full the battery is, which picks the fill color: green, yellow or red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryLevel {
    High,
    Medium,
    Low,
}

/// At or below this the fill turns red.
pub const LOW_PERCENT: u8 = 20;
/// At or below this the fill turns yellow.
pub const MEDIUM_PERCENT: u8 = 50;

/// What the system reports about the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryStatus {
    /// Charge, 0–100.
    pub percent: u8,
    /// Connected to a power supply (charging or full).
    pub plugged_in: bool,
    /// Actually charging; the fill pulses.
    pub charging: bool,
}

pub fn level_for(percent: u8) -> BatteryLevel {
    if percent <= LOW_PERCENT {
        BatteryLevel::Low
    } else if percent <= MEDIUM_PERCENT {
        BatteryLevel::Medium
    } else {
        BatteryLevel::High
    }
}

/// The share of the inner width filled for a charge percentage.
pub fn fill_fraction(percent: u8) -> f32 {
    f32::from(percent.min(100)) / 100.0
}

/// Sizes of the icon body (with a small cap on its right), the outline stroke,
/// and the percentage text below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryLayout {
    pub body_width: f32,
    pub body_height: f32,
    pub cap_width: f32,
    pub cap_height: f32,
    pub stroke: f32,
    pub font_size: f32,
    /// Distance from the window edges.
    pub edge_padding: f32,
}

impl BatteryLayout {
    /// Width available to the fill inside the outline and its padding.
    pub fn inner_width(&self) -> f32 {
        (self.body_width - 4.0 * self.stroke).max(0.0)
    }

    pub fn inner_height(&self) -> f32 {
        (self.body_height - 4.0 * self.stroke).max(0.0)
    }
}

/// Scales with the window like the date does; `size` is the user multiplier.
pub fn layout(width: f32, height: f32, size: f32) -> BatteryLayout {
    let short = width.max(1.0).min(height.max(1.0));
    let unit = (0.06 * short * size).max(20.0);
    BatteryLayout {
        body_width: unit * 2.0,
        body_height: unit,
        cap_width: unit * 0.14,
        cap_height: unit * 0.42,
        stroke: (unit * 0.07).max(2.0),
        // The percentage stays secondary to the icon but legible at a glance.
        font_size: (unit * 0.55).max(10.0),
        edge_padding: (short * 0.03).max(16.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_follows_the_green_yellow_red_thresholds() {
        for (percent, level) in [
            (100, BatteryLevel::High),
            (51, BatteryLevel::High),
            (50, BatteryLevel::Medium),
            (21, BatteryLevel::Medium),
            (20, BatteryLevel::Low),
            (0, BatteryLevel::Low),
        ] {
            assert_eq!(level_for(percent), level, "{percent}%");
        }
    }

    #[test]
    fn fill_tracks_the_charge_and_stays_in_range() {
        assert_eq!(fill_fraction(0), 0.0);
        assert!((fill_fraction(37) - 0.37).abs() < 1e-6);
        assert_eq!(fill_fraction(100), 1.0);
        assert_eq!(fill_fraction(140), 1.0);
    }

    #[test]
    fn layout_scales_with_the_window_and_the_size_setting() {
        let small = layout(1280.0, 800.0, 1.0);
        let big = layout(2560.0, 1600.0, 1.0);
        let doubled = layout(1280.0, 800.0, 2.0);
        assert!((small.body_width * 2.0 - big.body_width).abs() < 1e-3);
        assert!((small.body_width * 2.0 - doubled.body_width).abs() < 1e-3);
        assert!(small.body_width > small.body_height);
        assert!(small.inner_width() > 0.0 && small.inner_height() > 0.0);
    }

    #[test]
    fn layout_never_collapses_in_a_tiny_window() {
        let tiny = layout(10.0, 10.0, 0.5);
        assert!(tiny.body_height >= 20.0);
        assert!(tiny.font_size >= 10.0);
        assert!(tiny.inner_width() > 0.0);
    }
}
