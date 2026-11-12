//! Countdown timers and the countdown screen layout.
//!
//! Times are monotonic `f64` seconds supplied by the caller, as in `motion`.
//! The remaining time is always derived from the end instant, so it never
//! drifts however late the caller polls.

use crate::digits::TILE_COUNT;
use crate::layout::{self, ClockLayout, LayoutInput};

/// The longest countdown, 99:59:59, in seconds.
pub const MAX_SECONDS: u32 = 99 * 3600 + 59 * 60 + 59;

/// Largest digit of each tile while setting a countdown: H H M M S S.
const MAX_DIGIT: [u8; TILE_COUNT] = [9, 9, 5, 9, 5, 9];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CountdownState {
    #[default]
    Idle,
    Running,
    Paused,
    Finished,
}

/// A duration as H H M M S S, hours counting past 24 and capped at 99:59:59.
pub fn digits_of_duration(seconds: u32) -> [u8; TILE_COUNT] {
    let total = seconds.min(MAX_SECONDS);
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    [h / 10, h % 10, m / 10, m % 10, s / 10, s % 10].map(|d| d as u8)
}

/// The duration shown by six countdown digits, in seconds.
pub fn duration_of_digits(d: [u8; TILE_COUNT]) -> u32 {
    let n = |i: usize| u32::from(d[i]);
    (n(0) * 10 + n(1)) * 3600 + (n(2) * 10 + n(3)) * 60 + n(4) * 10 + n(5)
}

/// Step one tile up or down, wrapping within that tile's range without carrying.
pub fn step_digit(digits: [u8; TILE_COUNT], index: usize, delta: i32) -> [u8; TILE_COUNT] {
    let mut next = digits;
    let range = i32::from(MAX_DIGIT[index]) + 1;
    next[index] = (i32::from(digits[index]) + delta).rem_euclid(range) as u8;
    next
}

/// One countdown timer.
#[derive(Debug, Clone, Default)]
pub struct Countdown {
    pub description: String,
    /// The value set with the arrows, in seconds; Reset returns to it.
    duration: u32,
    state: CountdownState,
    end: f64,
    remaining_when_paused: f64,
}

impl Countdown {
    pub fn duration(&self) -> u32 {
        self.duration
    }

    pub fn state(&self) -> CountdownState {
        self.state
    }

    /// Digits shown while idle, edited one tile at a time.
    pub fn set_digits(&self) -> [u8; TILE_COUNT] {
        digits_of_duration(self.duration)
    }

    pub fn set_duration(&mut self, seconds: u32) {
        if self.state == CountdownState::Idle {
            self.duration = seconds.min(MAX_SECONDS);
        }
    }

    pub fn step_digit(&mut self, index: usize, delta: i32) {
        if self.state == CountdownState::Idle {
            self.set_duration(duration_of_digits(step_digit(
                self.set_digits(),
                index,
                delta,
            )));
        }
    }

    pub fn can_start(&self) -> bool {
        self.state == CountdownState::Idle && self.duration > 0
    }

    pub fn start(&mut self, now: f64) {
        if self.can_start() {
            self.end = now + f64::from(self.duration);
            self.state = CountdownState::Running;
        }
    }

    pub fn pause(&mut self, now: f64) {
        if self.state == CountdownState::Running {
            self.remaining_when_paused = self.remaining(now);
            self.state = CountdownState::Paused;
        }
    }

    pub fn resume(&mut self, now: f64) {
        if self.state == CountdownState::Paused {
            self.end = now + self.remaining_when_paused;
            self.state = CountdownState::Running;
        }
    }

    /// Back to the value that was set, ready to start again.
    pub fn reset(&mut self) {
        self.state = CountdownState::Idle;
    }

    /// Remaining time in seconds.
    pub fn remaining(&self, now: f64) -> f64 {
        match self.state {
            CountdownState::Idle => f64::from(self.duration),
            CountdownState::Paused => self.remaining_when_paused,
            CountdownState::Finished => 0.0,
            CountdownState::Running => (self.end - now).max(0.0),
        }
    }

    /// The displayed digits. Seconds round up, so 00:00:00 appears exactly at the end.
    pub fn digits(&self, now: f64) -> [u8; TILE_COUNT] {
        digits_of_duration((self.remaining(now) - 1e-6).ceil().max(0.0) as u32)
    }

    /// Move a running countdown to Finished once its time is up. Returns true on that transition.
    pub fn update(&mut self, now: f64) -> bool {
        if self.state != CountdownState::Running || now < self.end {
            return false;
        }
        self.state = CountdownState::Finished;
        true
    }

    /// When the displayed digits next change (or the countdown ends), or `None` if they are static.
    pub fn next_change(&self, now: f64) -> Option<f64> {
        if self.state != CountdownState::Running {
            return None;
        }
        let left = self.end - now;
        if left <= 0.0 {
            return Some(now);
        }
        let into_second = left - left.floor();
        Some(now + if into_second < 1e-9 { 1.0 } else { into_second })
    }
}

/// All countdowns of the session. Every one keeps counting and can finish in
/// the background, whichever is on screen.
#[derive(Debug, Clone, Default)]
pub struct Countdowns {
    items: Vec<Countdown>,
}

impl Countdowns {
    /// Add an idle countdown and return its index.
    pub fn add(&mut self) -> usize {
        self.items.push(Countdown::default());
        self.items.len() - 1
    }

    pub fn get(&self, index: usize) -> &Countdown {
        &self.items[index]
    }

    pub fn get_mut(&mut self, index: usize) -> &mut Countdown {
        &mut self.items[index]
    }

    /// Advance every countdown; returns the indices of those that finished on this call.
    pub fn update(&mut self, now: f64) -> Vec<usize> {
        self.items
            .iter_mut()
            .enumerate()
            .filter_map(|(i, c)| c.update(now).then_some(i))
            .collect()
    }

    /// The earliest instant any running countdown changes its display.
    pub fn next_change(&self, now: f64) -> Option<f64> {
        self.items
            .iter()
            .filter_map(|c| c.next_change(now))
            .min_by(f64::total_cmp)
    }
}

/// Height of the Start / Pause / Reset buttons, in logical pixels.
pub const CONTROLS_HEIGHT: f32 = 40.0;

/// The countdown screen: the clock with room above it for the description and
/// the up arrows, and below it for the down arrows and the controls.
#[derive(Debug, Clone, PartialEq)]
pub struct CountdownLayout {
    pub clock: ClockLayout,
    /// The margin used, at least the user's.
    pub margin: f32,
    pub arrow_size: f32,
    /// False in windows too small for the arrows; they are then hidden.
    pub arrows_fit: bool,
    pub text_size: f32,
    pub description_center_y: f32,
    pub controls_center_y: f32,
}

impl CountdownLayout {
    /// Height of the band holding the arrows above and below the tiles (0 when hidden).
    pub fn arrow_band(&self) -> f32 {
        if self.arrows_fit {
            self.arrow_size * 1.5
        } else {
            0.0
        }
    }
}

/// The margin grows past the user's setting when needed; when even the largest
/// margin is too thin (tiny windows) the arrows are dropped.
pub fn layout(
    width: f32,
    height: f32,
    margin: f32,
    seconds_scale: f32,
    text_scale: f32,
) -> CountdownLayout {
    let (width, height) = (width.max(1.0), height.max(1.0));
    let short = width.min(height);
    let text_size = (0.035 * short * text_scale).max(8.0);
    let arrow_size = (0.05 * short).clamp(12.0, 40.0);
    let arrow_band = arrow_size * 1.5;
    let controls_band = CONTROLS_HEIGHT + 24.0;

    let need = (text_size * 2.0 + arrow_band).max(controls_band + arrow_band);
    let margin = margin.max(need);
    let clock = layout::compute(&LayoutInput {
        width,
        height,
        margin,
        seconds_scale,
        date_visible: false,
        date_size: 1.0,
        date_position: Default::default(),
    });
    let (top, bottom) = (clock.tiles[0].y, clock.tiles[0].bottom());
    let fit =
        top >= need - 1e-3 && height - bottom >= need - 1e-3 && clock.tiles[5].w >= arrow_size;
    let band = if fit { arrow_band } else { 0.0 };
    CountdownLayout {
        description_center_y: (top - band) / 2.0,
        controls_center_y: (bottom + band + height) / 2.0,
        clock,
        margin,
        arrow_size,
        arrows_fit: fit,
        text_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(seconds: u32) -> Countdown {
        let mut c = Countdown::default();
        c.set_duration(seconds);
        c
    }

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-6)
    }

    #[test]
    fn starts_idle_at_zero_and_cannot_start() {
        let mut c = Countdown::default();
        assert_eq!(c.state(), CountdownState::Idle);
        assert_eq!(c.digits(0.0), [0; 6]);
        assert!(!c.can_start());
        c.start(0.0);
        assert_eq!(c.state(), CountdownState::Idle);
    }

    #[test]
    fn steps_wrap_per_tile_without_carry() {
        let mut c = Countdown::default();
        c.step_digit(5, -1);
        assert_eq!(c.set_digits(), [0, 0, 0, 0, 0, 9]);
        c.step_digit(4, -1);
        assert_eq!(c.set_digits(), [0, 0, 0, 0, 5, 9]);
        c.step_digit(4, 1);
        assert_eq!(c.set_digits(), [0, 0, 0, 0, 0, 9]);
        c.step_digit(0, 1);
        assert_eq!(c.duration(), 10 * 3600 + 9);
    }

    #[test]
    fn counts_down_rounding_seconds_up() {
        let mut c = set(90);
        c.start(1.0);
        assert_eq!(c.digits(1.0), [0, 0, 0, 1, 3, 0]);
        assert_eq!(c.digits(1.999), [0, 0, 0, 1, 3, 0]);
        assert_eq!(c.digits(2.001), [0, 0, 0, 1, 2, 9]);
        assert_eq!(c.digits(90.5), [0, 0, 0, 0, 0, 1]);
        assert_eq!(c.digits(91.0), [0; 6]);
    }

    #[test]
    fn finishes_once_at_the_end() {
        let mut c = set(2);
        c.start(0.0);
        assert!(!c.update(1.999));
        assert!(c.update(2.0));
        assert!(!c.update(2.5));
        assert_eq!(c.state(), CountdownState::Finished);
        assert_eq!(c.digits(3.0), [0; 6]);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let mut c = set(10);
        c.start(0.0);
        c.pause(3.5);
        assert!((c.remaining(100.0) - 6.5).abs() < 1e-9);
        assert_eq!(c.next_change(100.0), None);
        c.resume(100.0);
        assert!((c.remaining(100.0) - 6.5).abs() < 1e-9);
        assert!(c.update(106.5));
    }

    #[test]
    fn reset_returns_to_the_set_value_not_zero() {
        let mut c = set(300);
        c.start(0.0);
        c.pause(60.0);
        c.reset();
        assert_eq!(c.state(), CountdownState::Idle);
        assert_eq!(c.digits(0.0), [0, 0, 0, 5, 0, 0]);
        assert!(c.can_start());
    }

    #[test]
    fn value_cannot_change_while_running() {
        let mut c = set(10);
        c.start(0.0);
        c.step_digit(5, 1);
        c.set_duration(3600);
        assert_eq!(c.duration(), 10);
    }

    #[test]
    fn next_change_lands_on_display_boundaries() {
        let mut c = set(10);
        c.start(0.25);
        assert!(close(c.next_change(0.25), 1.25));
        assert!(close(c.next_change(0.7), 1.25));
        assert!(close(c.next_change(10.25), 10.25));
    }

    #[test]
    fn collection_tracks_the_earliest_change_and_reports_finishers() {
        let mut all = Countdowns::default();
        let a = all.add();
        let b = all.add();
        all.get_mut(a).set_duration(1);
        all.get_mut(b).set_duration(5);
        assert_eq!(all.next_change(0.0), None);
        all.get_mut(a).start(0.0);
        all.get_mut(b).start(0.3);
        assert!(close(all.next_change(0.0), 0.3));
        assert_eq!(all.update(1.0), vec![a]);
        assert!(all.update(1.001).is_empty());
        assert!(close(all.next_change(1.001), 1.3));
    }

    #[test]
    fn durations_past_a_day_keep_their_hours() {
        assert_eq!(digits_of_duration(200 * 3600), [9, 9, 5, 9, 5, 9]);
        assert_eq!(digits_of_duration(26 * 3600 + 61), [2, 6, 0, 1, 0, 1]);
    }

    #[test]
    fn arrows_get_room_even_with_zero_margin() {
        let l = layout(1920.0, 1080.0, 0.0, 1.0, 1.0);
        assert!(l.arrows_fit);
        assert!(l.margin > 0.0);
        let top = l.clock.tiles[0].y;
        assert!(top >= l.arrow_band());
        assert!(l.description_center_y < top - l.arrow_band());
        assert!(
            l.controls_center_y - CONTROLS_HEIGHT / 2.0
                > l.clock.tiles[0].bottom() + l.arrow_band()
        );
    }

    #[test]
    fn larger_user_margin_is_kept() {
        let l = layout(1920.0, 1080.0, 300.0, 1.0, 1.0);
        assert_eq!(l.margin, 300.0);
        assert!(l.arrows_fit);
    }

    #[test]
    fn arrows_are_dropped_when_the_window_is_too_short() {
        assert!(!layout(1600.0, 270.0, 0.0, 1.0, 1.0).arrows_fit);
    }

    #[test]
    fn arrows_are_dropped_when_wider_than_the_seconds_tiles() {
        assert!(!layout(480.0, 1000.0, 0.0, 0.3, 1.0).arrows_fit);
    }
}
