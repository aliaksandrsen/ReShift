//! MARK: - Core Conversion Logic
//!
//! The conversion pipeline runs over the `System` trait (keys, clipboard, layouts, sound), so every
//! path - selection, last word, greedy line, nothing to convert, copy that never answers - is tested
//! on a fake system with a fake clock. The real system is `system::MacSystem`.
//!
//! Design choices:
//! - the work happens on its own dedicated worker thread without blocking the main UI thread;
//! - the copy is detected by the pasteboard's change count instead of a fixed wait: an app that
//!   answers in 10 ms costs only 10 ms. The clipboard is not cleared first, so "nothing was copied"
//!   shows as "the count did not change";
//! - the whole clipboard is saved (every type of every item, preserving images and formatting);
//! - the user's clipboard is restored only if nobody copied something new in the meantime;
//! - a copy that took the whole line without a selection (VS Code and alike) is ignored, avoiding
//!   duplicate line pastes.

use std::time::Duration;

use reshift_core::{
    ConversionResult, KeyboardLayout, LayoutMaps, LayoutSwitchMode, SmartConversionMode,
    TextConverter,
};

use crate::input::{Chord, chords};
use crate::log::debug;
use crate::settings::Settings;

/// How long to wait for the target app to put the selection on the clipboard.
pub const COPY_TIMEOUT: Duration = Duration::from_millis(200);
/// How often the clipboard's change count is looked at while waiting.
pub const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Time for the target app to apply a selection keystroke before the copy. Posted keys
/// reach one app in order, so most apps need none; this is a margin for slow ones.
pub const SELECTION_DELAY: Duration = Duration::from_millis(150);
/// Pause between the failed direct attempt and the smart one.
pub const SMART_PAUSE: Duration = Duration::from_millis(50);
/// How long the pasted text stays on the clipboard before the user's clipboard comes back.
pub const RESTORE_DELAY: Duration = Duration::from_millis(300);

/// What the conversion needs from the settings, copied so the worker never touches the main thread's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversionSettings {
    pub enabled_layouts: Vec<KeyboardLayout>,
    pub smart_conversion_mode: SmartConversionMode,
    /// The switch only ever happens after a successful conversion.
    pub layout_switch_mode: LayoutSwitchMode,
    pub play_sounds: bool,
}

impl From<&Settings> for ConversionSettings {
    fn from(settings: &Settings) -> Self {
        Self {
            enabled_layouts: settings.enabled_layouts.clone(),
            smart_conversion_mode: settings.smart_conversion_mode,
            layout_switch_mode: settings.layout_switch_mode,
            play_sounds: settings.play_sounds,
        }
    }
}

/// What a copy brought to the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub text: String,
    /// The app copied the whole line because nothing was selected.
    pub whole_line: bool,
}

/// Everything the pipeline does to the outside world.
pub trait System {
    /// The saved clipboard.
    type Snapshot;

    /// Posting keys works only with the Accessibility permission; without it they vanish silently.
    fn can_post(&self) -> bool;
    fn change_count(&self) -> i64;
    fn snapshot(&mut self) -> Self::Snapshot;
    fn restore(&mut self, snapshot: &Self::Snapshot);
    fn read_copied(&mut self) -> Option<Copied>;
    /// Puts `text` on the clipboard (marked transient and concealed) and returns the change count right
    /// after the write.
    fn write_text(&mut self, text: &str) -> i64;
    fn press(&mut self, chord: Chord);
    fn sleep(&mut self, duration: Duration);
    fn switch_layout(&mut self, layout_id: &str);
    fn play_sound(&mut self);
}

/// The user's clipboard waiting to come back after the paste.
pub struct PendingRestore<T> {
    pub snapshot: T,
    /// The change count right after our write: a different number means the user copied something new.
    pub our_change_count: i64,
    pub delay: Duration,
}

pub struct Outcome<T> {
    #[cfg_attr(not(test), expect(dead_code, reason = "tests assert it"))]
    pub converted: bool,
    pub pending_restore: Option<PendingRestore<T>>,
}

impl<T> Outcome<T> {
    fn nothing() -> Self {
        Self {
            converted: false,
            pending_restore: None,
        }
    }
}

/// Puts the user's clipboard back unless something new was copied since our paste.
pub fn apply_restore<S: System>(system: &mut S, pending: &PendingRestore<S::Snapshot>) {
    if system.change_count() == pending.our_change_count {
        system.restore(&pending.snapshot);
    }
}

/// One conversion, triggered by the shortcut. Never panics on its own; the caller guards it anyway.
pub fn perform<S: System>(
    system: &mut S,
    maps: &LayoutMaps,
    settings: &ConversionSettings,
) -> Outcome<S::Snapshot> {
    if !system.can_post() {
        debug!("performConversion: no Accessibility permission, nothing is sent");
        return Outcome::nothing();
    }
    let converter = TextConverter::new(maps, &settings.enabled_layouts);
    let snapshot = system.snapshot();
    let start_count = system.change_count();

    // Try the clipboard-based approach first (works when text is selected)
    let replaced = match direct(system, &converter) {
        Some(replaced) => {
            debug!("Direct conversion succeeded");
            Some(replaced)
        }
        None => {
            debug!("No selected text, trying smart conversion after short delay...");
            system.sleep(SMART_PAUSE);
            smart(system, &converter, settings.smart_conversion_mode)
        }
    };

    let Some(replaced) = replaced else {
        // Nothing was pasted: if the attempts touched the clipboard, put the user's content back
        if system.change_count() != start_count {
            system.restore(&snapshot);
        }
        return Outcome::nothing();
    };

    debug!(
        "converted {} characters into {}",
        replaced.input.chars().count(),
        replaced.output.chars().count()
    );
    // The switch only ever happens after a successful conversion, so all three modes behave the same
    system.switch_layout(&replaced.target_layout_id);
    if settings.play_sounds {
        debug!("Sound enabled, playing conversion sound");
        system.play_sound();
    } else {
        debug!("Sound disabled, conversion is silent");
    }
    Outcome {
        converted: true,
        pending_restore: Some(PendingRestore {
            snapshot,
            our_change_count: replaced.change_count,
            delay: RESTORE_DELAY,
        }),
    }
}

struct Replaced {
    input: String,
    output: String,
    target_layout_id: String,
    change_count: i64,
}

// MARK: - Selected text

/// Copy the selection, convert it, paste the result. `None` if nothing was selected or the converter
/// declined.
fn direct<S: System>(system: &mut S, converter: &TextConverter<'_>) -> Option<Replaced> {
    let Some(selected) = copy_selection(system) else {
        debug!("getAndReplaceSelectedText: no text on clipboard after copy");
        return None;
    };
    debug!(
        "getAndReplaceSelectedText: copied {} characters",
        selected.chars().count()
    );
    let Some(result) = converter.convert_selected_text_with_info(&selected) else {
        debug!("getAndReplaceSelectedText: converter returned nil");
        return None;
    };
    Some(paste(system, selected, result))
}

// MARK: - Smart selection

fn smart<S: System>(
    system: &mut S,
    converter: &TextConverter<'_>,
    mode: SmartConversionMode,
) -> Option<Replaced> {
    debug!("performSmartConversion() mode={mode:?}");
    match mode {
        SmartConversionMode::Disabled => {
            debug!("Smart conversion is disabled");
            None
        }
        // Last Word mode: select one word left, convert if it looks wrong
        SmartConversionMode::LastWord => select_and_replace(
            system,
            chords::SELECT_WORD_LEFT,
            "selectAndReplaceLastWord",
            |text| {
                if !converter.looks_like_wrong_layout(text) {
                    return None;
                }
                converter.convert_selected_text_with_info(text)
            },
        ),
        // Greedy Line mode: select to line start, find boundary, convert wrong-layout tail
        SmartConversionMode::GreedyLine => select_and_replace(
            system,
            chords::SELECT_TO_LINE_START,
            "selectLineAndReplace",
            |line| converter.convert_line_greedy_with_info(line),
        ),
    }
}

/// Select with `select`, copy, let `convert` decide, paste the replacement over the selection. When
/// there is nothing to replace the selection is collapsed again with the Right arrow.
fn select_and_replace<S: System>(
    system: &mut S,
    select: Chord,
    name: &str,
    convert: impl FnOnce(&str) -> Option<ConversionResult>,
) -> Option<Replaced> {
    system.press(select);
    system.sleep(SELECTION_DELAY);
    let Some(selected) = copy_selection(system) else {
        debug!("{name}: no text copied, deselecting");
        system.press(chords::RIGHT_ARROW);
        return None;
    };
    debug!("{name}: copied {} characters", selected.chars().count());
    let Some(result) = convert(&selected) else {
        debug!("{name}: converter returned nil, deselecting");
        // No conversion needed - deselect by pressing Right arrow
        system.press(chords::RIGHT_ARROW);
        return None;
    };
    Some(paste(system, selected, result))
}

// MARK: - Clipboard round trip

/// Copies the selection and returns the text, or `None` if the app did not answer in time, copied
/// nothing, or copied a whole line that was not selected.
fn copy_selection<S: System>(system: &mut S) -> Option<String> {
    let before = system.change_count();
    system.press(chords::COPY);
    // Nothing selected => most apps do not touch the clipboard at all
    if !wait_for_change(system, before) {
        return None;
    }
    let copied = system.read_copied()?;
    if copied.text.is_empty() {
        return None;
    }
    if copied.whole_line {
        debug!("copy: the app copied the whole line without a selection - ignoring");
        return None;
    }
    Some(copied.text)
}

fn wait_for_change<S: System>(system: &mut S, before: i64) -> bool {
    let polls = COPY_TIMEOUT.as_millis() / POLL_INTERVAL.as_millis();
    for _ in 0..polls {
        if system.change_count() != before {
            return true;
        }
        system.sleep(POLL_INTERVAL);
    }
    system.change_count() != before
}

fn paste<S: System>(system: &mut S, input: String, result: ConversionResult) -> Replaced {
    let change_count = system.write_text(&result.text);
    system.press(chords::PASTE);
    Replaced {
        input,
        output: result.text,
        target_layout_id: result.target_layout_id,
        change_count,
    }
}

// MARK: - Whole-line copies

/// VS Code writes `"isFromEmptySelection":true` into the `vscode-editor-data` flavor of a copy made
/// without a selection (it copies the whole line then). Pasting that over nothing would duplicate the
/// line, so such a copy does not count as a selection.
pub fn is_vs_code_empty_selection_copy(editor_data: Option<&[u8]>) -> bool {
    const MARKER: &[u8] = b"\"isFromEmptySelection\":true";
    editor_data.is_some_and(|data| data.windows(MARKER.len()).any(|window| window == MARKER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::MARKER;
    use reshift_core::layout_char_map::LayoutMaps;

    const US: &str = "com.apple.keylayout.US";
    const RUSSIAN: &str = "com.apple.keylayout.Russian";

    fn layouts() -> Vec<KeyboardLayout> {
        vec![
            KeyboardLayout::new(US, "U.S.", "en"),
            KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
        ]
    }

    fn settings(mode: SmartConversionMode) -> ConversionSettings {
        ConversionSettings {
            enabled_layouts: layouts(),
            smart_conversion_mode: mode,
            layout_switch_mode: LayoutSwitchMode::Always,
            play_sounds: true,
        }
    }

    /// A text field with a cursor, a clipboard and an app that reacts to the keys we post.
    #[derive(Default)]
    struct Fake {
        permitted: bool,
        // The app
        selection: Option<String>,
        word_left: String,
        line_left: String,
        /// Cmd+C with no selection copies the whole line (VS Code, JetBrains)
        copies_line_without_selection: bool,
        /// The app never answers a copy
        deaf: bool,
        // The clipboard
        clipboard: Option<String>,
        whole_line_flag: bool,
        count: i64,
        // What we did
        keys: Vec<Chord>,
        slept: Duration,
        pasted: Vec<String>,
        switched: Vec<String>,
        sounds: u32,
        restores: u32,
        writes: Vec<String>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                permitted: true,
                clipboard: Some("the user's own clipboard".to_owned()),
                ..Self::default()
            }
        }

        fn selected(mut self, text: &str) -> Self {
            self.selection = Some(text.to_owned());
            self
        }

        fn line(mut self, line: &str) -> Self {
            self.line_left = line.to_owned();
            self
        }

        fn word(mut self, word: &str) -> Self {
            self.word_left = word.to_owned();
            self
        }
    }

    impl System for Fake {
        type Snapshot = Option<String>;

        fn can_post(&self) -> bool {
            self.permitted
        }

        fn change_count(&self) -> i64 {
            self.count
        }

        fn snapshot(&mut self) -> Self::Snapshot {
            self.clipboard.clone()
        }

        fn restore(&mut self, snapshot: &Self::Snapshot) {
            self.clipboard.clone_from(snapshot);
            self.whole_line_flag = false;
            self.count += 1;
            self.restores += 1;
        }

        fn read_copied(&mut self) -> Option<Copied> {
            Some(Copied {
                text: self.clipboard.clone()?,
                whole_line: self.whole_line_flag,
            })
        }

        fn write_text(&mut self, text: &str) -> i64 {
            self.clipboard = Some(text.to_owned());
            self.whole_line_flag = false;
            self.count += 1;
            self.writes.push(text.to_owned());
            self.count
        }

        fn press(&mut self, chord: Chord) {
            self.keys.push(chord);
            if chord == chords::COPY {
                if self.deaf {
                    return;
                }
                if let Some(selection) = self.selection.clone() {
                    self.clipboard = Some(selection);
                    self.whole_line_flag = false;
                    self.count += 1;
                } else if self.copies_line_without_selection {
                    self.clipboard = Some(self.line_left.clone());
                    self.whole_line_flag = true;
                    self.count += 1;
                }
            } else if chord == chords::SELECT_WORD_LEFT {
                self.selection = Some(self.word_left.clone());
            } else if chord == chords::SELECT_TO_LINE_START {
                self.selection = Some(self.line_left.clone());
            } else if chord == chords::RIGHT_ARROW {
                self.selection = None;
            } else if chord == chords::PASTE {
                self.pasted.extend(self.clipboard.clone());
                self.selection = None;
            }
        }

        fn sleep(&mut self, duration: Duration) {
            self.slept += duration;
        }

        fn switch_layout(&mut self, layout_id: &str) {
            self.switched.push(layout_id.to_owned());
        }

        fn play_sound(&mut self) {
            self.sounds += 1;
        }
    }

    fn run(fake: &mut Fake, mode: SmartConversionMode) -> Outcome<Option<String>> {
        perform(fake, LayoutMaps::builtin(), &settings(mode))
    }

    // MARK: - Selected text

    #[test]
    fn a_selection_is_converted_and_pasted_over_itself() {
        let mut fake = Fake::new().selected("ghbdtn");
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(outcome.converted);
        assert_eq!(fake.pasted, ["привет"]);
        assert_eq!(fake.keys, [chords::COPY, chords::PASTE], "no smart attempt");
    }

    #[test]
    fn a_russian_selection_becomes_latin_and_the_layout_switches_to_it() {
        let mut fake = Fake::new().selected("руддщ");
        run(&mut fake, SmartConversionMode::Disabled);
        assert_eq!(fake.pasted, ["hello"]);
        assert_eq!(fake.switched, [US]);
    }

    #[test]
    fn success_switches_the_layout() {
        let mut fake = Fake::new().selected("ghbdtn rfr");
        run(&mut fake, SmartConversionMode::GreedyLine);
        assert_eq!(fake.switched, [RUSSIAN]);
    }

    #[test]
    fn sound_follows_its_setting() {
        let mut fake = Fake::new().selected("ghbdtn");
        let mut with_sound = settings(SmartConversionMode::GreedyLine);
        with_sound.play_sounds = true;
        perform(&mut fake, LayoutMaps::builtin(), &with_sound);
        assert_eq!(fake.sounds, 1);

        let mut fake = Fake::new().selected("ghbdtn");
        let mut quiet = settings(SmartConversionMode::GreedyLine);
        quiet.play_sounds = false;
        perform(&mut fake, LayoutMaps::builtin(), &quiet);
        assert_eq!(fake.sounds, 0);
        assert_eq!(
            fake.pasted,
            ["привет"],
            "the conversion itself does not depend on sound"
        );
    }

    #[test]
    fn all_three_layout_switch_modes_switch_after_a_conversion() {
        for mode in LayoutSwitchMode::ALL {
            let mut fake = Fake::new().selected("ghbdtn");
            let mut with_mode = settings(SmartConversionMode::GreedyLine);
            with_mode.layout_switch_mode = mode;
            perform(&mut fake, LayoutMaps::builtin(), &with_mode);
            assert_eq!(fake.switched, [RUSSIAN], "{mode:?}");
        }
    }

    #[test]
    fn a_selection_that_cannot_be_converted_falls_through_to_the_smart_mode() {
        // Selection cannot be converted, so smart conversion runs
        let mut fake = Fake::new()
            .selected("\u{1F44B}\u{1F3FD}")
            .line("\u{1F44B}\u{1F3FD}");
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(!outcome.converted);
        assert!(fake.keys.contains(&chords::SELECT_TO_LINE_START));
        assert!(fake.pasted.is_empty());
    }

    // MARK: - Greedy line

    #[test]
    fn without_a_selection_greedy_line_converts_only_the_wrong_tail() {
        let mut fake = Fake::new().line("Привет ghbdtn rfr");
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(outcome.converted);
        assert_eq!(fake.pasted, ["Привет привет как"]);
    }

    #[test]
    fn greedy_line_presses_the_keys_in_the_expected_order() {
        let mut fake = Fake::new().line("ghbdtn");
        run(&mut fake, SmartConversionMode::GreedyLine);
        assert_eq!(
            fake.keys,
            [
                chords::COPY,                 // direct attempt: nothing selected, no answer
                chords::SELECT_TO_LINE_START, // smart: select to the line start
                chords::COPY,
                chords::PASTE,
            ]
        );
    }

    #[test]
    fn a_line_that_is_all_correct_is_left_alone_and_deselected() {
        let mut fake = Fake::new().line("12 34");
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(!outcome.converted);
        assert_eq!(fake.keys.last(), Some(&chords::RIGHT_ARROW));
        assert!(fake.pasted.is_empty() && fake.switched.is_empty());
        assert_eq!(fake.sounds, 0);
    }

    // MARK: - Last word

    #[test]
    fn last_word_mode_selects_one_word_to_the_left() {
        let mut fake = Fake::new().word("ghbdtn");
        let outcome = run(&mut fake, SmartConversionMode::LastWord);
        assert!(outcome.converted);
        assert_eq!(fake.pasted, ["привет"]);
        assert!(fake.keys.contains(&chords::SELECT_WORD_LEFT));
        assert!(!fake.keys.contains(&chords::SELECT_TO_LINE_START));
    }

    #[test]
    fn last_word_mode_leaves_a_word_that_looks_right() {
        let mut fake = Fake::new().word("2024");
        let outcome = run(&mut fake, SmartConversionMode::LastWord);
        assert!(!outcome.converted);
        assert_eq!(fake.keys.last(), Some(&chords::RIGHT_ARROW));
        assert!(fake.pasted.is_empty());
    }

    // MARK: - Disabled

    #[test]
    fn disabled_smart_mode_only_tries_the_selection() {
        let mut fake = Fake::new().line("ghbdtn");
        let outcome = run(&mut fake, SmartConversionMode::Disabled);
        assert!(!outcome.converted);
        assert_eq!(fake.keys, [chords::COPY], "no selection keys at all");
        assert!(fake.pasted.is_empty());
    }

    // MARK: - Failures

    #[test]
    fn an_app_that_never_answers_the_copy_is_given_up_on_after_the_timeout() {
        let mut fake = Fake::new().line("ghbdtn");
        fake.deaf = true;
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(!outcome.converted);
        // Two copies (direct, smart) waited the whole timeout; nothing waited forever
        let waited = COPY_TIMEOUT * 2 + SMART_PAUSE + SELECTION_DELAY;
        assert!(
            fake.slept >= waited - POLL_INTERVAL * 2 && fake.slept <= waited + POLL_INTERVAL * 4,
            "slept {:?}, expected about {waited:?}",
            fake.slept
        );
    }

    #[test]
    fn an_app_that_answers_at_once_costs_almost_no_waiting() {
        let mut fake = Fake::new().selected("ghbdtn");
        run(&mut fake, SmartConversionMode::GreedyLine);
        assert_eq!(
            fake.slept,
            Duration::ZERO,
            "the first poll already sees the copy"
        );
    }

    #[test]
    fn a_whole_line_copy_without_a_selection_is_not_pasted_back() {
        // VS Code: Cmd+C with no selection copies the line; pasting that would duplicate it
        let mut fake = Fake::new().line("12 34");
        fake.copies_line_without_selection = true;
        let outcome = run(&mut fake, SmartConversionMode::Disabled);
        assert!(!outcome.converted);
        assert!(fake.pasted.is_empty());
        assert!(
            fake.writes.is_empty(),
            "the clipboard was not written to either"
        );
    }

    #[test]
    fn a_whole_line_copy_does_not_stop_the_smart_mode_from_working() {
        let mut fake = Fake::new().line("ghbdtn");
        fake.copies_line_without_selection = true;
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(outcome.converted);
        assert_eq!(fake.pasted, ["привет"]);
    }

    #[test]
    fn nothing_happens_without_the_accessibility_permission() {
        let mut fake = Fake::new().selected("ghbdtn");
        fake.permitted = false;
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert!(!outcome.converted);
        assert!(fake.keys.is_empty(), "posted keys would only vanish");
        assert_eq!(fake.count, 0);
    }

    #[test]
    fn fewer_than_two_layouts_convert_nothing() {
        let mut fake = Fake::new().selected("ghbdtn");
        let mut one = settings(SmartConversionMode::GreedyLine);
        one.enabled_layouts.truncate(1);
        let outcome = perform(&mut fake, LayoutMaps::builtin(), &one);
        assert!(!outcome.converted);
        assert!(fake.pasted.is_empty());
    }

    #[test]
    fn an_empty_copy_counts_as_no_selection() {
        let mut fake = Fake::new().selected("");
        let outcome = run(&mut fake, SmartConversionMode::Disabled);
        assert!(!outcome.converted);
        assert!(fake.pasted.is_empty());
    }

    // MARK: - The user's clipboard

    #[test]
    fn the_user_clipboard_is_not_touched_when_nothing_was_copied() {
        let mut fake = Fake::new();
        run(&mut fake, SmartConversionMode::Disabled);
        assert_eq!(fake.restores, 0);
        assert_eq!(fake.clipboard.as_deref(), Some("the user's own clipboard"));
    }

    #[test]
    fn a_copy_that_led_to_nothing_puts_the_user_clipboard_back_at_once() {
        let mut fake = Fake::new().selected("\u{1F44B}\u{1F3FD}");
        let outcome = run(&mut fake, SmartConversionMode::Disabled);
        assert!(!outcome.converted);
        assert_eq!(fake.restores, 1);
        assert_eq!(fake.clipboard.as_deref(), Some("the user's own clipboard"));
    }

    #[test]
    fn after_a_paste_the_user_clipboard_comes_back_later_not_at_once() {
        let mut fake = Fake::new().selected("ghbdtn");
        let outcome = run(&mut fake, SmartConversionMode::GreedyLine);
        assert_eq!(fake.restores, 0, "the app has not pasted yet");
        assert_eq!(fake.clipboard.as_deref(), Some("привет"));
        let pending = outcome.pending_restore.unwrap();
        assert_eq!(pending.delay, RESTORE_DELAY);
        apply_restore(&mut fake, &pending);
        assert_eq!(fake.clipboard.as_deref(), Some("the user's own clipboard"));
    }

    #[test]
    fn the_clipboard_is_left_alone_if_the_user_copied_something_in_between() {
        let mut fake = Fake::new().selected("ghbdtn");
        let pending = run(&mut fake, SmartConversionMode::GreedyLine)
            .pending_restore
            .unwrap();
        // The user presses Cmd+C in another app within the 300 ms
        fake.clipboard = Some("fresh copy".to_owned());
        fake.count += 1;
        apply_restore(&mut fake, &pending);
        assert_eq!(fake.clipboard.as_deref(), Some("fresh copy"));
    }

    #[test]
    fn a_failed_conversion_has_nothing_to_restore_later() {
        let mut fake = Fake::new().line("12 34");
        assert!(
            run(&mut fake, SmartConversionMode::GreedyLine)
                .pending_restore
                .is_none()
        );
    }

    #[test]
    fn the_snapshot_is_taken_before_anything_changes_the_clipboard() {
        // The direct attempt copies the selection (clipboard changes) and the smart attempt copies again:
        // the restore must still bring back the very first content
        let mut fake = Fake::new()
            .selected("\u{1F44B}\u{1F3FD}")
            .line("\u{1F44B}\u{1F3FD}");
        run(&mut fake, SmartConversionMode::GreedyLine);
        assert_eq!(fake.clipboard.as_deref(), Some("the user's own clipboard"));
    }

    // MARK: - Pieces

    #[test]
    fn the_settings_are_copied_from_the_app_settings() {
        let mut app = Settings::defaults(layouts());
        app.play_sounds = false;
        app.smart_conversion_mode = SmartConversionMode::LastWord;
        app.layout_switch_mode = LayoutSwitchMode::IfAnyWordConverted;
        let copy = ConversionSettings::from(&app);
        assert_eq!(copy.enabled_layouts, layouts());
        assert!(!copy.play_sounds);
        assert_eq!(copy.smart_conversion_mode, SmartConversionMode::LastWord);
        assert_eq!(
            copy.layout_switch_mode,
            LayoutSwitchMode::IfAnyWordConverted
        );
    }

    #[test]
    fn vs_code_marks_a_copy_made_without_a_selection() {
        let marked = br#"{"version":1,"isFromEmptySelection":true,"multicursorText":null,"mode":"plaintext"}"#;
        let unmarked = br#"{"version":1,"isFromEmptySelection":false,"multicursorText":null}"#;
        assert!(is_vs_code_empty_selection_copy(Some(marked)));
        assert!(!is_vs_code_empty_selection_copy(Some(unmarked)));
        assert!(!is_vs_code_empty_selection_copy(None));
        assert!(!is_vs_code_empty_selection_copy(Some(b"")));
        assert!(!is_vs_code_empty_selection_copy(Some(
            b"isFromEmptySelection"
        )));
    }

    #[test]
    fn the_timeouts_are_sane() {
        assert!(POLL_INTERVAL < COPY_TIMEOUT);
        assert_eq!(COPY_TIMEOUT.as_millis() % POLL_INTERVAL.as_millis(), 0);
        assert!(RESTORE_DELAY >= Duration::from_millis(100));
        // Everything the user waits for with no selection, in the worst case: two copies + pauses
        assert!(COPY_TIMEOUT * 2 + SMART_PAUSE + SELECTION_DELAY < Duration::from_millis(700));
    }

    #[test]
    fn our_marker_is_distinct_from_cg_event_mask() {
        // The marker lives in eventSourceUserData (an integer field), not in event flags
        assert_eq!(MARKER, 0x4C53_5743);
    }
}
