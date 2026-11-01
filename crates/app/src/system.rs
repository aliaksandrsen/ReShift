//! MARK: - The real system
//!
//! `System` for the pipeline: the general pasteboard, synthetic keys, the layout switch, the sound and
//! the notification. Thin glue; every decision lives in `conversion`, which is tested on a fake.

use std::time::Duration;

use crate::conversion::{Copied, System};
use crate::input::{Chord, post_chord};
use crate::log::debug;
use crate::pasteboard::{Pasteboard, Snapshot};
use crate::{layouts, permissions, sound};

pub struct MacSystem {
    pasteboard: Pasteboard,
}

impl MacSystem {
    pub fn new() -> Self {
        Self {
            pasteboard: Pasteboard::general(),
        }
    }
}

impl System for MacSystem {
    type Snapshot = Snapshot;

    fn can_post(&self) -> bool {
        permissions::accessibility_trusted()
    }

    fn change_count(&self) -> i64 {
        self.pasteboard.change_count()
    }

    fn snapshot(&mut self) -> Snapshot {
        self.pasteboard.snapshot()
    }

    fn restore(&mut self, snapshot: &Snapshot) {
        self.pasteboard.restore(snapshot);
    }

    fn read_copied(&mut self) -> Option<Copied> {
        self.pasteboard.read_copied()
    }

    fn write_text(&mut self, text: &str) -> i64 {
        self.pasteboard.write_text(text)
    }

    fn press(&mut self, chord: Chord) {
        if let Err(error) = post_chord(chord) {
            debug!("ERROR: could not post a key: {error:?}");
        }
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn switch_layout(&mut self, layout_id: &str) {
        if layouts::switch_to_layout(layout_id) {
            debug!("switchToLayout: switched");
        } else {
            debug!("switchToLayout: could not switch to an enabled layout");
        }
    }

    fn play_sound(&mut self) {
        sound::play_sound();
    }
}
