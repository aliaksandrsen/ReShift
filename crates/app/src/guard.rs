//! MARK: - Panic guard
//!
//! A panic must never cross an FFI boundary or kill the app (AGENTS.md). Every callback body that
//! the system or AppKit calls runs inside `guarded`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::log::debug;

/// Runs `f`; if it panics, logs the place and returns `None` instead of unwinding.
pub fn guarded<T>(place: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(_) => {
            debug!("ERROR: panic in {place}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_call_returns_its_value() {
        assert_eq!(guarded("test", || 7), Some(7));
    }

    #[test]
    fn a_panic_is_contained() {
        let result = guarded("test", || -> i32 { panic!("boom") });
        assert_eq!(result, None);
    }

    #[test]
    fn the_guard_can_be_used_again_after_a_panic() {
        assert_eq!(guarded("a", || -> () { panic!("first") }), None);
        assert_eq!(guarded("b", || "still works"), Some("still works"));
    }

    #[test]
    fn captured_state_is_still_usable_after_a_contained_panic() {
        let mut counter = 0;
        let _ = guarded("count", || {
            counter += 1;
            panic!("after the increment");
        });
        assert_eq!(counter, 1);
    }
}
