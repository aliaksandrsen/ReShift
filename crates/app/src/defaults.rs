//! MARK: - UserDefaults store
//!
//! The production `Store`: `NSUserDefaults`. A bundled app gets the domain of its bundle ID
//! (`com.reshift.rust`); a bare binary (cargo run) gets one named after the executable, so a
//! development run never touches the settings of the installed app.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSNumber, NSString, NSUserDefaults};

use crate::settings::Store;

pub struct UserDefaultsStore {
    defaults: Retained<NSUserDefaults>,
}

impl UserDefaultsStore {
    /// The standard defaults of this process.
    pub fn standard() -> Self {
        Self {
            defaults: NSUserDefaults::standardUserDefaults(),
        }
    }

    /// A separate domain, for tests. `name` must not be the app's own bundle ID.
    #[cfg(test)]
    pub fn suite(name: &str) -> Option<Self> {
        use objc2::AnyThread;
        let defaults = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(&NSString::from_str(name)),
        )?;
        Some(Self { defaults })
    }

    #[cfg(test)]
    pub fn remove_domain(&self, name: &str) {
        self.defaults
            .removePersistentDomainForName(&NSString::from_str(name));
    }

    fn object(&self, key: &str) -> Option<Retained<AnyObject>> {
        self.defaults.objectForKey(&NSString::from_str(key))
    }

    fn number(&self, key: &str) -> Option<Retained<NSNumber>> {
        self.object(key)?.downcast::<NSNumber>().ok()
    }
}

impl Store for UserDefaultsStore {
    fn get_bool(&self, key: &str) -> Option<bool> {
        Some(self.number(key)?.boolValue())
    }

    fn get_int(&self, key: &str) -> Option<i64> {
        Some(self.number(key)?.integerValue() as i64)
    }

    fn get_string(&self, key: &str) -> Option<String> {
        // Not `stringForKey:`: it turns a stored number into a string, the contract says wrong type => None
        let value = self.object(key)?.downcast::<NSString>().ok()?;
        Some(value.to_string())
    }

    fn get_strings(&self, key: &str) -> Option<Vec<String>> {
        let array = self.defaults.stringArrayForKey(&NSString::from_str(key))?;
        Some(array.iter().map(|value| value.to_string()).collect())
    }

    fn set_bool(&self, key: &str, value: bool) {
        self.defaults
            .setBool_forKey(value, &NSString::from_str(key));
    }

    fn set_int(&self, key: &str, value: i64) {
        self.defaults
            .setInteger_forKey(value as isize, &NSString::from_str(key));
    }

    fn set_string(&self, key: &str, value: &str) {
        let value = NSString::from_str(value);
        // SAFETY: an NSString is a property list type
        unsafe {
            self.defaults
                .setObject_forKey(Some(value.as_ref()), &NSString::from_str(key));
        }
    }

    fn set_strings(&self, key: &str, values: &[String]) {
        let strings: Vec<Retained<NSString>> = values
            .iter()
            .map(|value| NSString::from_str(value))
            .collect();
        let array = NSArray::from_retained_slice(&strings);
        // SAFETY: an array of strings is a property list type
        unsafe {
            self.defaults
                .setObject_forKey(Some(array.as_ref()), &NSString::from_str(key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey_format::HotkeyModifiers;
    use crate::settings::Settings;
    use reshift_core::{KeyboardLayout, LayoutSwitchMode, SmartConversionMode};

    /// A throwaway domain; removed at the end so nothing is left in ~/Library/Preferences.
    struct Domain {
        name: String,
        store: UserDefaultsStore,
    }

    impl Domain {
        fn new(tag: &str) -> Option<Self> {
            // An absolute path as the domain keeps the plist in the temp dir instead of
            // ~/Library/Preferences, where cfprefsd would write it back after a delete
            let name = std::env::temp_dir()
                .join(format!(
                    "com.reshift.rust.test.{}.{tag}",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned();
            let store = UserDefaultsStore::suite(&name)?;
            Some(Self { name, store })
        }
    }

    impl Drop for Domain {
        fn drop(&mut self) {
            self.store.remove_domain(&self.name);
            let _ = std::fs::remove_file(format!("{}.plist", self.name));
        }
    }

    #[test]
    fn values_round_trip_through_user_defaults() {
        let domain = Domain::new("values").unwrap();
        let store = &domain.store;
        store.set_bool("b", true);
        store.set_int("i", -42);
        store.set_string("s", "ёжик \"quoted\"");
        store.set_strings("a", &["one".to_owned(), "два".to_owned()]);
        assert_eq!(store.get_bool("b"), Some(true));
        assert_eq!(store.get_int("i"), Some(-42));
        assert_eq!(store.get_string("s").as_deref(), Some("ёжик \"quoted\""));
        assert_eq!(
            store.get_strings("a"),
            Some(vec!["one".to_owned(), "два".to_owned()])
        );
    }

    #[test]
    fn missing_keys_read_as_none() {
        let domain = Domain::new("missing").unwrap();
        let store = &domain.store;
        assert_eq!(store.get_bool("nope"), None);
        assert_eq!(store.get_int("nope"), None);
        assert_eq!(store.get_string("nope"), None);
        assert_eq!(store.get_strings("nope"), None);
    }

    #[test]
    fn a_value_of_another_type_reads_as_none() {
        let domain = Domain::new("types").unwrap();
        let store = &domain.store;
        store.set_string("text", "hello");
        assert_eq!(store.get_bool("text"), None);
        assert_eq!(store.get_int("text"), None);
        assert_eq!(store.get_strings("text"), None);
        store.set_int("number", 3);
        assert_eq!(store.get_string("number"), None);
    }

    #[test]
    fn settings_round_trip_through_user_defaults() {
        let domain = Domain::new("settings").unwrap();
        let mut settings = Settings::defaults(vec![
            KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
            KeyboardLayout::new("com.apple.keylayout.RussianWin", "Russian – PC", "ru"),
        ]);
        settings.use_double_shift = false;
        settings.hotkey_key_code = 0x60;
        settings.hotkey_modifiers = HotkeyModifiers::CONTROL | HotkeyModifiers::COMMAND;
        settings.play_sounds = false;
        settings.smart_conversion_mode = SmartConversionMode::LastWord;
        settings.layout_switch_mode = LayoutSwitchMode::IfLastWordConverted;
        settings.app_language = Some("ru".to_owned());
        settings.save(&domain.store);
        assert_eq!(Settings::load(&domain.store, Vec::new), settings);
    }

    #[test]
    fn the_class_is_not_a_plain_object() {
        // downcast::<NSNumber> must reject an NSString, not just compile
        let text = NSString::from_str("1");
        assert!(text.downcast::<NSNumber>().is_err());
    }
}
