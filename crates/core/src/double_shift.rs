//! MARK: - Double Shift Detector
//!
//! Double Shift detector state machine: the
//! event tap feeds it the modifier flags of every `flagsChanged` event and a note for every key
//! down, tests feed it directly. No allocations - it runs inside the event tap callback.

/// Max time between the two Shift releases (macOS: doubleTapInterval = 0.4 s).
pub const DOUBLE_TAP_INTERVAL_MS: u64 = 400;

/// Device-independent modifier flags, like `NSEvent.ModifierFlags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierFlags(u8);

impl ModifierFlags {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const OPTION: Self = Self(4);
    pub const COMMAND: Self = Self(8);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ModifierFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ModifierFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Debug, Default)]
pub struct DoubleShiftDetector {
    // Double-tap detection state
    last_shift_press_time: Option<u64>,
    shift_was_alone_down: bool,
}

impl DoubleShiftDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Any key was pressed: a Shift press with a key in between is not a "clean" tap. The timer is
    /// intentionally NOT reset - same as the macOS app (keyDown monitor).
    pub fn on_key_down(&mut self) {
        self.shift_was_alone_down = false;
    }

    /// Feed the flags of one `flagsChanged` event. Returns true when a double Shift has just been
    /// detected.
    pub fn on_flags_changed(&mut self, flags: ModifierFlags, timestamp_ms: u64) -> bool {
        let shift_down = flags.contains(ModifierFlags::SHIFT);
        // Only Shift pressed, no other modifiers
        let only_shift = shift_down
            && !flags.intersects(
                ModifierFlags::COMMAND | ModifierFlags::OPTION | ModifierFlags::CONTROL,
            );

        if only_shift && !self.shift_was_alone_down {
            // Shift just pressed down (alone)
            self.shift_was_alone_down = true;
        } else if !shift_down && self.shift_was_alone_down {
            // Shift just released, and it was a "clean" press (no other keys pressed during)
            self.shift_was_alone_down = false;
            if let Some(last) = self.last_shift_press_time
                && timestamp_ms.saturating_sub(last) < DOUBLE_TAP_INTERVAL_MS
            {
                // Double Shift detected!
                self.last_shift_press_time = None;
                return true;
            }
            self.last_shift_press_time = Some(timestamp_ms);
        } else if !only_shift {
            // Some other modifier involved - reset
            self.shift_was_alone_down = false;
        }
        false
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    //! Tests driven by modifier flags.

    use super::*;

    const SHIFT: ModifierFlags = ModifierFlags::SHIFT;
    const NONE: ModifierFlags = ModifierFlags::NONE;
    const CONTROL: ModifierFlags = ModifierFlags::CONTROL;

    /// Press and release Shift; true if the press or the release fired.
    fn tap(detector: &mut DoubleShiftDetector, down_at: u64, up_at: u64) -> bool {
        let down = detector.on_flags_changed(SHIFT, down_at);
        let up = detector.on_flags_changed(NONE, up_at);
        down | up
    }

    #[test]
    fn two_quick_taps_fire() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        assert!(tap(&mut d, 1200, 1250));
    }

    #[test]
    fn taps_too_far_apart_do_not_fire_but_second_tap_starts_new_sequence() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        assert!(!tap(&mut d, 1500, 1600)); // 550 ms after the first release
        assert!(tap(&mut d, 1700, 1750)); // 150 ms after the second release
    }

    #[test]
    fn interval_is_measured_between_releases() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        assert!(!tap(&mut d, 1100, 1450)); // released exactly 400 ms later - not "< 400"
        assert!(tap(&mut d, 1500, 1840)); // 390 ms after the previous release
    }

    #[test]
    fn typing_capital_letters_does_not_fire() {
        // Shift held while typing "H" is not a clean Shift press
        let mut d = DoubleShiftDetector::new();
        d.on_flags_changed(SHIFT, 1000);
        d.on_key_down();
        assert!(!d.on_flags_changed(NONE, 1060));

        d.on_flags_changed(SHIFT, 1100);
        d.on_key_down();
        assert!(!d.on_flags_changed(NONE, 1160));
    }

    #[test]
    fn ctrl_shift_does_not_fire() {
        let mut d = DoubleShiftDetector::new();
        for t in [1000, 1200] {
            d.on_flags_changed(CONTROL, t);
            d.on_flags_changed(CONTROL | SHIFT, t + 10);
            assert!(!d.on_flags_changed(CONTROL, t + 20));
            d.on_flags_changed(NONE, t + 30);
        }
    }

    #[test]
    fn option_and_command_with_shift_do_not_fire() {
        for other in [ModifierFlags::OPTION, ModifierFlags::COMMAND] {
            let mut d = DoubleShiftDetector::new();
            for t in [1000, 1200] {
                d.on_flags_changed(other | SHIFT, t);
                assert!(!d.on_flags_changed(other, t + 20));
                d.on_flags_changed(NONE, t + 30);
            }
        }
    }

    #[test]
    fn both_shift_keys_count_as_one_tap_until_both_released() {
        // macOS reports the same flags while the second Shift is pressed and while one is released
        let mut d = DoubleShiftDetector::new();
        d.on_flags_changed(SHIFT, 1000); // left down
        d.on_flags_changed(SHIFT, 1010); // right down
        assert!(!d.on_flags_changed(SHIFT, 1020)); // right up, left still held
        assert!(!d.on_flags_changed(NONE, 1030)); // first tap
        assert!(tap(&mut d, 1100, 1150)); // second tap
    }

    #[test]
    fn repeated_flags_events_are_ignored() {
        let mut d = DoubleShiftDetector::new();
        d.on_flags_changed(SHIFT, 1000);
        d.on_flags_changed(SHIFT, 1030);
        d.on_flags_changed(SHIFT, 1060);
        assert!(!d.on_flags_changed(NONE, 1090));
        assert!(tap(&mut d, 1200, 1250));
    }

    #[test]
    fn triple_tap_fires_once_fourth_tap_fires_again() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        assert!(tap(&mut d, 1100, 1150));
        assert!(!tap(&mut d, 1200, 1250)); // sequence was reset after firing
        assert!(tap(&mut d, 1300, 1350));
    }

    // Another key between the taps resets only "shift alone", not the timer.
    #[test]
    fn other_key_between_taps_does_not_reset_timer() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        d.on_key_down();
        assert!(tap(&mut d, 1200, 1250));
    }

    // Releasing Ctrl while Shift is still held makes the Shift press "alone" again.
    #[test]
    fn releasing_other_modifier_while_shift_held_counts_as_clean_tap() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        d.on_flags_changed(SHIFT, 1100);
        d.on_flags_changed(SHIFT | CONTROL, 1110);
        d.on_flags_changed(SHIFT, 1120);
        assert!(d.on_flags_changed(NONE, 1130));
    }

    #[test]
    fn a_key_pressed_while_shift_is_held_cancels_that_tap() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        d.on_flags_changed(SHIFT, 1100);
        d.on_key_down();
        assert!(!d.on_flags_changed(NONE, 1130));
        // The timer still holds the first release (1050), the cancelled tap did not move it
        assert!(tap(&mut d, 1200, 1250));
    }

    #[test]
    fn reset_clears_state() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 1000, 1050));
        d.reset();
        assert!(!tap(&mut d, 1100, 1150));
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_fire() {
        let mut d = DoubleShiftDetector::new();
        assert!(!tap(&mut d, 5000, 5050));
        // saturating_sub gives 0 < 400: a stale timestamp counts as "quick", never underflows
        assert!(tap(&mut d, 100, 150));
    }

    #[test]
    fn flags_combine_and_test() {
        let flags = SHIFT | CONTROL;
        assert!(flags.contains(SHIFT) && flags.contains(CONTROL));
        assert!(!flags.contains(ModifierFlags::OPTION));
        assert!(flags.intersects(CONTROL | ModifierFlags::COMMAND));
        assert!(!NONE.intersects(SHIFT));
        let mut combined = NONE;
        combined |= ModifierFlags::COMMAND;
        assert_eq!(combined, ModifierFlags::COMMAND);
    }
}
