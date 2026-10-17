//! Flip, roll and cascade planning for the six tiles.
//!
//! Time is a monotonic `f64` in seconds supplied by the caller, so the whole
//! state machine is deterministic and testable.

use crate::digits::{TILE_COUNT, next_digit};

/// Duration of an intermediate flip during a roll, in seconds.
pub const ROLL_FLIP_DURATION: f64 = 0.07;

/// Safety cap on state transitions per tile per update.
const MAX_STEPS_PER_UPDATE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSettings {
    /// Duration of a final flip, in seconds.
    pub flip_duration: f64,
    /// Delay between the starts of consecutive tiles in a cascade, in seconds.
    pub cascade_delay: f64,
    /// Bounce strength, 0 (none) to 1 (strong).
    pub bounce: f32,
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            flip_duration: 0.45,
            cascade_delay: 0.06,
            bounce: 0.5,
        }
    }
}

/// Fraction of a flip's duration spent falling before the flap first lands.
pub fn fall_fraction(bounce: f32) -> f64 {
    1.0 - 0.4 * f64::from(bounce.clamp(0.0, 1.0))
}

/// Rotation of the top flap as a fraction of 180°, at normalized flip time `t` (0..=1).
///
/// The flap falls with increasing speed, lands at 1.0, then (with bounce) lifts
/// slightly twice before settling. The result never exceeds 1.0.
pub fn flap_angle(t: f64, bounce: f32) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let fall = fall_fraction(bounce);
    if t < fall {
        let u = t / fall;
        return u * u;
    }
    if bounce <= 0.0 || fall >= 1.0 {
        return 1.0;
    }
    let v = (t - fall) / (1.0 - fall);
    let amplitude = 0.12 * f64::from(bounce.clamp(0.0, 1.0));
    1.0 - amplitude * (std::f64::consts::TAU * v).sin().abs() * (1.0 - v)
}

#[derive(Debug, Clone, Copy)]
struct Flip {
    to: u8,
    start: f64,
    duration: f64,
    bounce: f32,
    landed: bool,
}

impl Flip {
    fn impact(&self) -> f64 {
        self.start + self.duration * fall_fraction(self.bounce)
    }

    fn end(&self) -> f64 {
        self.start + self.duration
    }
}

#[derive(Debug, Clone, Copy)]
struct Tile {
    shown: u8,
    target: u8,
    flip: Option<Flip>,
    pending_start: Option<f64>,
    test_flip: bool,
}

impl Tile {
    fn at(digit: u8) -> Self {
        Self {
            shown: digit,
            target: digit,
            flip: None,
            pending_start: None,
            test_flip: false,
        }
    }

    fn is_busy(&self) -> bool {
        self.flip.is_some() || self.pending_start.is_some()
    }
}

/// What the renderer needs to draw one tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileVisual {
    /// Digit on the static bottom half and on the front of the moving flap.
    pub current: u8,
    /// Digit on the static top half and on the back of the moving flap.
    pub next: u8,
    /// Flap rotation as a fraction of 180°; 0 when no flip is in progress.
    pub angle: f32,
    /// Whether a flip is in progress.
    pub flipping: bool,
}

/// The motion state of the whole clock face.
#[derive(Debug, Clone)]
pub struct Motion {
    tiles: [Tile; TILE_COUNT],
    settings: MotionSettings,
}

impl Motion {
    pub fn new(digits: [u8; TILE_COUNT], settings: MotionSettings) -> Self {
        Self {
            tiles: digits.map(Tile::at),
            settings,
        }
    }

    pub fn set_settings(&mut self, settings: MotionSettings) {
        self.settings = settings;
    }

    /// Jump to `digits` immediately, discarding any animation.
    pub fn snap(&mut self, digits: [u8; TILE_COUNT]) {
        self.tiles = digits.map(Tile::at);
    }

    /// Set the digits the tiles must reach. Idle tiles that change start a cascade
    /// right to left; tiles already moving keep going towards the new target.
    pub fn set_target(&mut self, digits: [u8; TILE_COUNT], now: f64) {
        let mut rank = 0.0;
        for i in (0..TILE_COUNT).rev() {
            let tile = &mut self.tiles[i];
            if tile.target == digits[i] {
                continue;
            }
            tile.target = digits[i];
            if tile.is_busy() {
                continue;
            }
            tile.pending_start = Some(now + rank * self.settings.cascade_delay);
            rank += 1.0;
        }
    }

    /// Make every idle tile perform one flip that keeps its digit, as a cascade.
    pub fn test_flip(&mut self, now: f64) {
        let mut rank = 0.0;
        for tile in self.tiles.iter_mut().rev() {
            if tile.is_busy() {
                continue;
            }
            tile.pending_start = Some(now + rank * self.settings.cascade_delay);
            tile.test_flip = true;
            rank += 1.0;
        }
    }

    /// Advance to `now`. Returns how many flaps landed since the last update.
    pub fn update(&mut self, now: f64) -> u32 {
        let settings = self.settings;
        let mut landed = 0;
        for (i, tile) in self.tiles.iter_mut().enumerate() {
            for _ in 0..MAX_STEPS_PER_UPDATE {
                if let Some(flip) = tile.flip.as_mut() {
                    if !flip.landed && now >= flip.impact() {
                        flip.landed = true;
                        landed += 1;
                    }
                    if now < flip.end() {
                        break;
                    }
                    let end = flip.end();
                    tile.shown = flip.to;
                    tile.flip = None;
                    if tile.shown == tile.target {
                        break;
                    }
                    tile.flip = Some(start_flip(i, tile, end, &settings));
                } else if let Some(start) = tile.pending_start {
                    if now < start {
                        break;
                    }
                    tile.pending_start = None;
                    if std::mem::take(&mut tile.test_flip) {
                        tile.flip = Some(Flip {
                            to: tile.shown,
                            start,
                            duration: settings.flip_duration,
                            bounce: settings.bounce,
                            landed: false,
                        });
                    } else if tile.shown != tile.target {
                        tile.flip = Some(start_flip(i, tile, start, &settings));
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        landed
    }

    /// Whether any tile is flipping or waiting to start.
    pub fn is_animating(&self) -> bool {
        self.tiles.iter().any(Tile::is_busy)
    }

    /// The digits currently fully shown by each tile.
    pub fn shown(&self) -> [u8; TILE_COUNT] {
        self.tiles.map(|t| t.shown)
    }

    pub fn visual(&self, tile: usize, now: f64) -> TileVisual {
        let t = &self.tiles[tile];
        match t.flip {
            Some(flip) => {
                let progress = ((now - flip.start) / flip.duration).clamp(0.0, 1.0);
                TileVisual {
                    current: t.shown,
                    next: flip.to,
                    angle: flap_angle(progress, flip.bounce) as f32,
                    flipping: true,
                }
            }
            None => TileVisual {
                current: t.shown,
                next: t.shown,
                angle: 0.0,
                flipping: false,
            },
        }
    }
}

/// Start the next flip of a tile towards its target: intermediate flips of a roll
/// are short and bounce-free, the final flip uses the configured duration and bounce.
fn start_flip(index: usize, tile: &Tile, start: f64, settings: &MotionSettings) -> Flip {
    let to = next_digit(index, tile.shown);
    let is_final = to == tile.target;
    Flip {
        to,
        start,
        duration: if is_final {
            settings.flip_duration
        } else {
            ROLL_FLIP_DURATION.min(settings.flip_duration)
        },
        bounce: if is_final { settings.bounce } else { 0.0 },
        landed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digits::digits_of;

    const SETTINGS: MotionSettings = MotionSettings {
        flip_duration: 0.45,
        cascade_delay: 0.06,
        bounce: 0.5,
    };

    /// Step the simulation in small increments, returning total ticks.
    fn run(motion: &mut Motion, from: f64, to: f64) -> u32 {
        let mut ticks = 0;
        let mut t = from;
        while t <= to {
            ticks += motion.update(t);
            t += 0.001;
        }
        ticks
    }

    #[test]
    fn a_second_change_flips_only_the_seconds_units_tile_with_one_tick() {
        let mut m = Motion::new(digits_of(10, 0, 0), SETTINGS);
        m.set_target(digits_of(10, 0, 1), 0.0);
        assert!(m.is_animating());
        assert_eq!(run(&mut m, 0.0, 1.0), 1);
        assert_eq!(m.shown(), digits_of(10, 0, 1));
        assert!(!m.is_animating());
    }

    #[test]
    fn the_tick_comes_when_the_flap_lands_not_when_the_flip_ends() {
        let mut m = Motion::new(digits_of(10, 0, 0), SETTINGS);
        m.set_target(digits_of(10, 0, 1), 0.0);
        let impact = SETTINGS.flip_duration * fall_fraction(SETTINGS.bounce);
        assert_eq!(m.update(impact - 0.001), 0);
        assert_eq!(m.update(impact), 1);
        assert_eq!(m.shown(), digits_of(10, 0, 0), "still bouncing");
        assert_eq!(m.update(SETTINGS.flip_duration), 0);
        assert_eq!(m.shown(), digits_of(10, 0, 1));
    }

    #[test]
    fn six_changes_cascade_right_to_left_with_six_ticks() {
        let mut m = Motion::new(digits_of(9, 59, 59), SETTINGS);
        m.set_target(digits_of(10, 0, 0), 0.0);
        m.update(0.0);
        assert!(m.visual(5, 0.0).flipping);
        assert!(
            !m.visual(4, 0.0).flipping,
            "second tile waits for the cascade delay"
        );
        m.update(0.06);
        assert!(m.visual(4, 0.06).flipping);
        m.update(0.299);
        assert!(!m.visual(0, 0.299).flipping);
        m.update(0.3);
        assert!(
            m.visual(0, 0.3).flipping,
            "leftmost tile starts after 5 delays"
        );
        assert_eq!(run(&mut m, 0.3, 2.0), 6);
        assert_eq!(m.shown(), digits_of(10, 0, 0));
    }

    #[test]
    fn midnight_rolls_the_hours_units_through_seven_flips() {
        let mut m = Motion::new(digits_of(23, 59, 59), SETTINGS);
        m.set_target(digits_of(0, 0, 0), 0.0);
        // Hours tens 2 -> 0 is one flip; hours units 3 -> 0 is seven; four more.
        assert_eq!(run(&mut m, 0.0, 3.0), 12);
        assert_eq!(m.shown(), digits_of(0, 0, 0));
    }

    #[test]
    fn intermediate_flips_of_a_roll_are_short_and_the_last_one_is_normal() {
        let mut m = Motion::new([0, 0, 0, 0, 0, 0], SETTINGS);
        m.set_target([0, 0, 0, 0, 0, 3], 0.0);
        m.update(0.0);
        m.update(ROLL_FLIP_DURATION);
        assert_eq!(m.shown()[5], 1, "first intermediate flip already done");
        m.update(2.0 * ROLL_FLIP_DURATION);
        assert_eq!(m.shown()[5], 2);
        m.update(2.0 * ROLL_FLIP_DURATION + SETTINGS.flip_duration - 0.01);
        assert_eq!(m.shown()[5], 2, "final flip uses the full duration");
        m.update(2.0 * ROLL_FLIP_DURATION + SETTINGS.flip_duration);
        assert_eq!(m.shown()[5], 3);
    }

    #[test]
    fn a_new_target_during_a_roll_continues_without_restarting() {
        let mut m = Motion::new([0, 0, 0, 0, 0, 0], SETTINGS);
        m.set_target([0, 0, 0, 0, 0, 5], 0.0);
        let mut ticks = run(&mut m, 0.0, 0.15);
        let reached = m.shown()[5];
        assert!(reached >= 1);
        m.set_target([0, 0, 0, 0, 0, 6], 0.15);
        ticks += run(&mut m, 0.151, 3.0);
        assert_eq!(m.shown()[5], 6);
        assert_eq!(ticks, 6, "one flip per digit from 0 to 6, none repeated");
    }

    #[test]
    fn test_flip_keeps_the_time_and_ticks_once_per_tile() {
        let digits = digits_of(12, 34, 56);
        let mut m = Motion::new(digits, SETTINGS);
        m.test_flip(0.0);
        let v = m.visual(5, 0.0);
        assert_eq!(v.current, v.next);
        assert_eq!(run(&mut m, 0.0, 2.0), 6);
        assert_eq!(m.shown(), digits);
    }

    #[test]
    fn snap_jumps_without_animation_or_ticks() {
        let mut m = Motion::new(digits_of(8, 0, 0), SETTINGS);
        m.set_target(digits_of(8, 0, 1), 0.0);
        m.snap(digits_of(14, 40, 22));
        assert!(!m.is_animating());
        assert_eq!(m.update(5.0), 0);
        assert_eq!(m.shown(), digits_of(14, 40, 22));
    }

    #[test]
    fn flap_angle_falls_lands_and_never_overshoots() {
        assert_eq!(flap_angle(0.0, 0.5), 0.0);
        assert_eq!(flap_angle(fall_fraction(0.5), 0.5), 1.0);
        assert_eq!(flap_angle(1.0, 0.5), 1.0);
        for i in 0..=100 {
            let a = flap_angle(f64::from(i) / 100.0, 1.0);
            assert!((0.0..=1.0).contains(&a));
        }
    }

    #[test]
    fn without_bounce_the_flap_lands_at_the_end() {
        assert_eq!(fall_fraction(0.0), 1.0);
        assert!(flap_angle(0.99, 0.0) < 1.0);
    }
}
