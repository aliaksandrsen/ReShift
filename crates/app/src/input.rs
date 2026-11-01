//! MARK: - Keyboard simulation
//!
//! Synthetic key events for the conversion pipeline. Every event gets the marker in
//! `eventSourceUserData`, so the event tap can tell them from the user's keys. The source is
//! `hidSystemState` with explicit flags, so a Shift that is still physically held after a double
//! Shift does not leak into the chord.

use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};

/// "LSWC" - the marker of our own events.
pub const MARKER: i64 = 0x4C53_5743;

// Virtual key codes (kVK_ANSI_C, kVK_ANSI_V, kVK_LeftArrow, kVK_RightArrow)
const KEY_C: u16 = 0x08;
const KEY_V: u16 = 0x09;
const KEY_LEFT: u16 = 0x7B;
const KEY_RIGHT: u16 = 0x7C;

// CGEventFlags masks
const SHIFT: u64 = 1 << 17;
const OPTION: u64 = 1 << 19;
const COMMAND: u64 = 1 << 20;

/// A key with the modifiers held while it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key_code: u16,
    pub flags: u64,
}

/// The key chords for simulated actions.
pub mod chords {
    use super::*;

    /// Cmd+C - copy the selection.
    pub const COPY: Chord = Chord {
        key_code: KEY_C,
        flags: COMMAND,
    };
    /// Cmd+V - paste.
    pub const PASTE: Chord = Chord {
        key_code: KEY_V,
        flags: COMMAND,
    };
    /// Cmd+Shift+Left - select from the cursor to the start of the line.
    pub const SELECT_TO_LINE_START: Chord = Chord {
        key_code: KEY_LEFT,
        flags: SHIFT | COMMAND,
    };
    /// Option+Shift+Left - select one word to the left.
    pub const SELECT_WORD_LEFT: Chord = Chord {
        key_code: KEY_LEFT,
        flags: SHIFT | OPTION,
    };
    /// Right arrow - collapse a selection that was not converted.
    pub const RIGHT_ARROW: Chord = Chord {
        key_code: KEY_RIGHT,
        flags: 0,
    };
}

#[derive(Debug, PartialEq, Eq)]
pub enum PostError {
    NoSource,
    NoEvent,
}

/// Presses and releases the chord. The system drops the events silently when the app has no
/// Accessibility permission, so callers check the permission first.
pub fn post_chord(chord: Chord) -> Result<(), PostError> {
    let source =
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok_or(PostError::NoSource)?;
    CGEventSource::set_user_data(Some(&source), MARKER);

    let down_event = CGEvent::new_keyboard_event(Some(&source), chord.key_code, true)
        .ok_or(PostError::NoEvent)?;
    CGEvent::set_flags(Some(&down_event), CGEventFlags(chord.flags));
    CGEvent::post(
        CGEventTapLocation::AnnotatedSessionEventTap,
        Some(&down_event),
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    let up_event = CGEvent::new_keyboard_event(Some(&source), chord.key_code, false)
        .ok_or(PostError::NoEvent)?;
    CGEvent::set_flags(Some(&up_event), CGEventFlags(chord.flags));
    CGEvent::post(
        CGEventTapLocation::AnnotatedSessionEventTap,
        Some(&up_event),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chords_match_expected_key_codes() {
        // Standard virtual keys: 0x08 + Command, 0x09 + Command,
        // 0x7B + [Shift, Command], 0x7B + [Shift, Alternate], 0x7C
        assert_eq!(
            chords::COPY,
            Chord {
                key_code: 0x08,
                flags: 0x10_0000
            }
        );
        assert_eq!(
            chords::PASTE,
            Chord {
                key_code: 0x09,
                flags: 0x10_0000
            }
        );
        assert_eq!(
            chords::SELECT_TO_LINE_START,
            Chord {
                key_code: 0x7B,
                flags: 0x12_0000
            }
        );
        assert_eq!(
            chords::SELECT_WORD_LEFT,
            Chord {
                key_code: 0x7B,
                flags: 0x0A_0000
            }
        );
        assert_eq!(
            chords::RIGHT_ARROW,
            Chord {
                key_code: 0x7C,
                flags: 0
            }
        );
    }

    #[test]
    fn the_flag_bits_are_the_ones_of_cg_event_flags() {
        assert_eq!(SHIFT, CGEventFlags::MaskShift.0);
        assert_eq!(OPTION, CGEventFlags::MaskAlternate.0);
        assert_eq!(COMMAND, CGEventFlags::MaskCommand.0);
    }

    #[test]
    fn the_marker_spells_lswc() {
        assert_eq!(MARKER.to_be_bytes()[4..], *b"LSWC");
    }

    #[test]
    fn no_chord_uses_control() {
        // Control combinations are terminal shortcuts; the macOS flow never needs them
        let control = 1u64 << 18;
        for chord in [
            chords::COPY,
            chords::PASTE,
            chords::SELECT_TO_LINE_START,
            chords::SELECT_WORD_LEFT,
            chords::RIGHT_ARROW,
        ] {
            assert_eq!(chord.flags & control, 0);
        }
    }
}
