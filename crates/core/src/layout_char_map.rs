//! MARK: - Layout Character Maps
//!
//! Standard QWERTY <-> various layout mappings for physical key positions. Built-in layout
//! strings serve as the reference and the fallback: the app derives the real maps dynamically from
//! the system (`UCKeyTranslate`) and puts them into `LayoutMaps`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// The 47 character keys of a US QWERTY keyboard: unshifted characters, then shifted ones.
pub const QWERTY: &str = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?";

/// Standard US QWERTY keyboard.
pub static QWERTY_US: LazyLock<CharacterMap> = LazyLock::new(|| CharacterMap::build(QWERTY));

/// Russian (standard) keyboard - mapped to the same physical keys as US QWERTY.
pub static RUSSIAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "ё1234567890-=йцукенгшщзхъ\\фывапролджэячсмитьбю.Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,",
    )
});

/// German QWERTZ.
pub static GERMAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "^1234567890ß´qwertzuiopü+#asdfghjklöäyxcvbnm,.-°!\"§$%&/()=?`QWERTZUIOPÜ*'ASDFGHJKLÖÄYXCVBNM;:_",
    )
});

/// French AZERTY.
pub static FRENCH: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "²&é\"'(-è_çà)=azertyuiop^$*qsdfghjklmùwxcvbn,;:!³1234567890°+AZERTYUIOP¨£µQSDFGHJKLM%WXCVBN?./§",
    )
});

/// Ukrainian.
pub static UKRAINIAN: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "'1234567890-=йцукенгшщзхї\\фівапролджєячсмитьбю.₴!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЇ/ФІВАПРОЛДЖЄЯЧСМИТЬБЮ,",
    )
});

/// Spanish.
pub static SPANISH: LazyLock<CharacterMap> = LazyLock::new(|| {
    CharacterMap::build(
        "º1234567890'¡qwertyuiop`+çasdfghjklñ´zxcvbnm,.-ª!\"·$%&/()=?¿QWERTYUIOP^*ÇASDFGHJKLÑ¨ZXCVBNM;:_",
    )
});

/// macOS input source ID => built-in character map.
/// Layout IDs are matched exactly and only layouts that really match a map are listed.
/// Anything else needs a map derived dynamically from the system (`LayoutMaps::insert`).
static MAPS_BY_LAYOUT_ID: [(&str, &LazyLock<CharacterMap>); 17] = [
    ("com.apple.keylayout.US", &QWERTY_US),
    ("com.apple.keylayout.ABC", &QWERTY_US), // same as US
    ("com.apple.keylayout.USInternational-PC", &QWERTY_US),
    ("com.apple.keylayout.Australian", &QWERTY_US),
    ("com.apple.keylayout.British", &QWERTY_US), // close enough for conversion
    ("com.apple.keylayout.British-PC", &QWERTY_US),
    ("com.apple.keylayout.Russian", &RUSSIAN),
    ("com.apple.keylayout.RussianWin", &RUSSIAN), // Russian - PC
    ("com.apple.keylayout.Ukrainian", &UKRAINIAN),
    ("com.apple.keylayout.Ukrainian-PC", &UKRAINIAN),
    ("com.apple.keylayout.German", &GERMAN),
    ("com.apple.keylayout.Austrian", &GERMAN),
    ("com.apple.keylayout.ABC-QWERTZ", &GERMAN),
    ("com.apple.keylayout.French", &FRENCH),
    ("com.apple.keylayout.French-PC", &FRENCH),
    ("com.apple.keylayout.Spanish", &SPANISH),
    ("com.apple.keylayout.Spanish-ISO", &SPANISH),
];

/// Built-in character map for a layout identifier (case-insensitive), or `None` if the layout
/// has no built-in map.
pub fn character_map(layout_id: &str) -> Option<&'static CharacterMap> {
    MAPS_BY_LAYOUT_ID
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(layout_id))
        .map(|(_, map)| LazyLock::force(map))
}

/// All layout IDs that have a built-in character map.
pub fn supported_layout_ids() -> impl Iterator<Item = &'static str> {
    MAPS_BY_LAYOUT_ID.iter().map(|(id, _)| *id)
}

/// Physical key (its US QWERTY character) => character of a layout, plus the reverse direction.
#[derive(Debug)]
pub struct CharacterMap {
    forward: HashMap<char, char>,
    /// Layout character => physical key. First wins on duplicates, in QWERTY order
    /// (no built-in map has duplicate values - see the tests).
    reverse: HashMap<char, char>,
}

impl CharacterMap {
    fn build(layout: &str) -> Self {
        Self::from_pairs(QWERTY.chars().zip(layout.chars()))
    }

    /// A map from `(physical key as its QWERTY character, character the key types)` pairs, e.g.
    /// derived from the system layout data. The first pair wins for a repeated layout character.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (char, char)>) -> Self {
        let mut forward = HashMap::with_capacity(QWERTY.len());
        let mut reverse = HashMap::with_capacity(QWERTY.len());
        for (physical_key, layout_char) in pairs {
            forward.insert(physical_key, layout_char);
            reverse.entry(layout_char).or_insert(physical_key);
        }
        Self { forward, reverse }
    }

    /// The character the physical key (given by its QWERTY character) types in this layout.
    pub fn get(&self, physical_key: char) -> Option<char> {
        self.forward.get(&physical_key).copied()
    }

    /// The physical key (its QWERTY character) that types `layout_char` in this layout.
    pub fn physical_key(&self, layout_char: char) -> Option<char> {
        self.reverse.get(&layout_char).copied()
    }

    /// True if some key of this layout types `layout_char`.
    pub fn types(&self, layout_char: char) -> bool {
        self.reverse.contains_key(&layout_char)
    }

    /// The characters of this layout.
    pub fn values(&self) -> impl Iterator<Item = char> + '_ {
        self.forward.values().copied()
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

/// The character maps the converter works with: maps derived from the system take priority over the
/// built-in tables. The core never talks to the system itself - the app fills this in.
#[derive(Debug, Default)]
pub struct LayoutMaps {
    /// Keys are lowercase layout IDs.
    overrides: HashMap<String, CharacterMap>,
}

static BUILTIN_MAPS: LazyLock<LayoutMaps> = LazyLock::new(LayoutMaps::default);

impl LayoutMaps {
    /// Only the built-in tables.
    pub fn builtin() -> &'static LayoutMaps {
        &BUILTIN_MAPS
    }

    /// Sets the map of a layout, replacing a built-in table or an earlier map. Returns the
    /// replaced override, if any.
    pub fn insert(&mut self, layout_id: &str, map: CharacterMap) -> Option<CharacterMap> {
        self.overrides.insert(layout_id.to_ascii_lowercase(), map)
    }

    /// The map of a layout (case-insensitive): an override first, then the built-in table.
    pub fn get(&self, layout_id: &str) -> Option<&CharacterMap> {
        if self.overrides.is_empty() {
            return character_map(layout_id);
        }
        self.overrides
            .get(&layout_id.to_ascii_lowercase())
            .or_else(|| character_map(layout_id))
    }
}

#[cfg(test)]
mod tests {
    //! Guarantees around the character maps and exact-ID matching.

    use super::*;
    use crate::test_layouts as ids;
    use std::collections::HashSet;

    // The macOS reverse map picks "first" in Dictionary iteration order, which is random per
    // process. That is only safe while no map has duplicate values - keep it that way.
    #[test]
    fn maps_have_no_duplicate_values() {
        for layout_id in supported_layout_ids() {
            let map = character_map(layout_id).unwrap();
            let unique: HashSet<char> = map.values().collect();
            assert_eq!(unique.len(), map.len(), "{layout_id} has duplicate values");
        }
    }

    #[test]
    fn maps_cover_all_physical_keys() {
        for layout_id in supported_layout_ids() {
            // 47 keys x (unshifted + shifted)
            assert_eq!(character_map(layout_id).unwrap().len(), 94, "{layout_id}");
        }
    }

    #[test]
    fn layout_ids_are_case_insensitive() {
        assert!(std::ptr::eq(
            character_map("com.apple.keylayout.french").unwrap(),
            &*FRENCH
        ));
    }

    #[test]
    fn variants_use_the_map_of_their_family() {
        let families: [(&[&str], &LazyLock<CharacterMap>); 6] = [
            (
                &[
                    ids::US,
                    ids::ABC,
                    ids::US_INTERNATIONAL,
                    ids::AUSTRALIAN,
                    ids::BRITISH,
                    ids::BRITISH_PC,
                ],
                &QWERTY_US,
            ),
            (&[ids::RUSSIAN, ids::RUSSIAN_WIN], &RUSSIAN),
            (&[ids::UKRAINIAN, ids::UKRAINIAN_PC], &UKRAINIAN),
            (&[ids::GERMAN, ids::AUSTRIAN, ids::ABC_QWERTZ], &GERMAN),
            (&[ids::FRENCH, ids::FRENCH_PC], &FRENCH),
            (&[ids::SPANISH, ids::SPANISH_ISO], &SPANISH),
        ];
        for (layout_ids, map) in families {
            for layout_id in layout_ids {
                assert!(
                    std::ptr::eq(character_map(layout_id).unwrap(), &**map),
                    "{layout_id}"
                );
            }
        }
    }

    // Layouts that do not match an exact built-in table rely on dynamic system maps.
    #[test]
    fn layouts_that_only_look_similar_are_unsupported() {
        for layout_id in [
            ids::RUSSIAN_PHONETIC,
            ids::BYELORUSSIAN,
            ids::DVORAK,
            ids::COLEMAK,
            ids::SWISS_GERMAN,
            ids::JAPANESE,
        ] {
            assert!(character_map(layout_id).is_none(), "{layout_id}");
        }
    }

    #[test]
    fn austrian_is_german_and_not_us() {
        assert_eq!(character_map(ids::AUSTRIAN).unwrap().get('z'), Some('y'));
    }

    // MARK: - LayoutMaps

    /// A made-up phonetic layout: a, b and c type Cyrillic letters, every other key is US.
    fn phonetic() -> CharacterMap {
        CharacterMap::from_pairs(QWERTY.chars().map(|key| {
            let typed = match key {
                'a' => 'а',
                'b' => 'б',
                'c' => 'ц',
                other => other,
            };
            (key, typed)
        }))
    }

    #[test]
    fn maps_without_overrides_are_the_built_in_tables() {
        let maps = LayoutMaps::builtin();
        assert!(std::ptr::eq(
            maps.get(ids::RUSSIAN).unwrap(),
            character_map(ids::RUSSIAN).unwrap()
        ));
        assert!(maps.get(ids::RUSSIAN_PHONETIC).is_none());
    }

    #[test]
    fn an_override_adds_a_layout_the_tables_do_not_know() {
        let mut maps = LayoutMaps::default();
        assert!(maps.insert(ids::RUSSIAN_PHONETIC, phonetic()).is_none());
        assert_eq!(maps.get(ids::RUSSIAN_PHONETIC).unwrap().get('a'), Some('а'));
        // The lookup ignores the case of the ID, like the built-in tables
        assert!(maps.get("com.apple.keylayout.russian-phonetic").is_some());
    }

    #[test]
    fn an_override_wins_over_the_built_in_table() {
        let mut maps = LayoutMaps::default();
        maps.insert(ids::US, phonetic());
        assert_eq!(maps.get(ids::US).unwrap().get('a'), Some('а'));
        assert_eq!(maps.get(ids::RUSSIAN).unwrap().get('q'), Some('й'));
    }

    #[test]
    fn inserting_twice_returns_the_replaced_map() {
        let mut maps = LayoutMaps::default();
        maps.insert(ids::US, phonetic());
        assert!(maps.insert(ids::US, phonetic()).is_some());
    }

    #[test]
    fn from_pairs_keeps_the_first_key_of_a_repeated_character() {
        let map = CharacterMap::from_pairs([('q', 'x'), ('w', 'x'), ('e', 'y')]);
        assert_eq!(map.physical_key('x'), Some('q'));
        assert_eq!(map.get('w'), Some('x'));
        assert_eq!(map.len(), 3);
        assert!(map.types('y') && !map.types('z'));
        assert!(!map.is_empty());
    }
}
