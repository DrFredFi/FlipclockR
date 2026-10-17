//! Startup window placement: restore the saved position when it is visible,
//! otherwise center a default-sized window on the primary monitor.

use crate::config::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

pub const DEFAULT_WIDTH: f64 = 1280.0;
pub const DEFAULT_HEIGHT: f64 = 720.0;
/// The default size never exceeds this fraction of the primary monitor.
pub const MAX_DEFAULT_FRACTION: f64 = 0.8;
/// Minimum visible title-bar area, in logical pixels, for a saved position to be reused.
pub const MIN_VISIBLE_TITLE: (f64, f64) = (100.0, 30.0);

/// A monitor in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// The saved window: outer position in physical pixels, inner size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedWindow {
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Outer position in physical pixels; `None` lets the OS decide.
    pub position: Option<(i32, i32)>,
    /// Inner size in logical pixels.
    pub size: (f64, f64),
}

pub fn place(
    saved: Option<SavedWindow>,
    monitors: &[Monitor],
    primary: Option<Monitor>,
) -> Placement {
    if let Some(s) = saved
        && title_bar_visible(&s, monitors)
    {
        return Placement {
            position: Some((s.x, s.y)),
            size: (
                s.width.max(MIN_WINDOW_WIDTH),
                s.height.max(MIN_WINDOW_HEIGHT),
            ),
        };
    }
    let Some(p) = primary.or_else(|| monitors.first().copied()) else {
        return Placement {
            position: None,
            size: (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        };
    };
    let logical_w = f64::from(p.width) / p.scale;
    let logical_h = f64::from(p.height) / p.scale;
    let w = DEFAULT_WIDTH
        .min(logical_w * MAX_DEFAULT_FRACTION)
        .max(MIN_WINDOW_WIDTH);
    let h = DEFAULT_HEIGHT
        .min(logical_h * MAX_DEFAULT_FRACTION)
        .max(MIN_WINDOW_HEIGHT);
    let x = f64::from(p.x) + (f64::from(p.width) - w * p.scale) / 2.0;
    let y = f64::from(p.y) + (f64::from(p.height) - h * p.scale) / 2.0;
    Placement {
        position: Some((x.round() as i32, y.round() as i32)),
        size: (w, h),
    }
}

fn title_bar_visible(s: &SavedWindow, monitors: &[Monitor]) -> bool {
    monitors.iter().any(|m| {
        let (min_w, min_h) = (MIN_VISIBLE_TITLE.0 * m.scale, MIN_VISIBLE_TITLE.1 * m.scale);
        let left = f64::from(s.x);
        let top = f64::from(s.y);
        let right = left + s.width * m.scale;
        let bottom = top + MIN_VISIBLE_TITLE.1 * m.scale;
        let ix = right.min(f64::from(m.x) + f64::from(m.width)) - left.max(f64::from(m.x));
        let iy = bottom.min(f64::from(m.y) + f64::from(m.height)) - top.max(f64::from(m.y));
        ix >= min_w && iy >= min_h - 0.5
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Monitor = Monitor {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    const SECONDARY: Monitor = Monitor {
        x: 1920,
        y: 0,
        width: 2560,
        height: 1440,
        scale: 1.5,
    };

    #[test]
    fn restores_a_visible_saved_window() {
        let saved = SavedWindow {
            x: 2100,
            y: 200,
            width: 900.0,
            height: 500.0,
        };
        let p = place(Some(saved), &[PRIMARY, SECONDARY], Some(PRIMARY));
        assert_eq!(p.position, Some((2100, 200)));
        assert_eq!(p.size, (900.0, 500.0));
    }

    #[test]
    fn centers_on_primary_when_the_saved_monitor_is_gone() {
        let saved = SavedWindow {
            x: 2100,
            y: 200,
            width: 900.0,
            height: 500.0,
        };
        let p = place(Some(saved), &[PRIMARY], Some(PRIMARY));
        assert_eq!(p.size, (1280.0, 720.0));
        assert_eq!(p.position, Some((320, 180)));
    }

    #[test]
    fn a_barely_visible_title_bar_does_not_count() {
        let saved = SavedWindow {
            x: 1870,
            y: 100,
            width: 900.0,
            height: 500.0,
        };
        let p = place(Some(saved), &[PRIMARY], Some(PRIMARY));
        assert_eq!(
            p.position,
            Some((320, 180)),
            "only 50 px of title bar visible"
        );
    }

    #[test]
    fn a_window_above_the_screen_is_not_restored() {
        let saved = SavedWindow {
            x: 100,
            y: -500,
            width: 900.0,
            height: 500.0,
        };
        let p = place(Some(saved), &[PRIMARY], Some(PRIMARY));
        assert_eq!(p.size, (1280.0, 720.0));
    }

    #[test]
    fn default_size_is_clamped_to_eighty_percent_of_small_screens() {
        let small = Monitor {
            x: 0,
            y: 0,
            width: 1366,
            height: 768,
            scale: 1.0,
        };
        let p = place(None, &[small], Some(small));
        assert_eq!(p.size, (1366.0 * 0.8, 768.0 * 0.8));
    }

    #[test]
    fn default_size_accounts_for_scaling() {
        let hidpi = Monitor {
            x: 0,
            y: 0,
            width: 2560,
            height: 1600,
            scale: 2.0,
        };
        let p = place(None, &[hidpi], Some(hidpi));
        assert_eq!(p.size, (1280.0 * 0.8, 800.0 * 0.8));
        assert_eq!(p.position, Some((256, 160)));
    }

    #[test]
    fn no_monitors_lets_the_os_decide() {
        let p = place(None, &[], None);
        assert_eq!(p.position, None);
    }
}
