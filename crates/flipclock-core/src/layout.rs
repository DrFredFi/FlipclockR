//! Clock face layout in logical pixels (origin top-left, y down).

use crate::config::DatePosition;
use crate::digits::TILE_COUNT;

/// Tile height relative to tile width.
pub const TILE_ASPECT: f32 = 1.45;
/// Gap between the two tiles of a pair, in tile widths.
pub const PAIR_GAP: f32 = 0.07;
/// Width of the slot holding a separator, in tile widths.
pub const SEPARATOR_SLOT: f32 = 0.5;
/// Separator dot radius, in hour/minute tile widths.
pub const DOT_RADIUS: f32 = 0.065;
/// Vertical offset of each separator dot from the tile center, in tile heights.
pub const DOT_OFFSET: f32 = 0.17;
/// The clock never drops below this fraction of the window in either direction.
pub const MIN_CLOCK_FRACTION: f32 = 0.4;
/// Date line height relative to window height, before the size multiplier.
pub const DATE_HEIGHT_FRACTION: f32 = 0.045;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DateLayout {
    /// x of the left edge, center or right edge depending on `position`.
    pub anchor_x: f32,
    pub center_y: f32,
    /// Line height in logical pixels.
    pub height: f32,
    pub position: DatePosition,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClockLayout {
    /// Hour/minute tile width in logical pixels.
    pub unit: f32,
    pub tiles: [Rect; TILE_COUNT],
    /// Centers of the separator dots: two separators, upper and lower dot each.
    pub dots: [[(f32, f32); 2]; 2],
    pub dot_radius: f32,
    /// Bounding box of tiles and separators.
    pub clock: Rect,
    pub date: Option<DateLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutInput {
    pub width: f32,
    pub height: f32,
    pub margin: f32,
    pub seconds_scale: f32,
    pub date_visible: bool,
    pub date_size: f32,
    pub date_position: DatePosition,
}

/// The margin actually used: the requested one, shrunk so the clock keeps at least
/// `MIN_CLOCK_FRACTION` of the window in both directions.
pub fn effective_margin(width: f32, height: f32, margin: f32) -> f32 {
    let max = (1.0 - MIN_CLOCK_FRACTION) / 2.0 * width.min(height);
    margin.clamp(0.0, max.max(0.0))
}

pub fn compute(input: &LayoutInput) -> ClockLayout {
    let (w, h) = (input.width.max(1.0), input.height.max(1.0));
    let s = input.seconds_scale.clamp(0.3, 1.0);
    let margin = effective_margin(w, h, input.margin);
    let pair = 2.0 + PAIR_GAP;
    let total_w = pair * 2.0 + SEPARATOR_SLOT * 2.0 + pair * s;
    let total_h = TILE_ASPECT;
    let unit = ((w - 2.0 * margin) / total_w)
        .min((h - 2.0 * margin) / total_h)
        .max(0.0);

    let clock_w = total_w * unit;
    let clock_h = total_h * unit;
    let left = (w - clock_w) / 2.0;
    let top = (h - clock_h) / 2.0;
    let bottom = top + clock_h;

    let mut tiles = [Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }; TILE_COUNT];
    let mut x = left;
    let mut dots = [[(0.0, 0.0); 2]; 2];
    let big_center_y = top + clock_h / 2.0;
    for pair_index in 0..3 {
        let scale = if pair_index == 2 { s } else { 1.0 };
        let tw = unit * scale;
        let th = unit * TILE_ASPECT * scale;
        for k in 0..2 {
            tiles[pair_index * 2 + k] = Rect {
                x,
                y: bottom - th,
                w: tw,
                h: th,
            };
            x += tw + if k == 0 { PAIR_GAP * unit * scale } else { 0.0 };
        }
        if pair_index < 2 {
            let cx = x + SEPARATOR_SLOT * unit / 2.0;
            let dy = DOT_OFFSET * TILE_ASPECT * unit;
            dots[pair_index] = [(cx, big_center_y - dy), (cx, big_center_y + dy)];
            x += SEPARATOR_SLOT * unit;
        }
    }

    let clock = Rect {
        x: left,
        y: top,
        w: clock_w,
        h: clock_h,
    };
    let date = input.date_visible.then(|| {
        let space = h - bottom;
        let height = (DATE_HEIGHT_FRACTION * h * input.date_size).min(space * 0.7);
        let anchor_x = match input.date_position {
            DatePosition::Left => clock.x,
            DatePosition::Center => w / 2.0,
            DatePosition::Right => clock.right(),
        };
        DateLayout {
            anchor_x,
            center_y: bottom + space / 2.0,
            height,
            position: input.date_position,
        }
    });

    ClockLayout {
        unit,
        tiles,
        dots,
        dot_radius: DOT_RADIUS * unit,
        clock,
        date: date.filter(|d| d.height >= 4.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(width: f32, height: f32, margin: f32) -> LayoutInput {
        LayoutInput {
            width,
            height,
            margin,
            seconds_scale: 1.0,
            date_visible: true,
            date_size: 1.0,
            date_position: DatePosition::Center,
        }
    }

    #[test]
    fn clock_fits_inside_the_margin() {
        let l = compute(&input(1920.0, 1080.0, 250.0));
        assert!(l.clock.x >= 250.0 - 0.01);
        assert!(l.clock.right() <= 1920.0 - 250.0 + 0.01);
        assert!(l.clock.y >= 250.0 - 0.01);
        assert!(l.clock.bottom() <= 1080.0 - 250.0 + 0.01);
    }

    #[test]
    fn margin_shrinks_so_the_clock_keeps_forty_percent() {
        assert_eq!(effective_margin(800.0, 450.0, 500.0), 0.3 * 450.0);
        let l = compute(&input(800.0, 450.0, 500.0));
        assert!(
            l.clock.w >= 0.4 * 800.0 - 0.01 || l.clock.h >= 0.4 * 450.0 - 0.01,
            "{:?}",
            l.clock
        );
    }

    #[test]
    fn seconds_tiles_are_scaled_and_bottom_aligned() {
        let mut i = input(1600.0, 900.0, 100.0);
        i.seconds_scale = 0.5;
        let l = compute(&i);
        assert!((l.tiles[4].w - l.tiles[0].w * 0.5).abs() < 0.01);
        assert!((l.tiles[4].bottom() - l.tiles[0].bottom()).abs() < 0.01);
        assert!((l.tiles[5].bottom() - l.tiles[0].bottom()).abs() < 0.01);
        assert!(l.tiles[4].y > l.tiles[0].y);
    }

    #[test]
    fn second_separator_stays_at_hour_minute_size() {
        let mut i = input(1600.0, 900.0, 100.0);
        i.seconds_scale = 0.4;
        let l = compute(&i);
        assert_eq!(l.dots[0][0].1, l.dots[1][0].1);
        assert_eq!(l.dots[0][1].1, l.dots[1][1].1);
    }

    #[test]
    fn tiles_are_ordered_left_to_right_without_overlap() {
        let l = compute(&input(1280.0, 720.0, 50.0));
        for pair in l.tiles.windows(2) {
            assert!(pair[0].right() < pair[1].x);
        }
        assert!((l.tiles[5].right() - l.clock.right()).abs() < 0.01);
    }

    #[test]
    fn date_sits_below_the_clock_inside_the_window() {
        let l = compute(&input(1280.0, 720.0, 150.0));
        let d = l.date.unwrap();
        assert!(d.center_y - d.height / 2.0 >= l.clock.bottom());
        assert!(d.center_y + d.height / 2.0 <= 720.0);
        assert_eq!(d.anchor_x, 640.0);
    }

    #[test]
    fn date_aligns_with_the_clock_edges() {
        let mut i = input(1280.0, 720.0, 150.0);
        i.date_position = DatePosition::Left;
        let l = compute(&i);
        assert_eq!(l.date.unwrap().anchor_x, l.clock.x);
        i.date_position = DatePosition::Right;
        let l = compute(&i);
        assert_eq!(l.date.unwrap().anchor_x, l.clock.right());
    }

    #[test]
    fn hidden_date_has_no_layout() {
        let mut i = input(1280.0, 720.0, 150.0);
        i.date_visible = false;
        assert!(compute(&i).date.is_none());
    }
}
