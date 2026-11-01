//! MARK: - Hotkey formatting
//!
//! The custom hotkey is a key code plus modifier flags. `HotkeyModifiers` uses the bit positions
//! that `NSEvent.ModifierFlags` and `CGEventFlags` share for the four real modifiers, so the value
//! can be compared with an event's flags directly.

use std::ops::BitOr;

/// Modifier flags of a hotkey (the device-independent bits of `NSEvent.ModifierFlags`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotkeyModifiers(pub u32);

impl HotkeyModifiers {
    pub const SHIFT: Self = Self(1 << 17);
    pub const CONTROL: Self = Self(1 << 18);
    pub const OPTION: Self = Self(1 << 19);
    pub const COMMAND: Self = Self(1 << 20);
    /// The four modifiers a hotkey may use; caps lock, fn, numeric pad and the like are not part of it.
    pub const ALL: Self = Self(Self::SHIFT.0 | Self::CONTROL.0 | Self::OPTION.0 | Self::COMMAND.0);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Only the four real modifiers; other bits of an event's flags are dropped.
    pub fn sanitized(self) -> Self {
        Self(self.0 & Self::ALL.0)
    }

    /// A hotkey needs Control, Option or Command: with Shift alone every capital letter would trigger it.
    pub fn is_valid_for_hotkey(self) -> bool {
        self.intersects(Self::CONTROL | Self::OPTION | Self::COMMAND)
    }
}

impl BitOr for HotkeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Key code of "S" (virtual key `kVK_ANSI_S`).
pub const DEFAULT_HOTKEY_KEY_CODE: u16 = 0x01;
/// Default modifier is Option.
pub const DEFAULT_HOTKEY_MODIFIERS: HotkeyModifiers = HotkeyModifiers::OPTION;

/// "⌃⌥⇧⌘" in the order macOS shows modifiers.
pub fn modifiers_to_string(modifiers: HotkeyModifiers) -> String {
    [
        (HotkeyModifiers::CONTROL, '⌃'),
        (HotkeyModifiers::OPTION, '⌥'),
        (HotkeyModifiers::SHIFT, '⇧'),
        (HotkeyModifiers::COMMAND, '⌘'),
    ]
    .into_iter()
    .filter(|(flag, _)| modifiers.contains(*flag))
    .map(|(_, symbol)| symbol)
    .collect()
}

/// Key code => label. Codes outside the table show as "Key<code>".
static KEY_NAMES: [(u16, &str); 62] = [
    (0x00, "A"),
    (0x01, "S"),
    (0x02, "D"),
    (0x03, "F"),
    (0x04, "H"),
    (0x05, "G"),
    (0x06, "Z"),
    (0x07, "X"),
    (0x08, "C"),
    (0x09, "V"),
    (0x0B, "B"),
    (0x0C, "Q"),
    (0x0D, "W"),
    (0x0E, "E"),
    (0x0F, "R"),
    (0x10, "Y"),
    (0x11, "T"),
    (0x12, "1"),
    (0x13, "2"),
    (0x14, "3"),
    (0x15, "4"),
    (0x16, "6"),
    (0x17, "5"),
    (0x18, "="),
    (0x19, "9"),
    (0x1A, "7"),
    (0x1B, "-"),
    (0x1C, "8"),
    (0x1D, "0"),
    (0x1E, "]"),
    (0x1F, "O"),
    (0x20, "U"),
    (0x21, "["),
    (0x22, "I"),
    (0x23, "P"),
    (0x25, "L"),
    (0x26, "J"),
    (0x28, "K"),
    (0x2C, "/"),
    (0x2D, "N"),
    (0x2E, "M"),
    (0x31, "Space"),
    (0x24, "Return"),
    (0x30, "Tab"),
    (0x33, "Delete"),
    (0x35, "Escape"),
    (0x7A, "F1"),
    (0x78, "F2"),
    (0x63, "F3"),
    (0x76, "F4"),
    (0x60, "F5"),
    (0x61, "F6"),
    (0x62, "F7"),
    (0x64, "F8"),
    (0x65, "F9"),
    (0x6D, "F10"),
    (0x67, "F11"),
    (0x6F, "F12"),
    (0x29, ";"),
    (0x27, "'"),
    (0x2B, ","),
    (0x2F, "."),
];

pub fn key_code_to_string(key_code: u16) -> String {
    KEY_NAMES
        .iter()
        .find(|(code, _)| *code == key_code)
        .map_or_else(|| format!("Key{key_code}"), |(_, name)| (*name).to_owned())
}

/// "⌥S": the modifiers followed by the key.
pub fn describe(key_code: u16, modifiers: HotkeyModifiers) -> String {
    format!(
        "{}{}",
        modifiers_to_string(modifiers),
        key_code_to_string(key_code)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_default_hotkey_is_option_s() {
        assert_eq!(
            describe(DEFAULT_HOTKEY_KEY_CODE, DEFAULT_HOTKEY_MODIFIERS),
            "⌥S"
        );
    }

    #[test]
    fn modifiers_are_shown_in_the_macos_order() {
        let all = HotkeyModifiers::SHIFT
            | HotkeyModifiers::COMMAND
            | HotkeyModifiers::OPTION
            | HotkeyModifiers::CONTROL;
        assert_eq!(modifiers_to_string(all), "⌃⌥⇧⌘");
        assert_eq!(modifiers_to_string(HotkeyModifiers(0)), "");
        assert_eq!(modifiers_to_string(HotkeyModifiers::COMMAND), "⌘");
    }

    #[test]
    fn a_full_shortcut_reads_modifiers_then_key() {
        let modifiers =
            HotkeyModifiers::CONTROL | HotkeyModifiers::SHIFT | HotkeyModifiers::COMMAND;
        assert_eq!(describe(0x60, modifiers), "⌃⇧⌘F5");
        assert_eq!(describe(0x31, HotkeyModifiers::OPTION), "⌥Space");
    }

    #[test]
    fn unknown_key_codes_show_their_number() {
        assert_eq!(key_code_to_string(0x7F), "Key127");
        assert_eq!(key_code_to_string(0), "A");
    }

    #[test]
    fn every_key_has_a_distinct_code_and_label() {
        let codes: HashSet<u16> = KEY_NAMES.iter().map(|(code, _)| *code).collect();
        let names: HashSet<&str> = KEY_NAMES.iter().map(|(_, name)| *name).collect();
        assert_eq!(codes.len(), KEY_NAMES.len(), "duplicate key code");
        assert_eq!(names.len(), KEY_NAMES.len(), "duplicate key label");
    }

    #[test]
    fn modifier_bits_match_ns_event_modifier_flags() {
        // NSEventModifierFlagShift/Control/Option/Command = 1<<17 .. 1<<20
        assert_eq!(HotkeyModifiers::SHIFT.0, 0x2_0000);
        assert_eq!(HotkeyModifiers::CONTROL.0, 0x4_0000);
        assert_eq!(HotkeyModifiers::OPTION.0, 0x8_0000);
        assert_eq!(HotkeyModifiers::COMMAND.0, 0x10_0000);
    }

    #[test]
    fn sanitizing_drops_caps_lock_fn_and_device_bits() {
        let caps_lock = 1 << 16;
        let function = 1 << 23;
        let device_left_shift = 0x2;
        let raw =
            HotkeyModifiers(HotkeyModifiers::OPTION.0 | caps_lock | function | device_left_shift);
        assert_eq!(raw.sanitized(), HotkeyModifiers::OPTION);
    }

    #[test]
    fn a_hotkey_needs_control_option_or_command() {
        assert!(HotkeyModifiers::CONTROL.is_valid_for_hotkey());
        assert!(HotkeyModifiers::OPTION.is_valid_for_hotkey());
        assert!((HotkeyModifiers::SHIFT | HotkeyModifiers::COMMAND).is_valid_for_hotkey());
        assert!(!HotkeyModifiers::SHIFT.is_valid_for_hotkey());
        assert!(!HotkeyModifiers(0).is_valid_for_hotkey());
        assert!(DEFAULT_HOTKEY_MODIFIERS.is_valid_for_hotkey());
    }

    #[test]
    fn contains_and_intersects_differ_for_combined_flags() {
        let both = HotkeyModifiers::OPTION | HotkeyModifiers::SHIFT;
        assert!(both.contains(HotkeyModifiers::OPTION));
        assert!(!HotkeyModifiers::OPTION.contains(both));
        assert!(HotkeyModifiers::OPTION.intersects(both));
        assert!(!HotkeyModifiers::OPTION.intersects(HotkeyModifiers::COMMAND));
    }
}
