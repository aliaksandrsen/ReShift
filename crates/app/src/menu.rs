//! MARK: - Menu layout
//!
//! The menu of the status item: Settings, About, a separator and Quit.

/// What choosing an item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenSettings,
    OpenAbout,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Item {
        /// Localization key of the title.
        title_key: &'static str,
        action: Action,
        /// Menu key equivalent (shown in the menu, active while the menu is open).
        key: &'static str,
    },
    Separator,
}

pub const ENTRIES: [Entry; 4] = [
    Entry::Item {
        title_key: "menu.settings",
        action: Action::OpenSettings,
        key: ",",
    },
    Entry::Item {
        title_key: "menu.about",
        action: Action::OpenAbout,
        key: "",
    },
    Entry::Separator,
    Entry::Item {
        title_key: "menu.quit",
        action: Action::Quit,
        key: "q",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use reshift_core::l10n::{self, AVAILABLE_LANGUAGES};

    fn actions() -> Vec<Action> {
        ENTRIES
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { action, .. } => Some(*action),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_has_exactly_settings_about_and_quit() {
        assert_eq!(
            actions(),
            [Action::OpenSettings, Action::OpenAbout, Action::Quit]
        );
    }

    #[test]
    fn quit_comes_last_below_a_separator() {
        assert_eq!(ENTRIES[ENTRIES.len() - 2], Entry::Separator);
        assert!(matches!(
            ENTRIES[ENTRIES.len() - 1],
            Entry::Item {
                action: Action::Quit,
                ..
            }
        ));
    }

    #[test]
    fn every_title_exists_in_every_language() {
        for language in AVAILABLE_LANGUAGES {
            for entry in ENTRIES {
                if let Entry::Item { title_key, .. } = entry {
                    let title = l10n::lookup(language.code, title_key);
                    assert_ne!(title, title_key, "{} lacks {title_key}", language.code);
                    assert!(!title.is_empty());
                }
            }
        }
    }

    #[test]
    fn extra_unneeded_items_stay_removed() {
        for removed in [
            "menu.header",
            "menu.convertText",
            "menu.activeLayouts",
            "menu.conversions",
        ] {
            assert!(
                ENTRIES.iter().all(|entry| !matches!(
                    entry,
                    Entry::Item { title_key, .. } if *title_key == removed
                )),
                "{removed} must not come back"
            );
            // The strings are gone too, not just hidden
            assert_eq!(l10n::lookup("en", removed), removed);
        }
    }

    #[test]
    fn key_equivalents_do_not_clash() {
        let keys: Vec<&str> = ENTRIES
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { key, .. } if !key.is_empty() => Some(*key),
                _ => None,
            })
            .collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(keys, [",", "q"]);
        assert_eq!(unique.len(), keys.len());
    }
}
