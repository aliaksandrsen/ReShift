//! MARK: - Tap supervisor
//!
//! Keeps the event tap alive: creates it once the permission is there, re-enables or rebuilds it when
//! the system switched it off, and drops it when the permission is taken away. Pure decisions over an
//! `Observation`; the AppKit shell (`app.rs`) takes the readings and carries the actions out.

use std::time::Duration;

/// Check interval while a healthy tap is running: the tap re-arms itself in its own callback when the
/// system times it out, so this only covers the silent cases.
pub const HEALTHY_INTERVAL: Duration = Duration::from_secs(10);
/// Check interval while waiting for the permission or for a failed tap to come back.
pub const WAITING_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapHealth {
    Healthy,
    /// The system switched the tap off (timeout, user input protection).
    Disabled,
    /// The mach port is gone.
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    /// Accessibility permission, read right now.
    pub trusted: bool,
    /// `None` = there is no tap.
    pub tap: Option<TapHealth>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Create,
    Reenable,
    /// Tear the tap down and build a new one.
    Recreate,
    /// Tear the tap down and wait for the permission.
    Destroy,
}

#[derive(Debug, Default)]
pub struct Supervisor {
    /// Ticks in a row that found the tap disabled: one re-enable is tried, then the tap is rebuilt.
    disabled_streak: u32,
    /// A tap could not be created and nothing has changed since: logged once, not every two seconds.
    create_failed: bool,
    /// The user's session came back (wake, unlock): the next tick rebuilds the tap.
    rebuild_requested: bool,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// The Mac woke up or the session became active again: rebuild the tap on the next tick, whatever it
    /// looks like. A tap that survived the sleep is fine, but one that did not rarely says so.
    pub fn request_rebuild(&mut self) {
        self.rebuild_requested = true;
    }

    /// What to do now.
    pub fn decide(&mut self, observation: Observation) -> Action {
        let action = match (observation.trusted, observation.tap) {
            (false, None) => Action::Nothing,
            (false, Some(_)) => Action::Destroy,
            (true, None) => Action::Create,
            (true, Some(TapHealth::Dead)) => Action::Recreate,
            (true, Some(TapHealth::Disabled)) => {
                if self.disabled_streak == 0 {
                    Action::Reenable
                } else {
                    Action::Recreate
                }
            }
            (true, Some(TapHealth::Healthy)) => {
                if self.rebuild_requested {
                    Action::Recreate
                } else {
                    Action::Nothing
                }
            }
        };
        self.disabled_streak = match (observation.tap, action) {
            (Some(TapHealth::Disabled), Action::Reenable) => self.disabled_streak + 1,
            (Some(TapHealth::Disabled), _) => self.disabled_streak,
            _ => 0,
        };
        if matches!(action, Action::Recreate | Action::Destroy | Action::Create) {
            self.rebuild_requested = false;
        }
        if action == Action::Nothing && observation.tap.is_some() {
            self.create_failed = false;
        }
        action
    }

    /// The result of a `Create` or `Recreate`. Returns true if it is worth logging the failure.
    pub fn creation_result(&mut self, created: bool) -> bool {
        if created {
            self.create_failed = false;
            false
        } else {
            let first_failure = !self.create_failed;
            self.create_failed = true;
            first_failure
        }
    }

    /// When to look again.
    pub fn next_interval(&self, tap_running: bool) -> Duration {
        if tap_running && !self.rebuild_requested && self.disabled_streak == 0 {
            HEALTHY_INTERVAL
        } else {
            WAITING_INTERVAL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(trusted: bool, tap: Option<TapHealth>) -> Observation {
        Observation { trusted, tap }
    }

    #[test]
    fn without_permission_and_without_a_tap_there_is_nothing_to_do() {
        assert_eq!(Supervisor::new().decide(seen(false, None)), Action::Nothing);
    }

    #[test]
    fn the_tap_is_created_as_soon_as_the_permission_arrives() {
        let mut supervisor = Supervisor::new();
        assert_eq!(supervisor.decide(seen(false, None)), Action::Nothing);
        assert_eq!(supervisor.decide(seen(true, None)), Action::Create);
    }

    #[test]
    fn a_healthy_tap_is_left_alone() {
        let mut supervisor = Supervisor::new();
        for _ in 0..3 {
            assert_eq!(
                supervisor.decide(seen(true, Some(TapHealth::Healthy))),
                Action::Nothing
            );
        }
    }

    #[test]
    fn a_revoked_permission_drops_the_tap_whatever_its_state() {
        for health in [TapHealth::Healthy, TapHealth::Disabled, TapHealth::Dead] {
            assert_eq!(
                Supervisor::new().decide(seen(false, Some(health))),
                Action::Destroy,
                "{health:?}"
            );
        }
    }

    #[test]
    fn a_dead_tap_is_rebuilt_at_once() {
        assert_eq!(
            Supervisor::new().decide(seen(true, Some(TapHealth::Dead))),
            Action::Recreate
        );
    }

    #[test]
    fn a_disabled_tap_is_re_enabled_once_then_rebuilt() {
        let mut supervisor = Supervisor::new();
        let disabled = seen(true, Some(TapHealth::Disabled));
        assert_eq!(supervisor.decide(disabled), Action::Reenable);
        // Still disabled on the next tick: re-enabling did not help
        assert_eq!(supervisor.decide(disabled), Action::Recreate);
    }

    #[test]
    fn a_tap_that_comes_back_after_re_enabling_resets_the_count() {
        let mut supervisor = Supervisor::new();
        let disabled = seen(true, Some(TapHealth::Disabled));
        assert_eq!(supervisor.decide(disabled), Action::Reenable);
        assert_eq!(
            supervisor.decide(seen(true, Some(TapHealth::Healthy))),
            Action::Nothing
        );
        // A later timeout starts with a re-enable again, not with a rebuild
        assert_eq!(supervisor.decide(disabled), Action::Reenable);
    }

    #[test]
    fn a_requested_rebuild_replaces_a_healthy_tap_once() {
        let mut supervisor = Supervisor::new();
        let healthy = seen(true, Some(TapHealth::Healthy));
        supervisor.request_rebuild();
        assert_eq!(supervisor.decide(healthy), Action::Recreate);
        assert_eq!(supervisor.decide(healthy), Action::Nothing);
    }

    #[test]
    fn a_requested_rebuild_is_kept_until_the_permission_is_there() {
        let mut supervisor = Supervisor::new();
        supervisor.request_rebuild();
        // No permission yet: nothing can be rebuilt, and the request must not be lost
        assert_eq!(supervisor.decide(seen(false, None)), Action::Nothing);
        assert_eq!(supervisor.decide(seen(true, None)), Action::Create);
        // Creating covers the request: the next healthy tick does not rebuild again
        assert_eq!(
            supervisor.decide(seen(true, Some(TapHealth::Healthy))),
            Action::Nothing
        );
    }

    #[test]
    fn a_failed_creation_is_retried_but_logged_once() {
        let mut supervisor = Supervisor::new();
        assert_eq!(supervisor.decide(seen(true, None)), Action::Create);
        assert!(
            supervisor.creation_result(false),
            "the first failure is logged"
        );
        assert_eq!(supervisor.decide(seen(true, None)), Action::Create);
        assert!(!supervisor.creation_result(false), "the second is not");
        assert_eq!(supervisor.decide(seen(true, None)), Action::Create);
        assert!(!supervisor.creation_result(true));
        // After a success a new failure is news again
        assert!(supervisor.creation_result(false));
    }

    #[test]
    fn the_check_interval_is_long_only_while_all_is_well() {
        let mut supervisor = Supervisor::new();
        assert_eq!(supervisor.next_interval(true), HEALTHY_INTERVAL);
        assert_eq!(supervisor.next_interval(false), WAITING_INTERVAL);
        supervisor.request_rebuild();
        assert_eq!(supervisor.next_interval(true), WAITING_INTERVAL);
        supervisor.decide(seen(true, Some(TapHealth::Healthy)));
        assert_eq!(supervisor.next_interval(true), HEALTHY_INTERVAL);
        // A disabled tap is checked again soon
        supervisor.decide(seen(true, Some(TapHealth::Disabled)));
        assert_eq!(supervisor.next_interval(true), WAITING_INTERVAL);
    }

    #[test]
    fn the_intervals_are_sane() {
        assert!(WAITING_INTERVAL < HEALTHY_INTERVAL);
        assert!(WAITING_INTERVAL >= Duration::from_secs(1));
    }
}
