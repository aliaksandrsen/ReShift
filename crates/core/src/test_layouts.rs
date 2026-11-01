//! macOS input source IDs used by the tests.

use crate::layout::KeyboardLayout;

pub const US: &str = "com.apple.keylayout.US";
pub const ABC: &str = "com.apple.keylayout.ABC";
pub const US_INTERNATIONAL: &str = "com.apple.keylayout.USInternational-PC";
pub const AUSTRALIAN: &str = "com.apple.keylayout.Australian";
pub const BRITISH: &str = "com.apple.keylayout.British";
pub const BRITISH_PC: &str = "com.apple.keylayout.British-PC";
pub const RUSSIAN: &str = "com.apple.keylayout.Russian";
pub const RUSSIAN_WIN: &str = "com.apple.keylayout.RussianWin";
pub const RUSSIAN_PHONETIC: &str = "com.apple.keylayout.Russian-Phonetic";
pub const BYELORUSSIAN: &str = "com.apple.keylayout.Byelorussian";
pub const UKRAINIAN: &str = "com.apple.keylayout.Ukrainian";
pub const UKRAINIAN_PC: &str = "com.apple.keylayout.Ukrainian-PC";
pub const GERMAN: &str = "com.apple.keylayout.German";
pub const AUSTRIAN: &str = "com.apple.keylayout.Austrian";
pub const ABC_QWERTZ: &str = "com.apple.keylayout.ABC-QWERTZ";
pub const SWISS_GERMAN: &str = "com.apple.keylayout.SwissGerman";
pub const FRENCH: &str = "com.apple.keylayout.French";
pub const FRENCH_PC: &str = "com.apple.keylayout.French-PC";
pub const SPANISH: &str = "com.apple.keylayout.Spanish";
pub const SPANISH_ISO: &str = "com.apple.keylayout.Spanish-ISO";
pub const DVORAK: &str = "com.apple.keylayout.Dvorak";
pub const COLEMAK: &str = "com.apple.keylayout.Colemak";
pub const JAPANESE: &str = "com.apple.keylayout.Japanese";

/// Deterministic [US, RU] pair - the macOS tests depended on the machine's layouts.
pub fn us_russian() -> Vec<KeyboardLayout> {
    vec![
        KeyboardLayout::new(US, "U.S.", "en"),
        KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
    ]
}
