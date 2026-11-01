//! MARK: - Keyboard Layout Detector
//!
//! Installed keyboard layouts through the Text Input Sources API (Carbon).
//! The choice of layouts is a pure function over `RawSource`
//! values, tested without the system; the FFI part only fills those in.

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_core_foundation::{CFArray, CFData, CFRetained, CFString, CFType};
use reshift_core::KeyboardLayout;
use std::ffi::c_void;
use std::ptr::NonNull;

/// What the system tells about one input source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSource {
    pub id: String,
    pub localized_name: String,
    /// First language of the source, e.g. "ru".
    pub language: Option<String>,
    /// `kTISTypeKeyboardLayout`: a plain layout, not an input method (Pinyin, emoji picker, ...).
    pub is_keyboard_layout: bool,
    /// Layout data (`uchr`) the character map can be derived from.
    pub has_layout_data: bool,
}

/// Layouts to convert between: real keyboard layouts that a character map can be built for, in the
/// system order. Maps are derived from the system, so any layout with data qualifies.
/// Falls back to US + Russian if nothing is left.
pub fn select_layouts(sources: &[RawSource]) -> Vec<KeyboardLayout> {
    let layouts: Vec<KeyboardLayout> = sources
        .iter()
        .filter(|source| source.is_keyboard_layout && source.has_layout_data)
        .map(|source| {
            KeyboardLayout::new(
                &source.id,
                &source.localized_name,
                source.language.as_deref().unwrap_or("unknown"),
            )
        })
        .collect();
    if layouts.is_empty() {
        default_layouts()
    } else {
        layouts
    }
}

/// Default fallback layouts.
pub fn default_layouts() -> Vec<KeyboardLayout> {
    vec![
        KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
        KeyboardLayout::new("com.apple.keylayout.Russian", "Russian", "ru"),
    ]
}

/// The layouts enabled in System Settings > Keyboard.
pub fn installed_layouts() -> Vec<KeyboardLayout> {
    select_layouts(&enabled_sources())
}

// MARK: - Text Input Sources (FFI)

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyInputSourceID: &'static CFString;
    static kTISPropertyLocalizedName: &'static CFString;
    static kTISPropertyInputSourceLanguages: &'static CFString;
    static kTISPropertyInputSourceType: &'static CFString;
    static kTISPropertyUnicodeKeyLayoutData: &'static CFString;
    static kTISTypeKeyboardLayout: &'static CFString;

    fn TISCreateInputSourceList(
        properties: *const c_void,
        include_all_installed: bool,
    ) -> Option<NonNull<CFArray<CFType>>>;
    fn TISCopyCurrentKeyboardInputSource() -> Option<NonNull<CFType>>;
    fn TISGetInputSourceProperty(source: &CFType, key: &CFString) -> *const c_void;
    fn TISSelectInputSource(source: &CFType) -> i32;
    fn KBGetLayoutType(keyboard_type: u32) -> u32;
    fn UCKeyTranslate(
        key_layout: *const u8,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
}

/// The enabled input sources, `include_all_installed = false`.
fn input_source_list() -> Option<CFRetained<CFArray<CFType>>> {
    // SAFETY: a null filter lists every source; the result follows the create rule
    let list = unsafe { TISCreateInputSourceList(std::ptr::null(), false) }?;
    // SAFETY: owned reference from a "Create" function
    Some(unsafe { CFRetained::from_raw(list) })
}

/// String property of a source, `None` if absent. The value is only borrowed from the source.
fn string_property(source: &CFType, key: &CFString) -> Option<String> {
    // SAFETY: valid source and key; the result is a CFString or null
    let value = unsafe { TISGetInputSourceProperty(source, key) };
    // SAFETY: a non-null result of a string property is a live CFString for as long as `source` is
    unsafe { value.cast::<CFString>().as_ref() }.map(|string| string.to_string())
}

fn raw_source(source: &CFType) -> Option<RawSource> {
    // SAFETY: statics of the Carbon framework
    let (id_key, name_key, type_key, layout_type, data_key, languages_key) = unsafe {
        (
            kTISPropertyInputSourceID,
            kTISPropertyLocalizedName,
            kTISPropertyInputSourceType,
            kTISTypeKeyboardLayout,
            kTISPropertyUnicodeKeyLayoutData,
            kTISPropertyInputSourceLanguages,
        )
    };
    let id = string_property(source, id_key)?;
    let localized_name = string_property(source, name_key).unwrap_or_else(|| id.clone());
    let is_keyboard_layout =
        string_property(source, type_key).is_some_and(|ty| ty == layout_type.to_string());
    // SAFETY: valid source and key; the result is a CFData or null
    let has_layout_data = unsafe {
        TISGetInputSourceProperty(source, data_key)
            .cast::<CFData>()
            .as_ref()
    }
    .is_some_and(|data| !data.is_empty());
    // SAFETY: valid source and key; the result is a CFArray of CFString or null
    let language = unsafe {
        TISGetInputSourceProperty(source, languages_key)
            .cast::<CFArray<CFString>>()
            .as_ref()
    }
    .and_then(|languages| languages.get(0))
    .map(|language| language.to_string());
    Some(RawSource {
        id,
        localized_name,
        language,
        is_keyboard_layout,
        has_layout_data,
    })
}

fn enabled_sources() -> Vec<RawSource> {
    let Some(list) = input_source_list() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|source| raw_source(&source))
        .collect()
}

// MARK: - Layout data

/// The `uchr` data of an enabled layout, copied out of the input source so it can be used on any thread.
pub fn layout_uchr(id: &str) -> Option<Vec<u8>> {
    let list = input_source_list()?;
    for source in list.iter() {
        if string_property(&source, unsafe { kTISPropertyInputSourceID }).as_deref() != Some(id) {
            continue;
        }
        // SAFETY: valid source and key; the result is a CFData or null
        let data = unsafe {
            TISGetInputSourceProperty(&source, kTISPropertyUnicodeKeyLayoutData)
                .cast::<CFData>()
                .as_ref()
        }?;
        let bytes = data.to_vec();
        return (!bytes.is_empty()).then_some(bytes);
    }
    None
}

/// The character a key types in a layout, with or without Shift. `None` for no character, several
/// characters or a failure. Dead keys give nothing (state is not carried between presses).
pub fn translate_key(uchr: &[u8], key_code: u16, shift: bool, keyboard_type: u32) -> Option<char> {
    const KEY_DOWN: u16 = 0;
    // The Carbon modifier mask (shiftKey >> 8) UCKeyTranslate expects
    const SHIFT_KEY: u32 = 2;
    // kUCKeyTranslateNoDeadKeysMask
    const NO_DEAD_KEYS: u32 = 1;
    let mut dead_key_state = 0u32;
    let mut length = 0usize;
    let mut buffer = [0u16; 8];
    // SAFETY: `uchr` is layout data that stays alive for the call; the buffers are sized as declared
    let status = unsafe {
        UCKeyTranslate(
            uchr.as_ptr(),
            key_code,
            KEY_DOWN,
            if shift { SHIFT_KEY } else { 0 },
            keyboard_type,
            NO_DEAD_KEYS,
            &mut dead_key_state,
            buffer.len(),
            &mut length,
            buffer.as_mut_ptr(),
        )
    };
    if status != 0 || length != 1 {
        return None;
    }
    char::from_u32(u32::from(buffer[0]))
}

/// The keyboard type of the attached hardware and its shape (the grave key differs on ISO boards).
pub fn keyboard() -> (u32, crate::keymap::BoardShape) {
    use crate::keymap::BoardShape;
    use objc2_core_graphics::CGEventSource;
    let keyboard_type = CGEventSource::keyboard_type(None);
    // SAFETY: a plain lookup
    let shape = match unsafe { KBGetLayoutType(keyboard_type) } {
        ISO_LAYOUT_TYPE => BoardShape::Iso,
        _ => BoardShape::Ansi,
    };
    (keyboard_type, shape)
}

/// `kKeyboardISO` ('ISO ').
const ISO_LAYOUT_TYPE: u32 = 0x4953_4F20;

/// Switches the active keyboard layout to the one with this input source ID. Returns false if the layout
/// is not enabled or the system refused. `TISSelectInputSource` is fine off the main thread (stage 0).
pub fn current_layout_id() -> Option<String> {
    let source = unsafe { TISCopyCurrentKeyboardInputSource() }?;
    let retained = unsafe { CFRetained::from_raw(source) };
    string_property(&retained, unsafe { kTISPropertyInputSourceID })
}

pub fn switch_to_layout_direct(target_id: &str) -> bool {
    let Some(list) = input_source_list() else {
        return false;
    };
    for source in list.iter() {
        if string_property(&source, unsafe { kTISPropertyInputSourceID }).as_deref()
            == Some(target_id)
        {
            // SAFETY: a source taken from the list
            return unsafe { TISSelectInputSource(&source) } == 0;
        }
    }
    false
}

/// Switches the active keyboard layout to the one with this input source ID. If called from a
/// background thread (such as the conversion worker), dispatches asynchronously to the main queue
/// because Carbon's TISCreateInputSourceList asserts that it must run on the main queue.
pub fn switch_to_layout(target_id: &str) -> bool {
    if MainThreadMarker::new().is_some() {
        switch_to_layout_direct(target_id)
    } else {
        let id = target_id.to_owned();
        DispatchQueue::main().exec_async(move || {
            let _ = switch_to_layout_direct(&id);
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, language: &str, keyboard: bool, data: bool) -> RawSource {
        RawSource {
            id: id.to_owned(),
            localized_name: id.rsplit('.').next().unwrap_or(id).to_owned(),
            language: Some(language.to_owned()),
            is_keyboard_layout: keyboard,
            has_layout_data: data,
        }
    }

    fn ids(layouts: &[KeyboardLayout]) -> Vec<&str> {
        layouts.iter().map(|layout| layout.id.as_str()).collect()
    }

    #[test]
    fn only_real_keyboard_layouts_with_data_are_selected_in_system_order() {
        let sources = [
            source("com.apple.CharacterPaletteIM", "en", false, false),
            source("com.apple.keylayout.US", "en", true, true),
            source("com.apple.PressAndHold", "en", false, false),
            source("com.apple.keylayout.RussianWin", "ru", true, true),
            source("com.apple.inputmethod.ironwood", "en", false, false),
        ];
        let layouts = select_layouts(&sources);
        assert_eq!(
            ids(&layouts),
            ["com.apple.keylayout.US", "com.apple.keylayout.RussianWin"]
        );
        assert_eq!(layouts[1].language_code, "ru");
        assert_eq!(layouts[1].localized_name, "RussianWin");
    }

    #[test]
    fn a_layout_without_data_cannot_get_a_map_and_is_skipped() {
        let sources = [
            source("com.apple.keylayout.US", "en", true, true),
            source("com.apple.keylayout.Odd", "xx", true, false),
        ];
        assert_eq!(ids(&select_layouts(&sources)), ["com.apple.keylayout.US"]);
    }

    #[test]
    fn non_standard_layouts_with_data_are_kept() {
        // Russian-Phonetic and Dvorak have layout data; their maps come from the system
        let sources = [
            source("com.apple.keylayout.Russian-Phonetic", "ru", true, true),
            source("com.apple.keylayout.Dvorak", "en", true, true),
        ];
        assert_eq!(select_layouts(&sources).len(), 2);
    }

    #[test]
    fn nothing_usable_falls_back_to_us_and_russian() {
        assert_eq!(select_layouts(&[]), default_layouts());
        let only_input_methods = [source("com.apple.inputmethod.SCIM", "zh", false, false)];
        assert_eq!(select_layouts(&only_input_methods), default_layouts());
    }

    #[test]
    fn a_missing_language_becomes_unknown() {
        let mut layout = source("com.apple.keylayout.US", "en", true, true);
        layout.language = None;
        assert_eq!(select_layouts(&[layout])[0].language_code, "unknown");
    }

    #[test]
    fn the_defaults_are_us_then_russian() {
        let defaults = default_layouts();
        assert_eq!(
            ids(&defaults),
            ["com.apple.keylayout.US", "com.apple.keylayout.Russian"]
        );
    }

    // Read-only queries of this machine: they must not crash and must give a usable list.
    #[test]
    fn the_system_reports_at_least_one_layout() {
        let layouts = installed_layouts();
        assert!(!layouts.is_empty());
        assert!(layouts.iter().all(|layout| !layout.id.is_empty()));
    }

    #[test]
    fn switching_to_an_unknown_layout_fails_without_side_effects() {
        assert!(!switch_to_layout_direct(
            "com.apple.keylayout.DefinitelyNotInstalled"
        ));
    }
}
