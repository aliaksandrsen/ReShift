//! MARK: - Startup decisions
//!
//! What the app works out before the first window: the saved settings, the UI language and the state of
//! the login item. Pure functions over a `Store`, so the whole start-up is tested without AppKit.

use reshift_core::KeyboardLayout;
use reshift_core::l10n;

use crate::login_item::{self, LoginItemState};
use crate::settings::{Settings, Store};

/// Lock shared by builds of ReShift, so two instances never convert the same text twice.
pub const LOCK_NAME: &str = "com.reshift.rust";

pub struct Prepared {
    pub settings: Settings,
    /// The language is chosen on the first launch and saved.
    pub needs_save: bool,
}

/// The UI language: the saved one, otherwise Russian if any preferred language is Russian,
/// otherwise English. `true` means the choice is new and should be saved.
pub fn resolve_language<'a>(
    saved: Option<&str>,
    preferred: impl IntoIterator<Item = &'a str>,
) -> (&'static str, bool) {
    let saved = saved.and_then(|code| {
        l10n::AVAILABLE_LANGUAGES
            .iter()
            .find(|language| language.code == code)
    });
    match saved {
        Some(language) => (language.code, false),
        None => (l10n::detect_default_language(preferred), true),
    }
}

/// Loads the settings and settles everything that depends on the system.
pub fn prepare<'a>(
    store: &dyn Store,
    system_layouts: impl FnOnce() -> Vec<KeyboardLayout>,
    preferred_languages: impl IntoIterator<Item = &'a str>,
    login_state: LoginItemState,
) -> Prepared {
    let mut settings = Settings::load(store, system_layouts);
    let (language, language_is_new) =
        resolve_language(settings.app_language.as_deref(), preferred_languages);
    settings.app_language = Some(language.to_owned());

    // The user may have switched the login item in System Settings: the system decides the checkbox
    let launch_at_login = login_item::effective_setting(settings.launch_at_login, login_state);
    let login_changed = launch_at_login != settings.launch_at_login;
    settings.launch_at_login = launch_at_login;

    Prepared {
        settings,
        needs_save: language_is_new || login_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::memory::{MemoryStore, Value};

    fn system() -> Vec<KeyboardLayout> {
        vec![
            KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
            KeyboardLayout::new("com.apple.keylayout.RussianWin", "Russian – PC", "ru"),
        ]
    }

    fn prepare_with(store: &MemoryStore, preferred: &[&str], login: LoginItemState) -> Prepared {
        prepare(store, system, preferred.iter().copied(), login)
    }

    // MARK: - Language

    #[test]
    fn a_saved_language_wins_over_the_system_language() {
        assert_eq!(resolve_language(Some("en"), ["ru-RU"]), ("en", false));
        assert_eq!(resolve_language(Some("ru"), ["en-US"]), ("ru", false));
    }

    #[test]
    fn the_first_launch_follows_the_preferred_languages_and_asks_to_save() {
        assert_eq!(resolve_language(None, ["en-US", "ru"]), ("ru", true));
        assert_eq!(resolve_language(None, ["de-DE"]), ("en", true));
        assert_eq!(resolve_language(None, []), ("en", true));
    }

    #[test]
    fn an_unknown_saved_language_is_chosen_again() {
        assert_eq!(resolve_language(Some("xx"), ["ru"]), ("ru", true));
        assert_eq!(resolve_language(Some(""), ["en"]), ("en", true));
    }

    // MARK: - Prepared settings

    #[test]
    fn a_fresh_install_gets_defaults_the_system_layouts_and_a_language_to_save() {
        let prepared = prepare_with(
            &MemoryStore::default(),
            &["ru-RU"],
            LoginItemState::NotRegistered,
        );
        assert_eq!(prepared.settings.enabled_layouts, system());
        assert_eq!(prepared.settings.app_language.as_deref(), Some("ru"));
        assert!(prepared.needs_save);
        assert!(!prepared.settings.launch_at_login);
    }

    #[test]
    fn saved_settings_are_left_alone() {
        let store = MemoryStore::default();
        let mut settings = Settings::defaults(system());
        settings.app_language = Some("en".to_owned());
        settings.play_sounds = false;
        settings.save(&store);

        let prepared = prepare_with(&store, &["ru"], LoginItemState::NotRegistered);
        assert_eq!(prepared.settings, settings);
        assert!(!prepared.needs_save, "nothing changed, nothing to write");
    }

    #[test]
    fn the_login_item_state_of_the_system_overrides_a_stale_checkbox() {
        let store = MemoryStore::default();
        let mut settings = Settings::defaults(system());
        settings.app_language = Some("en".to_owned());
        settings.launch_at_login = true;
        settings.save(&store);

        // Switched off in System Settings > Login Items
        let off = prepare_with(&store, &[], LoginItemState::NotRegistered);
        assert!(!off.settings.launch_at_login);
        assert!(off.needs_save);

        // Switched on there while the checkbox said no
        settings.launch_at_login = false;
        settings.save(&store);
        let on = prepare_with(&store, &[], LoginItemState::Enabled);
        assert!(on.settings.launch_at_login);
        assert!(on.needs_save);
    }

    #[test]
    fn a_bare_binary_keeps_the_stored_checkbox() {
        let store = MemoryStore::default();
        let mut settings = Settings::defaults(system());
        settings.app_language = Some("en".to_owned());
        settings.launch_at_login = true;
        settings.save(&store);
        let prepared = prepare_with(&store, &[], LoginItemState::NotFound);
        assert!(prepared.settings.launch_at_login);
        assert!(!prepared.needs_save);
    }

    #[test]
    fn garbage_in_the_store_still_starts_with_defaults() {
        let store = MemoryStore::default();
        store.put("enabledLayouts", Value::Int(5));
        store.put("appLanguage", Value::Bool(true));
        store.put("smartConversionMode", Value::Text("greedy".to_owned()));
        let prepared = prepare_with(&store, &["en"], LoginItemState::NotRegistered);
        assert_eq!(prepared.settings.enabled_layouts, system());
        assert_eq!(prepared.settings.app_language.as_deref(), Some("en"));
        assert!(prepared.needs_save);
    }

    #[test]
    fn the_lock_name_is_shared_by_release_and_dev_builds() {
        assert_eq!(LOCK_NAME, "com.reshift.rust");
    }
}
