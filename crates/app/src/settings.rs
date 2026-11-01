//! MARK: - Settings
//!
//! User preferences (UserDefaults in production). The logic lives
//! behind the small `Store` trait, so it is tested with an in-memory store. Keys of removed features
//! (statistics, conversion log) are neither read nor written.

use reshift_core::l10n;
use reshift_core::{KeyboardLayout, LayoutSwitchMode, SmartConversionMode};

use crate::hotkey_format::{
    self, DEFAULT_HOTKEY_KEY_CODE, DEFAULT_HOTKEY_MODIFIERS, HotkeyModifiers,
};
use crate::indicator::IndicatorStyle;

/// Preference key names.
mod keys {
    pub const ENABLED_LAYOUTS: &str = "enabledLayouts";
    pub const USE_DOUBLE_SHIFT: &str = "useDoubleShift";
    pub const HOTKEY_KEY_CODE: &str = "hotkeyKeyCode";
    pub const HOTKEY_MODIFIERS: &str = "hotkeyModifiers";
    pub const LAUNCH_AT_LOGIN: &str = "launchAtLogin";
    pub const PLAY_SOUNDS: &str = "playSounds";
    pub const SMART_CONVERSION_MODE: &str = "smartConversionMode";
    pub const LAYOUT_SWITCH_MODE: &str = "layoutSwitchMode";
    pub const INDICATOR_STYLE: &str = "indicatorStyle";
    pub const APP_LANGUAGE: &str = "appLanguage";
}

/// A key-value store of preferences. Reads return `None` for a missing key or a value of the wrong
/// type, like `defaults.object(forKey:) as? T`.
pub trait Store {
    fn get_bool(&self, key: &str) -> Option<bool>;
    fn get_int(&self, key: &str) -> Option<i64>;
    fn get_string(&self, key: &str) -> Option<String>;
    fn get_strings(&self, key: &str) -> Option<Vec<String>>;
    fn set_bool(&self, key: &str, value: bool);
    fn set_int(&self, key: &str, value: i64);
    fn set_string(&self, key: &str, value: &str);
    fn set_strings(&self, key: &str, values: &[String]);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled_layouts: Vec<KeyboardLayout>,
    /// true = double-shift mode, false = regular modifier+key hotkey.
    pub use_double_shift: bool,
    /// Virtual key code of the custom hotkey.
    pub hotkey_key_code: u16,
    pub hotkey_modifiers: HotkeyModifiers,
    pub launch_at_login: bool,
    pub play_sounds: bool,
    pub smart_conversion_mode: SmartConversionMode,
    pub layout_switch_mode: LayoutSwitchMode,
    pub indicator_style: IndicatorStyle,
    /// UI language code; `None` until the user or the first launch chooses one.
    pub app_language: Option<String>,
}

impl Settings {
    /// Default application settings.
    pub fn defaults(system_layouts: Vec<KeyboardLayout>) -> Self {
        Self {
            enabled_layouts: system_layouts,
            use_double_shift: true,
            hotkey_key_code: DEFAULT_HOTKEY_KEY_CODE,
            hotkey_modifiers: DEFAULT_HOTKEY_MODIFIERS,
            launch_at_login: false,
            play_sounds: false,
            smart_conversion_mode: SmartConversionMode::GreedyLine,
            layout_switch_mode: LayoutSwitchMode::Always,
            indicator_style: IndicatorStyle::Flag,
            app_language: None,
        }
    }

    /// "⇧⇧ (Double Shift)" or e.g. "⌥S", in the given UI language.
    pub fn hotkey_description(&self, language: &str) -> String {
        if self.use_double_shift {
            l10n::lookup(language, "hotkey.doubleShiftDescription").to_owned()
        } else {
            hotkey_format::describe(self.hotkey_key_code, self.hotkey_modifiers)
        }
    }

    /// Whether the hotkey has to be re-registered after a change:
    /// a mode change always does; the custom key only matters while it is in use.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the hotkey registration uses it from stage 3")
    )]
    pub fn hotkey_changed(&self, previous: &Self) -> bool {
        self.use_double_shift != previous.use_double_shift
            || (!self.use_double_shift
                && (self.hotkey_key_code != previous.hotkey_key_code
                    || self.hotkey_modifiers != previous.hotkey_modifiers))
    }

    /// Loads the settings. Missing or invalid values fall back to the defaults one by one, like
    /// `defaults.object(forKey:) ?? default`. `system_layouts` is only called when no layouts are saved.
    pub fn load(store: &dyn Store, system_layouts: impl FnOnce() -> Vec<KeyboardLayout>) -> Self {
        let saved_layouts: Vec<KeyboardLayout> = store
            .get_strings(keys::ENABLED_LAYOUTS)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| decode_layout(entry))
            .collect();
        let mut settings = Self::defaults(Vec::new());
        settings.enabled_layouts = if saved_layouts.is_empty() {
            system_layouts()
        } else {
            saved_layouts
        };
        let defaults = settings.clone();

        settings.use_double_shift = store
            .get_bool(keys::USE_DOUBLE_SHIFT)
            .unwrap_or(defaults.use_double_shift);
        let key_code = store
            .get_int(keys::HOTKEY_KEY_CODE)
            .and_then(|code| u16::try_from(code).ok());
        let modifiers = store
            .get_int(keys::HOTKEY_MODIFIERS)
            .and_then(|flags| u32::try_from(flags).ok())
            .map(|flags| HotkeyModifiers(flags).sanitized());
        // A custom hotkey is only usable as a whole pair: a lone invalid half falls back together
        if let (Some(key_code), Some(modifiers)) = (key_code, modifiers)
            && modifiers.is_valid_for_hotkey()
        {
            settings.hotkey_key_code = key_code;
            settings.hotkey_modifiers = modifiers;
        }
        settings.launch_at_login = store
            .get_bool(keys::LAUNCH_AT_LOGIN)
            .unwrap_or(defaults.launch_at_login);
        settings.play_sounds = store
            .get_bool(keys::PLAY_SOUNDS)
            .unwrap_or(defaults.play_sounds);
        settings.smart_conversion_mode = store
            .get_int(keys::SMART_CONVERSION_MODE)
            .and_then(SmartConversionMode::from_i64)
            .unwrap_or(defaults.smart_conversion_mode);
        settings.layout_switch_mode = store
            .get_int(keys::LAYOUT_SWITCH_MODE)
            .and_then(LayoutSwitchMode::from_i64)
            .unwrap_or(defaults.layout_switch_mode);
        settings.indicator_style = store
            .get_int(keys::INDICATOR_STYLE)
            .and_then(IndicatorStyle::from_i64)
            .unwrap_or(defaults.indicator_style);
        settings.app_language = store
            .get_string(keys::APP_LANGUAGE)
            .filter(|code| l10n::AVAILABLE_LANGUAGES.iter().any(|l| l.code == code));
        settings
    }

    pub fn save(&self, store: &dyn Store) {
        let layouts: Vec<String> = self.enabled_layouts.iter().map(encode_layout).collect();
        store.set_strings(keys::ENABLED_LAYOUTS, &layouts);
        store.set_bool(keys::USE_DOUBLE_SHIFT, self.use_double_shift);
        store.set_int(keys::HOTKEY_KEY_CODE, i64::from(self.hotkey_key_code));
        store.set_int(keys::HOTKEY_MODIFIERS, i64::from(self.hotkey_modifiers.0));
        store.set_bool(keys::LAUNCH_AT_LOGIN, self.launch_at_login);
        store.set_bool(keys::PLAY_SOUNDS, self.play_sounds);
        store.set_int(
            keys::SMART_CONVERSION_MODE,
            self.smart_conversion_mode as i64,
        );
        store.set_int(keys::LAYOUT_SWITCH_MODE, self.layout_switch_mode as i64);
        store.set_int(keys::INDICATOR_STYLE, self.indicator_style as i64);
        if let Some(language) = &self.app_language {
            store.set_string(keys::APP_LANGUAGE, language);
        }
    }
}

// MARK: - Saved layouts

/// Separates the fields of a saved layout. It cannot occur in an input source ID, a localized name
/// or a language code, so no escaping is needed.
const FIELD_SEPARATOR: char = '\u{1F}';

fn encode_layout(layout: &KeyboardLayout) -> String {
    format!(
        "{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}",
        layout.id, layout.localized_name, layout.language_code
    )
}

/// `None` for an entry that is not "id, name, language" or has an empty ID.
fn decode_layout(entry: &str) -> Option<KeyboardLayout> {
    let mut fields = entry.split(FIELD_SEPARATOR);
    let (id, name, language) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() || id.is_empty() {
        return None;
    }
    Some(KeyboardLayout::new(id, name, language))
}

#[cfg(test)]
pub mod memory {
    //! An in-memory `Store` for tests.

    use super::Store;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Clone, Debug, PartialEq)]
    pub enum Value {
        Bool(bool),
        Int(i64),
        Text(String),
        Texts(Vec<String>),
    }

    #[derive(Default)]
    pub struct MemoryStore {
        pub values: RefCell<HashMap<String, Value>>,
    }

    impl MemoryStore {
        pub fn put(&self, key: &str, value: Value) {
            self.values.borrow_mut().insert(key.to_owned(), value);
        }

        pub fn get(&self, key: &str) -> Option<Value> {
            self.values.borrow().get(key).cloned()
        }
    }

    impl Store for MemoryStore {
        fn get_bool(&self, key: &str) -> Option<bool> {
            match self.get(key)? {
                Value::Bool(value) => Some(value),
                _ => None,
            }
        }

        fn get_int(&self, key: &str) -> Option<i64> {
            match self.get(key)? {
                Value::Int(value) => Some(value),
                _ => None,
            }
        }

        fn get_string(&self, key: &str) -> Option<String> {
            match self.get(key)? {
                Value::Text(value) => Some(value),
                _ => None,
            }
        }

        fn get_strings(&self, key: &str) -> Option<Vec<String>> {
            match self.get(key)? {
                Value::Texts(value) => Some(value),
                _ => None,
            }
        }

        fn set_bool(&self, key: &str, value: bool) {
            self.put(key, Value::Bool(value));
        }

        fn set_int(&self, key: &str, value: i64) {
            self.put(key, Value::Int(value));
        }

        fn set_string(&self, key: &str, value: &str) {
            self.put(key, Value::Text(value.to_owned()));
        }

        fn set_strings(&self, key: &str, values: &[String]) {
            self.put(key, Value::Texts(values.to_vec()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::{MemoryStore, Value};
    use super::*;
    use std::cell::Cell;

    fn system() -> Vec<KeyboardLayout> {
        vec![
            KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
            KeyboardLayout::new("com.apple.keylayout.RussianWin", "Russian – PC", "ru"),
        ]
    }

    fn load(store: &MemoryStore) -> Settings {
        Settings::load(store, system)
    }

    // MARK: - Defaults

    #[test]
    fn an_empty_store_gives_expected_defaults() {
        let settings = load(&MemoryStore::default());
        assert_eq!(settings, Settings::defaults(system()));
        assert!(settings.use_double_shift);
        assert_eq!(settings.hotkey_key_code, 0x01);
        assert_eq!(settings.hotkey_modifiers, HotkeyModifiers::OPTION);
        assert!(!settings.launch_at_login);
        assert!(!settings.play_sounds);
        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::GreedyLine
        );
        assert_eq!(settings.layout_switch_mode, LayoutSwitchMode::Always);
        assert_eq!(settings.app_language, None);
    }

    #[test]
    fn system_layouts_are_asked_only_when_none_are_saved() {
        let calls = Cell::new(0);
        let counting = || {
            calls.set(calls.get() + 1);
            system()
        };
        let store = MemoryStore::default();
        Settings::load(&store, counting);
        assert_eq!(calls.get(), 1);

        Settings::defaults(system()).save(&store);
        Settings::load(&store, || {
            calls.set(calls.get() + 1);
            Vec::new()
        });
        assert_eq!(
            calls.get(),
            1,
            "saved layouts must not trigger a system query"
        );
    }

    // MARK: - Round trip

    #[test]
    fn saved_settings_load_back_unchanged() {
        let mut settings = Settings::defaults(system());
        settings.use_double_shift = false;
        settings.hotkey_key_code = 0x60;
        settings.hotkey_modifiers = HotkeyModifiers::CONTROL | HotkeyModifiers::COMMAND;
        settings.launch_at_login = true;
        settings.play_sounds = true;
        settings.smart_conversion_mode = SmartConversionMode::LastWord;
        settings.layout_switch_mode = LayoutSwitchMode::IfAnyWordConverted;
        settings.app_language = Some("ru".to_owned());

        let store = MemoryStore::default();
        settings.save(&store);
        assert_eq!(load(&store), settings);
    }

    #[test]
    fn the_stored_values_use_expected_key_names_and_numbers() {
        let store = MemoryStore::default();
        let mut settings = Settings::defaults(system());
        settings.smart_conversion_mode = SmartConversionMode::Disabled;
        settings.layout_switch_mode = LayoutSwitchMode::IfLastWordConverted;
        settings.app_language = Some("en".to_owned());
        settings.save(&store);

        assert_eq!(store.get("smartConversionMode"), Some(Value::Int(2)));
        assert_eq!(store.get("layoutSwitchMode"), Some(Value::Int(1)));
        assert_eq!(store.get("hotkeyKeyCode"), Some(Value::Int(1)));
        assert_eq!(store.get("hotkeyModifiers"), Some(Value::Int(1 << 19)));
        assert_eq!(store.get("useDoubleShift"), Some(Value::Bool(true)));
        assert_eq!(store.get("appLanguage"), Some(Value::Text("en".to_owned())));
        for key in ["launchAtLogin", "playSounds", "enabledLayouts"] {
            assert!(store.get(key).is_some(), "{key} was not saved");
        }
    }

    #[test]
    fn removed_features_leave_no_keys() {
        let store = MemoryStore::default();
        Settings::defaults(system()).save(&store);
        for key in [
            "conversionCount",
            "loggingEnabled",
            "logMaxEntries",
            "showNotifications",
        ] {
            assert!(store.get(key).is_none(), "{key} must not come back");
        }
        assert!(store.get("appLanguage").is_none(), "no language chosen yet");
    }

    // MARK: - Invalid values

    #[test]
    fn out_of_range_modes_fall_back_to_the_defaults() {
        let store = MemoryStore::default();
        store.put("smartConversionMode", Value::Int(7));
        store.put("layoutSwitchMode", Value::Int(-1));
        let settings = load(&store);
        assert_eq!(
            settings.smart_conversion_mode,
            SmartConversionMode::GreedyLine
        );
        assert_eq!(settings.layout_switch_mode, LayoutSwitchMode::Always);
    }

    #[test]
    fn values_of_the_wrong_type_are_ignored() {
        let store = MemoryStore::default();
        store.put("useDoubleShift", Value::Text("yes".to_owned()));
        store.put("playSounds", Value::Int(0));
        store.put("smartConversionMode", Value::Bool(true));
        store.put("enabledLayouts", Value::Text("US".to_owned()));
        let settings = load(&store);
        assert_eq!(settings, Settings::defaults(system()));
    }

    #[test]
    fn the_hotkey_pair_is_validated_as_a_whole() {
        let store = MemoryStore::default();
        // Shift alone would trigger on every capital letter
        store.put("hotkeyKeyCode", Value::Int(0x60));
        store.put(
            "hotkeyModifiers",
            Value::Int(i64::from(HotkeyModifiers::SHIFT.0)),
        );
        let settings = load(&store);
        assert_eq!(settings.hotkey_key_code, DEFAULT_HOTKEY_KEY_CODE);
        assert_eq!(settings.hotkey_modifiers, DEFAULT_HOTKEY_MODIFIERS);

        // A key code that does not fit a u16
        store.put("hotkeyKeyCode", Value::Int(70_000));
        store.put(
            "hotkeyModifiers",
            Value::Int(i64::from(HotkeyModifiers::COMMAND.0)),
        );
        assert_eq!(load(&store).hotkey_key_code, DEFAULT_HOTKEY_KEY_CODE);

        // Only one of the two saved: the pair is incomplete
        let store = MemoryStore::default();
        store.put("hotkeyKeyCode", Value::Int(0x60));
        assert_eq!(load(&store).hotkey_key_code, DEFAULT_HOTKEY_KEY_CODE);
    }

    #[test]
    fn extra_modifier_bits_are_dropped_on_load() {
        let store = MemoryStore::default();
        let caps_lock = 1 << 16;
        store.put("hotkeyKeyCode", Value::Int(0x60));
        store.put(
            "hotkeyModifiers",
            Value::Int(i64::from(HotkeyModifiers::OPTION.0) | caps_lock),
        );
        assert_eq!(load(&store).hotkey_modifiers, HotkeyModifiers::OPTION);
    }

    #[test]
    fn an_unknown_language_is_dropped() {
        let store = MemoryStore::default();
        store.put("appLanguage", Value::Text("xx".to_owned()));
        assert_eq!(load(&store).app_language, None);
        store.put("appLanguage", Value::Text("ru".to_owned()));
        assert_eq!(load(&store).app_language.as_deref(), Some("ru"));
    }

    // MARK: - Saved layouts

    #[test]
    fn malformed_layout_entries_are_skipped() {
        let sep = FIELD_SEPARATOR;
        let store = MemoryStore::default();
        store.put(
            "enabledLayouts",
            Value::Texts(vec![
                format!("com.apple.keylayout.US{sep}U.S.{sep}en"),
                "just-an-id".to_owned(),
                format!("{sep}no id{sep}xx"),
                format!("a{sep}b{sep}c{sep}d"),
                String::new(),
                format!("com.apple.keylayout.German{sep}German{sep}de"),
            ]),
        );
        let ids: Vec<String> = load(&store)
            .enabled_layouts
            .into_iter()
            .map(|layout| layout.id)
            .collect();
        assert_eq!(
            ids,
            ["com.apple.keylayout.US", "com.apple.keylayout.German"]
        );
    }

    #[test]
    fn all_entries_malformed_means_the_system_layouts() {
        let store = MemoryStore::default();
        store.put("enabledLayouts", Value::Texts(vec!["junk".to_owned()]));
        assert_eq!(load(&store).enabled_layouts, system());
    }

    #[test]
    fn layouts_with_odd_characters_round_trip() {
        let layout = KeyboardLayout::new(
            "com.apple.keylayout.RussianWin",
            "Russian – PC \"quoted\" \\ ёжик",
            "ru",
        );
        assert_eq!(decode_layout(&encode_layout(&layout)), Some(layout));
    }

    #[test]
    fn layout_order_is_kept() {
        let mut settings = Settings::defaults(system());
        settings.enabled_layouts.reverse();
        let store = MemoryStore::default();
        settings.save(&store);
        assert_eq!(load(&store).enabled_layouts, settings.enabled_layouts);
    }

    // MARK: - Hotkey

    #[test]
    fn the_description_follows_the_hotkey_mode_and_language() {
        let mut settings = Settings::defaults(system());
        assert_eq!(settings.hotkey_description("en"), "⇧⇧ (Double Shift)");
        assert_eq!(settings.hotkey_description("ru"), "⇧⇧ (двойной Shift)");
        settings.use_double_shift = false;
        assert_eq!(settings.hotkey_description("en"), "⌥S");
        assert_eq!(settings.hotkey_description("ru"), "⌥S");
    }

    #[test]
    fn a_hotkey_change_is_detected_only_where_it_matters() {
        let base = Settings::defaults(system());

        let mut changed = base.clone();
        changed.use_double_shift = false;
        assert!(changed.hotkey_changed(&base));
        assert!(base.hotkey_changed(&changed));

        // The custom key is irrelevant while double Shift is active
        let mut other_key = base.clone();
        other_key.hotkey_key_code = 0x60;
        assert!(!other_key.hotkey_changed(&base));

        // ... and relevant once it is in use
        let mut custom = base.clone();
        custom.use_double_shift = false;
        let mut custom_other_key = custom.clone();
        custom_other_key.hotkey_key_code = 0x60;
        assert!(custom_other_key.hotkey_changed(&custom));
        let mut custom_other_modifier = custom.clone();
        custom_other_modifier.hotkey_modifiers = HotkeyModifiers::COMMAND;
        assert!(custom_other_modifier.hotkey_changed(&custom));
        assert!(!custom.hotkey_changed(&custom.clone()));

        // Other settings never re-register the hotkey
        let mut sounds = base.clone();
        sounds.play_sounds = false;
        assert!(!sounds.hotkey_changed(&base));
    }
}
