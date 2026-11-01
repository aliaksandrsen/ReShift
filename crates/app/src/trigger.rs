//! MARK: - Trigger logic
//!
//! What the event tap decides for every key event: does it fire the conversion, and is the event
//! swallowed. Pure and allocation-free (it runs inside the tap callback), so it is tested with plain
//! values:
//! - the custom hotkey is swallowed;
//! - only the four real modifiers are compared (ignoring caps lock and fn);
//! - a held hotkey does not fire again with every key repeat.

use reshift_core::double_shift::{DoubleShiftDetector, ModifierFlags};

use crate::hotkey_format::HotkeyModifiers;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    DoubleShift,
    Hotkey,
}

/// Which shortcut is armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyConfig {
    /// true = double Shift, false = the custom key below.
    pub use_double_shift: bool,
    pub key_code: u16,
    pub modifiers: HotkeyModifiers,
}

impl HotkeyConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            use_double_shift: settings.use_double_shift,
            key_code: settings.hotkey_key_code,
            modifiers: settings.hotkey_modifiers,
        }
    }

    /// One word, so the main thread can hand a new config to the tap thread with a single atomic store.
    pub fn pack(self) -> u64 {
        u64::from(self.use_double_shift)
            | (u64::from(self.key_code) << 8)
            | (u64::from(self.modifiers.sanitized().0) << 32)
    }

    pub fn unpack(bits: u64) -> Self {
        Self {
            use_double_shift: bits & 1 != 0,
            key_code: ((bits >> 8) & 0xFFFF) as u16,
            modifiers: HotkeyModifiers((bits >> 32) as u32),
        }
    }
}

/// How the tap is created. Only a custom hotkey has to swallow events, so only it needs an active tap:
/// a listen-only tap can never delay or block the keyboard of the whole session, which makes the default
/// double Shift mode harmless even if this app hangs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapMode {
    ListenOnly,
    Active,
}

pub fn tap_mode(config: HotkeyConfig) -> TapMode {
    if config.use_double_shift {
        TapMode::ListenOnly
    } else {
        TapMode::Active
    }
}

/// A new config can be handed to the running tap unless it needs the other kind of tap.
pub fn needs_new_tap(current: TapMode, config: HotkeyConfig) -> bool {
    current != tap_mode(config)
}

/// What the tap reports about one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    KeyDown {
        key_code: u16,
        /// Raw `CGEventFlags`.
        flags: u64,
        autorepeat: bool,
    },
    KeyUp {
        key_code: u16,
    },
    FlagsChanged {
        /// Raw `CGEventFlags`.
        flags: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Do not deliver the event to the focused app.
    pub swallow: bool,
    pub trigger: Option<Trigger>,
}

impl Outcome {
    const PASS: Self = Self {
        swallow: false,
        trigger: None,
    };
}

#[derive(Debug, Default)]
pub struct TapLogic {
    detector: DoubleShiftDetector,
    /// The config the state below belongs to; a change starts from scratch.
    config: Option<HotkeyConfig>,
    /// The hotkey key whose key down was swallowed: its key up is swallowed too, so no app sees a
    /// release without a press.
    swallowed_key: Option<u16>,
}

impl TapLogic {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, config: HotkeyConfig, event: KeyEvent, now_ms: u64) -> Outcome {
        if self.config != Some(config) {
            self.detector.reset();
            self.swallowed_key = None;
            self.config = Some(config);
        }
        match event {
            KeyEvent::FlagsChanged { flags } => {
                if config.use_double_shift
                    && self
                        .detector
                        .on_flags_changed(detector_flags(flags), now_ms)
                {
                    return Outcome {
                        swallow: false,
                        trigger: Some(Trigger::DoubleShift),
                    };
                }
                Outcome::PASS
            }
            KeyEvent::KeyDown {
                key_code,
                flags,
                autorepeat,
            } => {
                if config.use_double_shift {
                    // Another key between the presses makes the Shift press "not clean"
                    self.detector.on_key_down();
                    return Outcome::PASS;
                }
                if !matches_hotkey(config, key_code, flags) {
                    return Outcome::PASS;
                }
                self.swallowed_key = Some(key_code);
                Outcome {
                    swallow: true,
                    trigger: (!autorepeat).then_some(Trigger::Hotkey),
                }
            }
            KeyEvent::KeyUp { key_code } => {
                if self.swallowed_key == Some(key_code) {
                    self.swallowed_key = None;
                    Outcome {
                        swallow: true,
                        trigger: None,
                    }
                } else {
                    Outcome::PASS
                }
            }
        }
    }
}

fn matches_hotkey(config: HotkeyConfig, key_code: u16, flags: u64) -> bool {
    // A hotkey without Control, Option or Command would swallow plain typing
    config.modifiers.is_valid_for_hotkey()
        && key_code == config.key_code
        && HotkeyModifiers((flags & 0xFFFF_FFFF) as u32).sanitized() == config.modifiers.sanitized()
}

/// The modifier state the detector works with, from raw `CGEventFlags`.
fn detector_flags(flags: u64) -> ModifierFlags {
    let modifiers = HotkeyModifiers((flags & 0xFFFF_FFFF) as u32);
    let mut result = ModifierFlags::NONE;
    for (flag, detector_flag) in [
        (HotkeyModifiers::SHIFT, ModifierFlags::SHIFT),
        (HotkeyModifiers::CONTROL, ModifierFlags::CONTROL),
        (HotkeyModifiers::OPTION, ModifierFlags::OPTION),
        (HotkeyModifiers::COMMAND, ModifierFlags::COMMAND),
    ] {
        if modifiers.contains(flag) {
            result |= detector_flag;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: u64 = 1 << 17;
    const CONTROL: u64 = 1 << 18;
    const OPTION: u64 = 1 << 19;
    const COMMAND: u64 = 1 << 20;
    const CAPS_LOCK: u64 = 1 << 16;
    const FN: u64 = 1 << 23;
    const KEY_S: u16 = 0x01;
    const KEY_F5: u16 = 0x60;

    const DOUBLE_SHIFT: HotkeyConfig = HotkeyConfig {
        use_double_shift: true,
        key_code: KEY_S,
        modifiers: HotkeyModifiers::OPTION,
    };
    const OPTION_S: HotkeyConfig = HotkeyConfig {
        use_double_shift: false,
        key_code: KEY_S,
        modifiers: HotkeyModifiers::OPTION,
    };

    fn down(key_code: u16, flags: u64) -> KeyEvent {
        KeyEvent::KeyDown {
            key_code,
            flags,
            autorepeat: false,
        }
    }

    fn flags(flags: u64) -> KeyEvent {
        KeyEvent::FlagsChanged { flags }
    }

    // MARK: - Double Shift

    #[test]
    fn two_quick_shift_taps_fire_and_nothing_is_swallowed() {
        let mut logic = TapLogic::new();
        let mut run = |event, t| logic.handle(DOUBLE_SHIFT, event, t);
        assert_eq!(run(flags(SHIFT), 1000), Outcome::PASS);
        assert_eq!(run(flags(0), 1050), Outcome::PASS);
        assert_eq!(run(flags(SHIFT), 1200), Outcome::PASS);
        let fired = run(flags(0), 1250);
        assert_eq!(fired.trigger, Some(Trigger::DoubleShift));
        assert!(!fired.swallow, "the Shift release must reach the app");
    }

    #[test]
    fn slow_shift_taps_do_not_fire() {
        let mut logic = TapLogic::new();
        for (event, t) in [(SHIFT, 1000), (0, 1050), (SHIFT, 1600), (0, 1650)] {
            assert_eq!(logic.handle(DOUBLE_SHIFT, flags(event), t).trigger, None);
        }
    }

    #[test]
    fn typing_a_capital_letter_between_shift_presses_cancels_the_tap() {
        let mut logic = TapLogic::new();
        let mut run = |event, t| logic.handle(DOUBLE_SHIFT, event, t);
        run(flags(SHIFT), 1000);
        run(flags(0), 1050);
        run(flags(SHIFT), 1100);
        run(down(0x04, SHIFT), 1120); // "H"
        assert_eq!(run(flags(0), 1150).trigger, None);
    }

    #[test]
    fn caps_lock_and_fn_do_not_disturb_double_shift() {
        let mut logic = TapLogic::new();
        let mut run = |event, t| logic.handle(DOUBLE_SHIFT, event, t);
        // Caps Lock was on the whole time; Fn is held for the second tap
        run(flags(CAPS_LOCK | SHIFT), 1000);
        run(flags(CAPS_LOCK), 1050);
        run(flags(CAPS_LOCK | SHIFT | FN), 1200);
        assert_eq!(
            run(flags(CAPS_LOCK | FN), 1250).trigger,
            Some(Trigger::DoubleShift)
        );
    }

    #[test]
    fn shift_with_another_modifier_does_not_count() {
        for other in [CONTROL, OPTION, COMMAND] {
            let mut logic = TapLogic::new();
            let mut run = |event, t| logic.handle(DOUBLE_SHIFT, event, t);
            for t in [1000, 1200] {
                run(flags(other | SHIFT), t);
                assert_eq!(run(flags(other), t + 20).trigger, None);
                run(flags(0), t + 30);
            }
        }
    }

    #[test]
    fn the_hotkey_key_means_nothing_in_double_shift_mode() {
        let mut logic = TapLogic::new();
        let outcome = logic.handle(DOUBLE_SHIFT, down(KEY_S, OPTION), 1000);
        assert_eq!(outcome, Outcome::PASS, "Option+S must stay a normal key");
    }

    // MARK: - Custom hotkey

    #[test]
    fn the_hotkey_fires_and_is_swallowed() {
        let mut logic = TapLogic::new();
        let outcome = logic.handle(OPTION_S, down(KEY_S, OPTION), 1000);
        assert_eq!(
            outcome,
            Outcome {
                swallow: true,
                trigger: Some(Trigger::Hotkey)
            }
        );
    }

    #[test]
    fn the_key_up_of_a_swallowed_hotkey_is_swallowed_once() {
        let mut logic = TapLogic::new();
        logic.handle(OPTION_S, down(KEY_S, OPTION), 1000);
        let up = logic.handle(OPTION_S, KeyEvent::KeyUp { key_code: KEY_S }, 1050);
        assert_eq!(
            up,
            Outcome {
                swallow: true,
                trigger: None
            }
        );
        // The next key up of the same key (a normal press) belongs to the app
        let later = logic.handle(OPTION_S, KeyEvent::KeyUp { key_code: KEY_S }, 2000);
        assert_eq!(later, Outcome::PASS);
    }

    #[test]
    fn a_key_up_without_a_swallowed_press_passes() {
        let mut logic = TapLogic::new();
        let outcome = logic.handle(OPTION_S, KeyEvent::KeyUp { key_code: KEY_S }, 1000);
        assert_eq!(outcome, Outcome::PASS);
    }

    #[test]
    fn a_held_hotkey_fires_once_but_every_repeat_is_swallowed() {
        let mut logic = TapLogic::new();
        logic.handle(OPTION_S, down(KEY_S, OPTION), 1000);
        let repeat = logic.handle(
            OPTION_S,
            KeyEvent::KeyDown {
                key_code: KEY_S,
                flags: OPTION,
                autorepeat: true,
            },
            1500,
        );
        assert_eq!(
            repeat,
            Outcome {
                swallow: true,
                trigger: None
            }
        );
    }

    #[test]
    fn other_keys_and_other_modifiers_pass() {
        let mut logic = TapLogic::new();
        assert_eq!(
            logic.handle(OPTION_S, down(0x02, OPTION), 1000),
            Outcome::PASS,
            "another key"
        );
        assert_eq!(
            logic.handle(OPTION_S, down(KEY_S, 0), 1000),
            Outcome::PASS,
            "no modifier"
        );
        assert_eq!(
            logic.handle(OPTION_S, down(KEY_S, OPTION | SHIFT), 1000),
            Outcome::PASS,
            "an extra modifier"
        );
        assert_eq!(
            logic.handle(OPTION_S, down(KEY_S, COMMAND), 1000),
            Outcome::PASS,
            "a different modifier"
        );
    }

    #[test]
    fn caps_lock_fn_and_device_bits_do_not_break_the_hotkey() {
        let mut logic = TapLogic::new();
        for extra in [
            CAPS_LOCK,
            FN,
            0x20, /* NX_DEVICELALTKEYMASK */
            CAPS_LOCK | FN | 0x20,
        ] {
            let outcome = logic.handle(OPTION_S, down(KEY_S, OPTION | extra), 1000);
            assert_eq!(
                outcome.trigger,
                Some(Trigger::Hotkey),
                "extra bits {extra:#x}"
            );
        }
    }

    #[test]
    fn a_combined_hotkey_needs_all_its_modifiers() {
        let config = HotkeyConfig {
            use_double_shift: false,
            key_code: KEY_F5,
            modifiers: HotkeyModifiers::CONTROL | HotkeyModifiers::COMMAND,
        };
        let mut logic = TapLogic::new();
        assert_eq!(
            logic.handle(config, down(KEY_F5, CONTROL), 1000),
            Outcome::PASS
        );
        assert_eq!(
            logic.handle(config, down(KEY_F5, COMMAND), 1000),
            Outcome::PASS
        );
        assert_eq!(
            logic
                .handle(config, down(KEY_F5, CONTROL | COMMAND), 1000)
                .trigger,
            Some(Trigger::Hotkey)
        );
    }

    #[test]
    fn shift_in_the_hotkey_is_part_of_the_match() {
        let config = HotkeyConfig {
            use_double_shift: false,
            key_code: KEY_S,
            modifiers: HotkeyModifiers::OPTION | HotkeyModifiers::SHIFT,
        };
        let mut logic = TapLogic::new();
        assert_eq!(
            logic.handle(config, down(KEY_S, OPTION), 1000),
            Outcome::PASS
        );
        assert_eq!(
            logic
                .handle(config, down(KEY_S, OPTION | SHIFT), 1000)
                .trigger,
            Some(Trigger::Hotkey)
        );
    }

    #[test]
    fn a_hotkey_without_a_real_modifier_never_matches() {
        // Shift alone or nothing would swallow normal typing; the settings never produce it, but the
        // tap must not rely on that
        for modifiers in [HotkeyModifiers::SHIFT, HotkeyModifiers(0)] {
            let config = HotkeyConfig {
                use_double_shift: false,
                key_code: KEY_S,
                modifiers,
            };
            let mut logic = TapLogic::new();
            assert_eq!(
                logic.handle(config, down(KEY_S, modifiers.0 as u64), 1000),
                Outcome::PASS
            );
        }
    }

    #[test]
    fn flags_events_do_nothing_in_hotkey_mode() {
        let mut logic = TapLogic::new();
        for (event, t) in [(SHIFT, 1000), (0, 1050), (SHIFT, 1200), (0, 1250)] {
            assert_eq!(logic.handle(OPTION_S, flags(event), t), Outcome::PASS);
        }
    }

    // MARK: - Changing the config

    #[test]
    fn a_new_config_starts_from_scratch() {
        let mut logic = TapLogic::new();
        // First tap under double Shift ...
        logic.handle(DOUBLE_SHIFT, flags(SHIFT), 1000);
        logic.handle(DOUBLE_SHIFT, flags(0), 1050);
        // ... the user switches to the custom hotkey and back: the old half-tap is forgotten
        logic.handle(OPTION_S, down(0x02, 0), 1100);
        logic.handle(DOUBLE_SHIFT, flags(SHIFT), 1150);
        assert_eq!(logic.handle(DOUBLE_SHIFT, flags(0), 1200).trigger, None);
    }

    #[test]
    fn a_swallowed_key_is_forgotten_when_the_hotkey_changes() {
        let mut logic = TapLogic::new();
        logic.handle(OPTION_S, down(KEY_S, OPTION), 1000);
        let other = HotkeyConfig {
            key_code: KEY_F5,
            ..OPTION_S
        };
        // The key up of S belongs to the app now: nobody swallowed its press under the new config
        let up = logic.handle(other, KeyEvent::KeyUp { key_code: KEY_S }, 1100);
        assert_eq!(up, Outcome::PASS);
    }

    // MARK: - Tap mode

    #[test]
    fn double_shift_uses_a_listen_only_tap_and_a_custom_hotkey_an_active_one() {
        assert_eq!(tap_mode(DOUBLE_SHIFT), TapMode::ListenOnly);
        assert_eq!(tap_mode(OPTION_S), TapMode::Active);
    }

    #[test]
    fn only_a_change_of_the_mode_needs_a_new_tap() {
        assert!(!needs_new_tap(TapMode::ListenOnly, DOUBLE_SHIFT));
        assert!(!needs_new_tap(TapMode::Active, OPTION_S));
        assert!(needs_new_tap(TapMode::ListenOnly, OPTION_S));
        assert!(needs_new_tap(TapMode::Active, DOUBLE_SHIFT));
        // Another key or modifier under the same mode just updates the running tap
        let other = HotkeyConfig {
            key_code: KEY_F5,
            modifiers: HotkeyModifiers::COMMAND,
            ..OPTION_S
        };
        assert!(!needs_new_tap(TapMode::Active, other));
    }

    // MARK: - Packing

    #[test]
    fn a_config_survives_packing() {
        for config in [
            DOUBLE_SHIFT,
            OPTION_S,
            HotkeyConfig {
                use_double_shift: false,
                key_code: 0x7F,
                modifiers: HotkeyModifiers::ALL,
            },
            HotkeyConfig {
                use_double_shift: true,
                key_code: u16::MAX,
                modifiers: HotkeyModifiers::COMMAND,
            },
        ] {
            assert_eq!(HotkeyConfig::unpack(config.pack()), config);
        }
    }

    #[test]
    fn packing_drops_modifier_bits_that_are_not_modifiers() {
        let config = HotkeyConfig {
            use_double_shift: false,
            key_code: KEY_S,
            modifiers: HotkeyModifiers(HotkeyModifiers::OPTION.0 | CAPS_LOCK as u32),
        };
        assert_eq!(
            HotkeyConfig::unpack(config.pack()).modifiers,
            HotkeyModifiers::OPTION
        );
    }

    #[test]
    fn the_config_follows_the_settings() {
        let mut settings = Settings::defaults(Vec::new());
        assert_eq!(HotkeyConfig::from_settings(&settings), DOUBLE_SHIFT);
        settings.use_double_shift = false;
        assert_eq!(HotkeyConfig::from_settings(&settings), OPTION_S);
    }

    #[test]
    fn detector_flags_follow_the_four_modifiers() {
        assert_eq!(detector_flags(0), ModifierFlags::NONE);
        assert_eq!(detector_flags(SHIFT), ModifierFlags::SHIFT);
        assert_eq!(
            detector_flags(SHIFT | OPTION | CAPS_LOCK | FN),
            ModifierFlags::SHIFT | ModifierFlags::OPTION
        );
        assert_eq!(
            detector_flags(CONTROL | COMMAND),
            ModifierFlags::CONTROL | ModifierFlags::COMMAND
        );
        // Bits above 32 (none are modifiers) are ignored
        assert_eq!(detector_flags(1 << 40), ModifierFlags::NONE);
    }
}
