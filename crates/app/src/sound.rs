//! MARK: - Conversion sound
//!
//! The Tink sound on conversion (disabled by default).

use dispatch2::DispatchQueue;
use objc2_app_kit::NSSound;
use objc2_foundation::NSString;

const SOUND_NAME: &str = "Tink";

pub fn play_sound() {
    crate::log::debug!("play_sound: playing {}", SOUND_NAME);
    DispatchQueue::main().exec_async(|| {
        if let Some(sound) = NSSound::soundNamed(&NSString::from_str(SOUND_NAME)) {
            sound.play();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sound_exists_on_this_system() {
        assert_eq!(SOUND_NAME, "Tink");
        assert!(std::path::Path::new("/System/Library/Sounds/Tink.aiff").exists());
    }
}
