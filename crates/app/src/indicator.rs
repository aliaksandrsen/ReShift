//! MARK: - Menu Bar Indicator
//!
//! Maps current keyboard layout to either a country flag emoji or uppercase 2-letter language code.
//! By user request, replaces the generic keyboard symbol in the status bar with the current layout's
//! visual indicator (like in Punto Switcher).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IndicatorStyle {
    #[default]
    Flag = 0,
    Letters = 1,
}

impl IndicatorStyle {
    pub const ALL: [Self; 2] = [Self::Flag, Self::Letters];

    pub fn from_i64(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|s| *s as i64 == value)
    }

    pub fn name_key(self) -> &'static str {
        match self {
            Self::Flag => "general.indicatorFlag",
            Self::Letters => "general.indicatorLetters",
        }
    }
}

/// The display text (flag emoji or uppercase letters) for the menu bar button.
pub fn indicator_text(style: IndicatorStyle, layout_id: &str, language_code: &str) -> String {
    match style {
        IndicatorStyle::Flag => {
            flag_for_layout(layout_id).unwrap_or_else(|| letters_for_language(language_code))
        }
        IndicatorStyle::Letters => letters_for_language(language_code),
    }
}

fn flag_for_layout(layout_id: &str) -> Option<String> {
    let lower = layout_id.to_ascii_lowercase();
    if lower.contains("russian") {
        Some("🇷🇺".to_owned())
    } else if lower.contains("british") {
        Some("🇬🇧".to_owned())
    } else if lower.contains("us")
        || lower.contains("abc")
        || lower.contains("australian")
        || lower.contains("canadian")
    {
        Some("🇺🇸".to_owned())
    } else if lower.contains("ukrainian") {
        Some("🇺🇦".to_owned())
    } else if lower.contains("austrian") {
        Some("🇦🇹".to_owned())
    } else if lower.contains("swiss") {
        Some("🇨🇭".to_owned())
    } else if lower.contains("german") {
        Some("🇩🇪".to_owned())
    } else if lower.contains("french") {
        Some("🇫🇷".to_owned())
    } else if lower.contains("spanish") {
        Some("🇪🇸".to_owned())
    } else if lower.contains("italian") {
        Some("🇮🇹".to_owned())
    } else if lower.contains("polish") {
        Some("🇵🇱".to_owned())
    } else {
        None
    }
}

fn letters_for_language(lang: &str) -> String {
    let code = lang.trim();
    if code.len() >= 2 {
        code[..2].to_ascii_uppercase()
    } else if !code.is_empty() {
        code.to_ascii_uppercase()
    } else {
        "??".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_maps_from_stored_number() {
        assert_eq!(IndicatorStyle::from_i64(0), Some(IndicatorStyle::Flag));
        assert_eq!(IndicatorStyle::from_i64(1), Some(IndicatorStyle::Letters));
        assert_eq!(IndicatorStyle::from_i64(2), None);
    }

    #[test]
    fn flag_mode_shows_country_flag_for_known_layouts() {
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.RussianWin", "ru"),
            "🇷🇺"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.US", "en"),
            "🇺🇸"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.German", "de"),
            "🇩🇪"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.Ukrainian", "uk"),
            "🇺🇦"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.British", "en"),
            "🇬🇧"
        );
    }

    #[test]
    fn flag_mode_falls_back_to_letters_for_unknown_layout() {
        assert_eq!(
            indicator_text(IndicatorStyle::Flag, "com.apple.keylayout.Odd", "zh"),
            "ZH"
        );
    }

    #[test]
    fn letters_mode_shows_two_uppercase_letters() {
        assert_eq!(
            indicator_text(
                IndicatorStyle::Letters,
                "com.apple.keylayout.RussianWin",
                "ru"
            ),
            "RU"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Letters, "com.apple.keylayout.US", "en"),
            "EN"
        );
        assert_eq!(
            indicator_text(IndicatorStyle::Letters, "com.apple.keylayout.German", "de"),
            "DE"
        );
    }
}
