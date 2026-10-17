//! Digit sequences of the six tiles.

/// Number of tiles on the clock face.
pub const TILE_COUNT: usize = 6;

/// Length of each tile's cyclic digit sequence, left to right:
/// hours tens, hours units, minutes tens, minutes units, seconds tens, seconds units.
pub const SEQUENCE_LEN: [u8; TILE_COUNT] = [3, 10, 6, 10, 6, 10];

/// The digit shown by each tile for a given time of day.
pub fn digits_of(hour: u32, minute: u32, second: u32) -> [u8; TILE_COUNT] {
    let h = (hour % 24) as u8;
    let m = (minute % 60) as u8;
    let s = (second % 60) as u8;
    [h / 10, h % 10, m / 10, m % 10, s / 10, s % 10]
}

/// The digit that follows `digit` on tile `tile` (tiles only ever move forward).
pub fn next_digit(tile: usize, digit: u8) -> u8 {
    (digit + 1) % SEQUENCE_LEN[tile]
}

/// How many forward flips tile `tile` needs to go from `from` to `to`.
pub fn forward_steps(tile: usize, from: u8, to: u8) -> u8 {
    let len = SEQUENCE_LEN[tile];
    (to % len + len - from % len) % len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_of_splits_time_into_six_tiles() {
        assert_eq!(digits_of(9, 5, 37), [0, 9, 0, 5, 3, 7]);
        assert_eq!(digits_of(23, 59, 59), [2, 3, 5, 9, 5, 9]);
    }

    #[test]
    fn next_digit_wraps_on_each_tiles_own_sequence() {
        assert_eq!(next_digit(0, 2), 0);
        assert_eq!(next_digit(1, 9), 0);
        assert_eq!(next_digit(2, 5), 0);
        assert_eq!(next_digit(5, 3), 4);
    }

    #[test]
    fn hours_units_roll_forward_seven_flips_at_midnight() {
        assert_eq!(forward_steps(1, 3, 0), 7);
    }

    #[test]
    fn going_back_one_hour_rolls_nine_flips_forward() {
        assert_eq!(forward_steps(1, 3, 2), 9);
    }

    #[test]
    fn same_digit_needs_no_flip() {
        assert_eq!(forward_steps(4, 4, 4), 0);
    }
}
