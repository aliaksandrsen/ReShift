//! MARK: - Keyboard Layout Model

/// A system keyboard layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyboardLayout {
    /// macOS input source ID, e.g. "com.apple.keylayout.Russian".
    pub id: String,
    /// e.g. "U.S." or "Russian".
    pub localized_name: String,
    /// e.g. "en", "ru".
    pub language_code: String,
}

impl KeyboardLayout {
    pub fn new(id: &str, localized_name: &str, language_code: &str) -> Self {
        Self {
            id: id.to_owned(),
            localized_name: localized_name.to_owned(),
            language_code: language_code.to_owned(),
        }
    }

    /// e.g. "Russian (RU)".
    pub fn display_name(&self) -> String {
        format!(
            "{} ({})",
            self.localized_name,
            self.language_code.to_uppercase()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_shows_the_language_code() {
        assert_eq!(
            KeyboardLayout::new("com.apple.keylayout.Russian", "Russian", "ru").display_name(),
            "Russian (RU)"
        );
    }
}
