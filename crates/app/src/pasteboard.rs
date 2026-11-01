//! MARK: - Pasteboard
//!
//! The general pasteboard for the conversion: a full snapshot (every type of every item,
//! preserving images and files), a restore, and a text write that clipboard
//! managers are asked to ignore. Works off the main thread.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::conversion::Copied;

/// Marks content that should not be kept (nspasteboard.org convention, honored by clipboard managers).
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";
/// Marks content that may be sensitive: managers do not show or record it.
const CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";
/// VS Code's flavor of a copy; its JSON says whether the copy had a selection.
const VS_CODE_EDITOR_DATA: &str = "com.microsoft.VSCode.editor-data";
const CHROMIUM_CUSTOM_DATA: &str = "org.chromium.web-custom-data";

/// Every type of every item, in order.
pub struct Snapshot(Vec<Vec<(Retained<NSString>, Retained<NSData>)>>);

impl Snapshot {
    #[cfg(test)]
    pub fn item_count(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub fn type_count(&self) -> usize {
        self.0.iter().map(Vec::len).sum()
    }
}

pub struct Pasteboard {
    board: Retained<NSPasteboard>,
}

impl Pasteboard {
    pub fn general() -> Self {
        Self {
            board: NSPasteboard::generalPasteboard(),
        }
    }

    /// A private pasteboard for tests: the user's clipboard is never touched.
    #[cfg(test)]
    pub fn unique() -> Self {
        Self {
            board: NSPasteboard::pasteboardWithUniqueName(),
        }
    }

    pub fn change_count(&self) -> i64 {
        self.board.changeCount() as i64
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut items = Vec::new();
        if let Some(pasteboard_items) = self.board.pasteboardItems() {
            for item in pasteboard_items.iter() {
                let mut entries = Vec::new();
                if let Some(types) = item.types().into() {
                    let types: Retained<NSArray<NSString>> = types;
                    for data_type in types.iter() {
                        if let Some(data) = item.dataForType(&data_type) {
                            entries.push((data_type.clone(), data));
                        }
                    }
                }
                items.push(entries);
            }
        }
        Snapshot(items)
    }

    pub fn restore(&self, snapshot: &Snapshot) {
        self.board.clearContents();
        let items: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = snapshot
            .0
            .iter()
            .filter(|entries| !entries.is_empty())
            .map(|entries| {
                let item = NSPasteboardItem::new();
                for (data_type, data) in entries {
                    item.setData_forType(data, data_type);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        if !items.is_empty() {
            self.board
                .writeObjects(&NSArray::from_retained_slice(&items));
        }
    }

    /// The plain text on the clipboard and whether it is a whole-line copy of an editor.
    pub fn read_copied(&self) -> Option<Copied> {
        // SAFETY: the type constant is the documented NSString
        let text = unsafe { self.board.stringForType(NSPasteboardTypeString) }?.to_string();
        let editor_data = self
            .board
            .dataForType(&NSString::from_str(VS_CODE_EDITOR_DATA))
            .or_else(|| {
                self.board
                    .dataForType(&NSString::from_str(CHROMIUM_CUSTOM_DATA))
            })
            .map(|data| data.to_vec());
        Some(Copied {
            text,
            whole_line: crate::conversion::is_vs_code_empty_selection_copy(editor_data.as_deref()),
        })
    }

    /// Replaces the clipboard with `text`, marked transient and concealed. Returns the change count right
    /// after the write.
    pub fn write_text(&self, text: &str) -> i64 {
        self.board.clearContents();
        let item = NSPasteboardItem::new();
        // SAFETY: the type constant is the documented NSString
        item.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
        let empty = NSData::with_bytes(&[]);
        for marker in [TRANSIENT_TYPE, CONCEALED_TYPE] {
            item.setData_forType(&empty, &NSString::from_str(marker));
        }
        let items = [ProtocolObject::from_retained(item)];
        self.board
            .writeObjects(&NSArray::from_retained_slice(&items));
        self.change_count()
    }

    /// The types on the pasteboard right now (tests and the log).
    #[cfg(test)]
    pub fn type_names(&self) -> Vec<String> {
        self.board
            .types()
            .map(|types| types.iter().map(|t| t.to_string()).collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn set_custom(&self, text: &str, custom_type: &str, bytes: &[u8]) {
        self.board.clearContents();
        let item = NSPasteboardItem::new();
        // SAFETY: the type constant is the documented NSString
        item.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
        item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(custom_type));
        let items = [ProtocolObject::from_retained(item)];
        self.board
            .writeObjects(&NSArray::from_retained_slice(&items));
    }

    #[cfg(test)]
    pub fn custom_bytes(&self, custom_type: &str) -> Option<Vec<u8>> {
        self.board
            .dataForType(&NSString::from_str(custom_type))
            .map(|data| data.to_vec())
    }

    #[cfg(test)]
    pub fn plain_text(&self) -> Option<String> {
        // SAFETY: the type constant is the documented NSString
        unsafe { self.board.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Run on a private pasteboard: the user's clipboard is never read or written.

    use super::*;

    const CUSTOM: &str = "com.reshift.test.blob";

    #[test]
    fn a_snapshot_keeps_every_type_and_restores_it_byte_for_byte() {
        let board = Pasteboard::unique();
        board.set_custom("hello", CUSTOM, &[1, 2, 3, 0, 255]);
        let snapshot = board.snapshot();
        assert_eq!(snapshot.item_count(), 1);
        assert_eq!(snapshot.type_count(), 2);

        board.write_text("converted");
        assert_eq!(board.plain_text().as_deref(), Some("converted"));
        assert_eq!(
            board.custom_bytes(CUSTOM),
            None,
            "the write replaced everything"
        );

        board.restore(&snapshot);
        assert_eq!(board.plain_text().as_deref(), Some("hello"));
        assert_eq!(board.custom_bytes(CUSTOM), Some(vec![1, 2, 3, 0, 255]));
    }

    #[test]
    fn restoring_an_empty_snapshot_empties_the_clipboard() {
        let board = Pasteboard::unique();
        let empty = board.snapshot();
        assert_eq!(empty.item_count(), 0);
        board.write_text("something");
        board.restore(&empty);
        assert_eq!(board.plain_text(), None);
        assert!(board.type_names().is_empty());
    }

    #[test]
    fn our_text_is_marked_transient_and_concealed() {
        let board = Pasteboard::unique();
        board.write_text("secret-ish");
        let names = board.type_names();
        assert!(names.contains(&TRANSIENT_TYPE.to_owned()), "{names:?}");
        assert!(names.contains(&CONCEALED_TYPE.to_owned()), "{names:?}");
        assert_eq!(board.plain_text().as_deref(), Some("secret-ish"));
    }

    #[test]
    fn a_restored_clipboard_does_not_carry_our_markers() {
        let board = Pasteboard::unique();
        board.set_custom("mine", CUSTOM, &[9]);
        let snapshot = board.snapshot();
        board.write_text("converted");
        board.restore(&snapshot);
        let names = board.type_names();
        assert!(!names.contains(&TRANSIENT_TYPE.to_owned()), "{names:?}");
        assert!(!names.contains(&CONCEALED_TYPE.to_owned()), "{names:?}");
    }

    #[test]
    fn every_write_and_restore_changes_the_change_count() {
        let board = Pasteboard::unique();
        let start = board.change_count();
        let after_write = board.write_text("a");
        assert!(after_write > start);
        assert_eq!(
            board.change_count(),
            after_write,
            "reading does not change it"
        );
        let snapshot = board.snapshot();
        assert_eq!(
            board.change_count(),
            after_write,
            "a snapshot does not either"
        );
        board.restore(&snapshot);
        assert!(board.change_count() > after_write);
    }

    #[test]
    fn text_with_unicode_and_newlines_round_trips() {
        let board = Pasteboard::unique();
        let text = "Привет, мир!\nстрока 2\t\u{1F44B}\u{1F3FD}\r\n";
        board.write_text(text);
        assert_eq!(board.read_copied().map(|c| c.text).as_deref(), Some(text));
    }

    #[test]
    fn a_whole_line_copy_is_recognized_from_the_vs_code_flavor() {
        let board = Pasteboard::unique();
        board.set_custom(
            "a whole line",
            VS_CODE_EDITOR_DATA,
            br#"{"version":1,"isFromEmptySelection":true,"mode":"plaintext"}"#,
        );
        assert!(board.read_copied().unwrap().whole_line);
        board.set_custom(
            "a selection",
            VS_CODE_EDITOR_DATA,
            br#"{"isFromEmptySelection":false}"#,
        );
        assert!(!board.read_copied().unwrap().whole_line);
        board.set_custom("plain", CUSTOM, &[1]);
        assert!(!board.read_copied().unwrap().whole_line);
    }

    #[test]
    fn an_empty_clipboard_has_nothing_to_copy() {
        let board = Pasteboard::unique();
        assert!(board.read_copied().is_none());
    }

    #[test]
    fn several_items_survive_a_snapshot() {
        let board = Pasteboard::unique();
        let first = NSPasteboardItem::new();
        let second = NSPasteboardItem::new();
        first.setData_forType(&NSData::with_bytes(&[1]), &NSString::from_str(CUSTOM));
        second.setData_forType(&NSData::with_bytes(&[2]), &NSString::from_str(CUSTOM));
        board.board.clearContents();
        board.board.writeObjects(&NSArray::from_retained_slice(&[
            ProtocolObject::from_retained(first),
            ProtocolObject::from_retained(second),
        ]));
        let snapshot = board.snapshot();
        assert_eq!(snapshot.item_count(), 2);
        board.write_text("x");
        board.restore(&snapshot);
        assert_eq!(board.snapshot().item_count(), 2);
    }
}
