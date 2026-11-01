//! MARK: - System character maps
//!
//! Character maps derived dynamically from the system, so the converter is exact for every enabled
//! layout. For each layout the 47 character keys of a keyboard are translated via `UCKeyTranslate`,
//! unshifted and shifted (94 pairs total).
//!
//! The pairing logic is pure (`derive_pairs`, tested with a fake keyboard); the FFI part only feeds it.

use std::collections::HashSet;

use reshift_core::layout_char_map::QWERTY;
use reshift_core::{CharacterMap, KeyboardLayout, LayoutMaps};

/// Virtual key codes of the 47 character keys, in `QWERTY` order (unshifted half). The first one is the key
/// left of "1": 0x32 on ANSI and JIS boards, 0x0A on ISO ones.
pub const ANSI_KEY_CODES: [u16; 47] = [
    0x32, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19, 0x1D, 0x1B,
    0x18, // ` 1..0 - =
    0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x10, 0x20, 0x22, 0x1F, 0x23, 0x21, 0x1E,
    0x2A, // q..p [ ] \
    0x00, 0x01, 0x02, 0x03, 0x05, 0x04, 0x26, 0x28, 0x25, 0x29, 0x27, // a..l ; '
    0x06, 0x07, 0x08, 0x09, 0x0B, 0x2D, 0x2E, 0x2B, 0x2F, 0x2C, // z..m , . /
];

/// The key left of "1" on an ISO keyboard.
const ISO_GRAVE_KEY_CODE: u16 = 0x0A;

/// The physical shape of the keyboard, which decides the key left of "1".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardShape {
    Ansi,
    Iso,
}

/// The key codes to press, in `QWERTY` order: 47 unshifted, then the same 47 shifted.
pub fn key_codes(shape: BoardShape) -> [u16; 47] {
    let mut codes = ANSI_KEY_CODES;
    if shape == BoardShape::Iso {
        codes[0] = ISO_GRAVE_KEY_CODE;
    }
    codes
}

/// `(physical key as its QWERTY character, character the key types)` for every key the layout types a
/// single character on. `translate(key code, shift held)` is the system (or a fake in tests); keys it
/// has no character for (nothing, several characters, a control character) are left out.
pub fn derive_pairs(
    shape: BoardShape,
    mut translate: impl FnMut(u16, bool) -> Option<char>,
) -> Vec<(char, char)> {
    let codes = key_codes(shape);
    let mut pairs = Vec::with_capacity(QWERTY.len());
    for (index, physical) in QWERTY.chars().enumerate() {
        let (code, shift) = (codes[index % 47], index >= 47);
        let Some(typed) = translate(code, shift) else {
            continue;
        };
        if typed.is_control() || typed == '\u{FFFD}' {
            continue;
        }
        pairs.push((physical, typed));
    }
    pairs
}

/// The layouts a map has to be built for, without repeats, in order.
pub fn layout_ids(layouts: &[KeyboardLayout]) -> Vec<&str> {
    let mut seen = HashSet::new();
    layouts
        .iter()
        .map(|layout| layout.id.as_str())
        .filter(|id| seen.insert(id.to_ascii_lowercase()))
        .collect()
}

/// Maps for `ids` taken from the system. A layout the system cannot give data for keeps its built-in
/// table (`LayoutMaps::get` falls back to it), so this never makes conversion worse than the tables.
pub fn build_maps(ids: &[&str], shape: BoardShape, mut source: impl LayoutSource) -> LayoutMaps {
    let mut maps = LayoutMaps::default();
    for id in ids {
        let Some(data) = source.layout_data(id) else {
            continue;
        };
        let pairs = derive_pairs(shape, |code, shift| source.translate(&data, code, shift));
        // A layout that types fewer than half the keys is not a usable map (no data, wrong format)
        if pairs.len() >= QWERTY.chars().count() / 2 {
            maps.insert(id, CharacterMap::from_pairs(pairs));
        }
    }
    maps
}

/// Where the layout data and the key translation come from (the system, or a fake).
pub trait LayoutSource {
    type Data;
    fn layout_data(&mut self, id: &str) -> Option<Self::Data>;
    fn translate(&mut self, data: &Self::Data, key_code: u16, shift: bool) -> Option<char>;
}

// MARK: - The system

/// `UCKeyTranslate` over the `uchr` data of the Text Input Sources.
pub struct SystemSource {
    keyboard_type: u32,
}

impl SystemSource {
    pub fn new(keyboard_type: u32) -> Self {
        Self { keyboard_type }
    }
}

impl LayoutSource for SystemSource {
    type Data = Vec<u8>;

    fn layout_data(&mut self, id: &str) -> Option<Vec<u8>> {
        crate::layouts::layout_uchr(id)
    }

    fn translate(&mut self, data: &Vec<u8>, key_code: u16, shift: bool) -> Option<char> {
        crate::layouts::translate_key(data, key_code, shift, self.keyboard_type)
    }
}

/// Maps for the enabled layouts, for the keyboard that is attached right now.
pub fn system_maps(layouts: &[KeyboardLayout]) -> LayoutMaps {
    let (keyboard_type, shape) = crate::layouts::keyboard();
    build_maps(
        &layout_ids(layouts),
        shape,
        SystemSource::new(keyboard_type),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A keyboard where the unshifted key at position `i` types `lower[i]` and shifted `upper[i]`.
    fn fake_layout(
        shape: BoardShape,
        lower: &str,
        upper: &str,
    ) -> impl FnMut(u16, bool) -> Option<char> {
        let codes = key_codes(shape);
        let lower: Vec<char> = lower.chars().collect();
        let upper: Vec<char> = upper.chars().collect();
        move |code, shift| {
            let index = codes.iter().position(|c| *c == code)?;
            if index >= lower.len() || index >= upper.len() {
                return None;
            }
            Some(if shift { upper[index] } else { lower[index] })
        }
    }

    const US_LOWER: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./";
    const US_UPPER: &str = "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";

    // MARK: - Key codes

    #[test]
    fn there_are_47_distinct_key_codes() {
        let unique: HashSet<u16> = ANSI_KEY_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 47);
        assert_eq!(QWERTY.chars().count(), 94);
    }

    #[test]
    fn an_iso_board_swaps_only_the_grave_key() {
        let ansi = key_codes(BoardShape::Ansi);
        let iso = key_codes(BoardShape::Iso);
        assert_eq!(ansi[0], 0x32);
        assert_eq!(iso[0], 0x0A);
        assert_eq!(ansi[1..], iso[1..]);
    }

    #[test]
    fn the_key_codes_match_the_known_ansi_positions() {
        // kVK_ANSI_Q = 0x0C, kVK_ANSI_A = 0x00, kVK_ANSI_Z = 0x06, kVK_ANSI_1 = 0x12, kVK_ANSI_Slash = 0x2C
        let position = |c: char| QWERTY.chars().position(|q| q == c).unwrap();
        let codes = ANSI_KEY_CODES;
        assert_eq!(codes[position('q')], 0x0C);
        assert_eq!(codes[position('a')], 0x00);
        assert_eq!(codes[position('z')], 0x06);
        assert_eq!(codes[position('1')], 0x12);
        assert_eq!(codes[position('/')], 0x2C);
        assert_eq!(codes[position('`')], 0x32);
    }

    // MARK: - Pairs

    #[test]
    fn a_us_keyboard_maps_every_key_to_itself() {
        let pairs = derive_pairs(
            BoardShape::Ansi,
            fake_layout(BoardShape::Ansi, US_LOWER, US_UPPER),
        );
        assert_eq!(pairs.len(), 94);
        assert!(pairs.iter().all(|(physical, typed)| physical == typed));
    }

    #[test]
    fn a_russian_keyboard_maps_physical_keys_to_cyrillic() {
        let lower = "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.";
        let upper = "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";
        let pairs: HashMap<char, char> = derive_pairs(
            BoardShape::Ansi,
            fake_layout(BoardShape::Ansi, lower, upper),
        )
        .into_iter()
        .collect();
        assert_eq!(pairs[&'q'], 'й');
        assert_eq!(pairs[&'Q'], 'Й');
        assert_eq!(pairs[&';'], 'ж');
        assert_eq!(pairs[&'/'], '.');
        assert_eq!(pairs[&'?'], ',');
        assert_eq!(pairs[&'`'], 'ё');
    }

    #[test]
    fn keys_without_a_single_character_are_left_out() {
        let mut pairs = derive_pairs(BoardShape::Ansi, |code, shift| match code {
            0x0C => None,                 // q: nothing
            0x0D if !shift => Some('\n'), // w: a control character
            0x0E => Some('\u{FFFD}'),     // e: the translate failure marker
            0x0F if !shift => Some('r'),
            _ => Some('x'),
        });
        pairs.retain(|(physical, _)| "qwer".contains(*physical));
        assert_eq!(pairs, [('r', 'r')]);
    }

    #[test]
    fn an_iso_keyboard_reads_the_iso_grave_key() {
        let mut seen = Vec::new();
        derive_pairs(BoardShape::Iso, |code, _| {
            seen.push(code);
            Some('x')
        });
        assert!(seen.contains(&0x0A));
        assert!(!seen.contains(&0x32));
        seen.clear();
        derive_pairs(BoardShape::Ansi, |code, _| {
            seen.push(code);
            Some('x')
        });
        assert!(seen.contains(&0x32));
        assert!(!seen.contains(&0x0A));
    }

    #[test]
    fn every_key_is_pressed_once_unshifted_and_once_shifted() {
        let mut presses: Vec<(u16, bool)> = Vec::new();
        derive_pairs(BoardShape::Ansi, |code, shift| {
            presses.push((code, shift));
            Some('x')
        });
        assert_eq!(presses.len(), 94);
        assert_eq!(presses.iter().filter(|(_, shift)| *shift).count(), 47);
        let unique: HashSet<(u16, bool)> = presses.into_iter().collect();
        assert_eq!(unique.len(), 94);
    }

    // MARK: - Layout ids

    #[test]
    fn layout_ids_are_listed_once_in_order_ignoring_case() {
        let layouts = [
            KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
            KeyboardLayout::new("com.apple.keylayout.RussianWin", "Russian", "ru"),
            KeyboardLayout::new("com.apple.keylayout.us", "again", "en"),
        ];
        assert_eq!(
            layout_ids(&layouts),
            ["com.apple.keylayout.US", "com.apple.keylayout.RussianWin"]
        );
        assert!(layout_ids(&[]).is_empty());
    }

    // MARK: - Building maps

    struct FakeSystem {
        layouts: HashMap<&'static str, (&'static str, &'static str)>,
        shape: BoardShape,
    }

    impl LayoutSource for FakeSystem {
        type Data = (&'static str, &'static str);

        fn layout_data(&mut self, id: &str) -> Option<Self::Data> {
            self.layouts.get(id).copied()
        }

        fn translate(&mut self, data: &Self::Data, key_code: u16, shift: bool) -> Option<char> {
            fake_layout(self.shape, data.0, data.1)(key_code, shift)
        }
    }

    fn system() -> FakeSystem {
        FakeSystem {
            layouts: HashMap::from([
                ("us", (US_LOWER, US_UPPER)),
                (
                    "ru",
                    (
                        "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.",
                        "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,",
                    ),
                ),
                ("broken", ("", "")),
            ]),
            shape: BoardShape::Ansi,
        }
    }

    #[test]
    fn built_maps_convert_between_layouts() {
        let maps = build_maps(&["us", "ru"], BoardShape::Ansi, system());
        let layouts = [
            KeyboardLayout::new("us", "US", "en"),
            KeyboardLayout::new("ru", "RU", "ru"),
        ];
        let converter = reshift_core::TextConverter::new(&maps, &layouts);
        assert_eq!(
            converter.convert_selected_text("ghbdtn").as_deref(),
            Some("привет")
        );
        assert_eq!(
            converter.convert_selected_text("руддщ").as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn a_layout_the_system_has_no_data_for_is_skipped() {
        let maps = build_maps(&["us", "nope"], BoardShape::Ansi, system());
        assert!(maps.get("us").is_some());
        assert!(maps.get("nope").is_none());
    }

    #[test]
    fn a_layout_that_types_almost_nothing_is_not_a_map() {
        // "broken" has data but every key translates to nothing: keeping it would hide the built-in table
        let maps = build_maps(&["broken"], BoardShape::Ansi, system());
        assert!(maps.get("broken").is_none());
    }

    #[test]
    fn a_system_map_replaces_the_built_in_table_of_the_same_id() {
        let mut fake = system();
        // A US keyboard whose "q" key types "й": the system says so, the table says "q"
        fake.layouts.insert(
            "com.apple.keylayout.US",
            ("`1234567890-=йwertyuiop[]\\asdfghjkl;'zxcvbnm,./", US_UPPER),
        );
        let maps = build_maps(&["com.apple.keylayout.US"], BoardShape::Ansi, fake);
        assert_eq!(
            maps.get("com.apple.keylayout.US").unwrap().get('q'),
            Some('й')
        );
    }

    #[test]
    fn a_layout_without_system_data_still_converts_through_its_built_in_table() {
        let maps = build_maps(&["com.apple.keylayout.Russian"], BoardShape::Ansi, system());
        assert!(
            maps.get("com.apple.keylayout.Russian").is_some(),
            "the table is the fallback"
        );
    }
}
