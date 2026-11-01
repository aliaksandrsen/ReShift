//! MARK: - Permissions
//!
//! Accessibility is what an active event tap and posting key events need (`AXIsProcessTrusted`
//! and `AXIsProcessTrustedWithOptions` with the prompt).
//! Input Monitoring is checked for diagnostics and logging.

use std::ffi::c_void;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFString, kCFBooleanTrue};
use objc2_core_graphics::CGPreflightListenEventAccess;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

/// The value of `kAXTrustedCheckOptionPrompt`.
const PROMPT_OPTION: &str = "AXTrustedCheckOptionPrompt";

/// Accessibility permission, without showing anything.
pub fn accessibility_trusted() -> bool {
    // SAFETY: a plain query
    unsafe { AXIsProcessTrusted() }
}

/// Accessibility permission; shows the system prompt (with a button to System Settings) if it is missing.
pub fn request_accessibility() -> bool {
    let key = CFString::from_str(PROMPT_OPTION);
    let Some(yes) = (unsafe { kCFBooleanTrue }) else {
        return accessibility_trusted();
    };
    let options: objc2_core_foundation::CFRetained<CFDictionary<CFString, CFBoolean>> =
        CFDictionary::from_slices(&[&*key], &[yes]);
    // SAFETY: a valid options dictionary; the call only reads it
    unsafe {
        AXIsProcessTrustedWithOptions(
            (&*options as *const CFDictionary<CFString, CFBoolean>).cast(),
        )
    }
}

/// Input Monitoring permission, without showing anything.
pub fn input_monitoring_granted() -> bool {
    CGPreflightListenEventAccess()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_option_is_the_documented_key() {
        assert_eq!(PROMPT_OPTION, "AXTrustedCheckOptionPrompt");
    }

    // Read-only queries: they must not crash and must agree with each other
    #[test]
    fn the_permission_queries_work() {
        let first = accessibility_trusted();
        assert_eq!(first, accessibility_trusted());
        let _ = input_monitoring_granted();
    }
}
