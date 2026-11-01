// MARK: - English Strings
// To add a new language, copy this file as xx.rs, translate every value and register it in
// mod.rs (AVAILABLE_LANGUAGES and TABLES).

pub(super) static STRINGS: &[(&str, &str)] = &[
    // -- Menu bar ---------------------------------------------------------
    ("menu.settings", "Settings..."),
    ("menu.about", "About ReShift"),
    ("menu.quit", "Quit ReShift"),
    ("menu.tooltip", "ReShift - %@ to convert"),
    // -- Settings Window --------------------------------------------------
    ("settings.windowTitle", "ReShift Settings"),
    ("settings.tab.general", "General"),
    ("settings.tab.layouts", "Layouts"),
    ("settings.tab.hotkey", "Hotkey"),
    ("settings.tab.permissions", "Permissions"),
    // -- General Tab ------------------------------------------------------
    ("general.language", "Language"),
    ("general.launchAtLogin", "Launch at Login"),
    ("general.playSound", "Play Sound on Conversion"),
    ("general.smartConversion", "Smart Conversion (No Selection)"),
    ("general.mode", "Mode:"),
    ("general.indicator", "Menu Bar Icon:"),
    ("general.indicatorFlag", "Flag (🇷🇺, 🇺🇸)"),
    ("general.indicatorLetters", "Letters (RU, EN)"),
    ("general.layoutSwitch", "Layout Switch after Conversion"),
    ("general.howItWorks", "How It Works"),
    (
        "general.howItWorksText",
        "When text is selected, the shortcut converts the selection.\nWhen no text is selected, the behavior depends on the Smart Conversion mode above.",
    ),
    // -- Smart Conversion Modes -------------------------------------------
    ("smartMode.lastWord.name", "Last Word Only"),
    ("smartMode.greedyLine.name", "Greedy (Entire Phrase)"),
    ("smartMode.disabled.name", "Disabled"),
    (
        "smartMode.lastWord.desc",
        "Converts only the last typed word before the cursor.",
    ),
    (
        "smartMode.greedyLine.desc",
        "Selects text to the start of line, finds where the wrong layout begins, and converts the entire wrong-layout phrase.",
    ),
    (
        "smartMode.disabled.desc",
        "Smart conversion is off. You must manually select text before pressing the hotkey.",
    ),
    // -- Layout Switch Modes ----------------------------------------------
    ("layoutSwitchMode.always.name", "Always switch layout"),
    (
        "layoutSwitchMode.ifLastWord.name",
        "Switch if last word was converted",
    ),
    (
        "layoutSwitchMode.ifAnyWord.name",
        "Switch if any word required conversion",
    ),
    (
        "layoutSwitchMode.always.desc",
        "Always switch the keyboard layout after pressing the hotkey, regardless of whether text was converted.",
    ),
    (
        "layoutSwitchMode.ifLastWord.desc",
        "Switch the layout only if the last word before the cursor was typed in the wrong layout and got reconverted.",
    ),
    (
        "layoutSwitchMode.ifAnyWord.desc",
        "Switch the layout if any word in the converted text needed reconversion to the correct layout.",
    ),
    // -- Layouts Tab ------------------------------------------------------
    ("layouts.title", "Active Keyboard Layouts"),
    (
        "layouts.description",
        "These layouts are detected from your system keyboard settings. The converter maps characters between them based on physical key positions.",
    ),
    ("layouts.refresh", "Refresh from System"),
    ("layouts.count", "%d layouts detected"),
    // -- Hotkey Tab -------------------------------------------------------
    ("hotkey.title", "Keyboard Shortcut"),
    (
        "hotkey.description",
        "Press this shortcut to convert text. If text is selected it converts the selection; otherwise Smart Conversion selects the text before the cursor.",
    ),
    ("hotkey.current", "Current shortcut:"),
    ("hotkey.useDoubleShift", "Use Double Shift (⇧⇧) as shortcut"),
    (
        "hotkey.doubleShiftHint",
        "Quickly press Shift twice to trigger conversion. This is the recommended shortcut - fast and doesn't conflict with other apps.",
    ),
    ("hotkey.doubleShiftDescription", "⇧⇧ (Double Shift)"),
    ("hotkey.customTitle", "Custom Shortcut"),
    (
        "hotkey.requireModifier",
        "Use at least one of Control, Option or Command.",
    ),
    // -- Hotkey Recorder --------------------------------------------------
    (
        "hotkeyRecorder.prompt",
        "Click \"Record\" and press your desired shortcut:",
    ),
    ("hotkeyRecorder.pressKey", "Press a key combination..."),
    ("hotkeyRecorder.cancel", "Cancel"),
    ("hotkeyRecorder.record", "Record"),
    // -- Permissions Tab --------------------------------------------------
    ("permissions.title", "Permissions"),
    (
        "permissions.description",
        "ReShift needs Accessibility access to read and replace selected text in other applications.",
    ),
    ("permissions.accessibilityTitle", "Accessibility"),
    ("permissions.granted", "Granted - ReShift can convert text"),
    (
        "permissions.notGranted",
        "Not granted - Please enable in System Settings",
    ),
    ("permissions.grantAccess", "Grant Access"),
    ("permissions.howToEnable", "How to enable:"),
    (
        "permissions.step1",
        "1. Open System Settings > Privacy & Security > Accessibility",
    ),
    (
        "permissions.step2",
        "2. Click the lock icon to make changes",
    ),
    ("permissions.step3", "3. Enable ReShift in the list"),
    ("permissions.step4", "4. Restart ReShift if needed"),
    ("permissions.openSettings", "Open System Settings"),
    ("permissions.refreshStatus", "Refresh Status"),
    // -- About Window -----------------------------------------------------
    ("about.windowTitle", "About ReShift"),
    ("about.appName", "ReShift"),
    ("about.version", "Version %@"),
    (
        "about.tagline",
        "Open-source keyboard layout text converter for macOS",
    ),
    ("about.howToUse", "How to use:"),
    ("about.step1", "Select text typed in wrong layout"),
    ("about.step2", "Press ⇧⇧ (double Shift)"),
    ("about.step3", "Text is automatically converted!"),
    ("about.license", "MIT License"),
    // -- Alerts -----------------------------------------------------------
    ("alert.accessibilityTitle", "Accessibility Access Required"),
    (
        "alert.accessibilityMessage",
        "ReShift needs Accessibility permission to read and replace text.\n\nGo to System Settings > Privacy & Security > Accessibility and enable ReShift.\n\nIf you already granted permission but it still does not work, remove ReShift from the list, add it again and restart the app. ReShift may also need to be listed under Privacy & Security > Input Monitoring.",
    ),
    ("alert.openSystemSettings", "Open System Settings"),
    ("alert.continueAnyway", "Continue Anyway"),
    // -- Common -----------------------------------------------------------
    ("common.cancel", "Cancel"),
];
