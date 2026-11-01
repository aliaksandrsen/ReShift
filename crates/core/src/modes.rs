//! Conversion settings stored as numbers in UserDefaults.

// MARK: - Smart Conversion Mode

/// Determines behavior when no text is selected and the hotkey is pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SmartConversionMode {
    /// Convert only the last word.
    LastWord = 0,
    /// Select to line start, find the boundary, convert the wrong-layout tail.
    #[default]
    GreedyLine = 1,
    /// No smart conversion - only works with an explicit selection.
    Disabled = 2,
}

impl SmartConversionMode {
    pub const ALL: [Self; 3] = [Self::LastWord, Self::GreedyLine, Self::Disabled];

    pub fn from_i64(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| *mode as i64 == value)
    }

    pub fn name_key(self) -> &'static str {
        match self {
            Self::LastWord => "smartMode.lastWord.name",
            Self::GreedyLine => "smartMode.greedyLine.name",
            Self::Disabled => "smartMode.disabled.name",
        }
    }

    pub fn description_key(self) -> &'static str {
        match self {
            Self::LastWord => "smartMode.lastWord.desc",
            Self::GreedyLine => "smartMode.greedyLine.desc",
            Self::Disabled => "smartMode.disabled.desc",
        }
    }
}

// MARK: - Layout Switch Mode

/// Controls whether the keyboard layout is switched after a hotkey-triggered conversion.
/// Note: as in the macOS app, the switch only ever happens after a successful conversion,
/// so all three modes currently behave the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayoutSwitchMode {
    /// Always switch layout when the hotkey is pressed.
    #[default]
    Always = 0,
    /// Switch if the last word was in the wrong layout and got reconverted.
    IfLastWordConverted = 1,
    /// Switch if any word in the converted text needed reconversion.
    IfAnyWordConverted = 2,
}

impl LayoutSwitchMode {
    pub const ALL: [Self; 3] = [
        Self::Always,
        Self::IfLastWordConverted,
        Self::IfAnyWordConverted,
    ];

    pub fn from_i64(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| *mode as i64 == value)
    }

    pub fn name_key(self) -> &'static str {
        match self {
            Self::Always => "layoutSwitchMode.always.name",
            Self::IfLastWordConverted => "layoutSwitchMode.ifLastWord.name",
            Self::IfAnyWordConverted => "layoutSwitchMode.ifAnyWord.name",
        }
    }

    pub fn description_key(self) -> &'static str {
        match self {
            Self::Always => "layoutSwitchMode.always.desc",
            Self::IfLastWordConverted => "layoutSwitchMode.ifLastWord.desc",
            Self::IfAnyWordConverted => "layoutSwitchMode.ifAnyWord.desc",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_values_match_the_other_apps() {
        assert_eq!(
            SmartConversionMode::from_i64(0),
            Some(SmartConversionMode::LastWord)
        );
        assert_eq!(
            SmartConversionMode::from_i64(1),
            Some(SmartConversionMode::GreedyLine)
        );
        assert_eq!(
            SmartConversionMode::from_i64(2),
            Some(SmartConversionMode::Disabled)
        );
        assert_eq!(SmartConversionMode::from_i64(3), None);
        assert_eq!(
            LayoutSwitchMode::from_i64(2),
            Some(LayoutSwitchMode::IfAnyWordConverted)
        );
    }

    #[test]
    fn defaults_match_the_other_apps() {
        assert_eq!(
            SmartConversionMode::default(),
            SmartConversionMode::GreedyLine
        );
        assert_eq!(LayoutSwitchMode::default(), LayoutSwitchMode::Always);
    }
}
