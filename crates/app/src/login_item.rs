//! MARK: - Launch at login
//!
//! `SMAppService.mainApp` (macOS 13+).
//!
//! The system is the source of truth: the user can switch the item off in System Settings > General >
//! Login Items, so at startup the setting is read from the system (`effective_setting`) and the system is
//! only changed when the user toggles the checkbox (`apply`). Re-registering at every launch would undo
//! the user's choice.

use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// What the system currently knows about the login item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginItemState {
    NotRegistered,
    Enabled,
    /// Registered, but the user has to approve it in System Settings > General > Login Items.
    RequiresApproval,
    /// The app is not in a form the system can register (for example a bare binary during development).
    NotFound,
}

impl LoginItemState {
    fn from_status(status: SMAppServiceStatus) -> Self {
        match status {
            SMAppServiceStatus::Enabled => Self::Enabled,
            SMAppServiceStatus::RequiresApproval => Self::RequiresApproval,
            SMAppServiceStatus::NotFound => Self::NotFound,
            _ => Self::NotRegistered,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginItemAction {
    Nothing,
    Register,
    Unregister,
}

/// What to do to make the system match the setting. A login item waiting for approval counts as
/// registered: registering it again would only nag the user.
pub fn action_for(wanted: bool, state: LoginItemState) -> LoginItemAction {
    match (wanted, state) {
        (true, LoginItemState::NotRegistered) => LoginItemAction::Register,
        (false, LoginItemState::Enabled | LoginItemState::RequiresApproval) => {
            LoginItemAction::Unregister
        }
        _ => LoginItemAction::Nothing,
    }
}

/// The value of the "Launch at Login" checkbox given the stored setting and the system state.
/// A bare binary (no bundle) cannot be registered, so the stored value stays.
pub fn effective_setting(stored: bool, state: LoginItemState) -> bool {
    match state {
        LoginItemState::Enabled | LoginItemState::RequiresApproval => true,
        LoginItemState::NotRegistered => false,
        LoginItemState::NotFound => stored,
    }
}

pub fn state() -> LoginItemState {
    // SAFETY: plain query of the main app service
    let status = unsafe { SMAppService::mainAppService().status() };
    LoginItemState::from_status(status)
}

/// Registers or unregisters the app. `Err` carries the system's message.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    // SAFETY: register/unregister of the main app service; the error is only read
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|error| error.localizedDescription().to_string())
}

/// Brings the system in line with the setting. Returns what was done, for the log.
pub fn apply(wanted: bool) -> (LoginItemAction, Result<(), String>) {
    let action = action_for(wanted, state());
    let result = match action {
        LoginItemAction::Nothing => Ok(()),
        LoginItemAction::Register => set_enabled(true),
        LoginItemAction::Unregister => set_enabled(false),
    };
    (action, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use LoginItemAction::*;
    use LoginItemState::*;

    #[test]
    fn wanted_and_missing_registers() {
        assert_eq!(action_for(true, NotRegistered), Register);
    }

    #[test]
    fn wanted_and_present_does_nothing() {
        assert_eq!(action_for(true, Enabled), Nothing);
        // Re-registering would show the approval prompt again
        assert_eq!(action_for(true, RequiresApproval), Nothing);
    }

    #[test]
    fn not_wanted_and_present_unregisters() {
        assert_eq!(action_for(false, Enabled), Unregister);
        assert_eq!(action_for(false, RequiresApproval), Unregister);
    }

    #[test]
    fn not_wanted_and_absent_does_nothing() {
        assert_eq!(action_for(false, NotRegistered), Nothing);
        assert_eq!(action_for(false, NotFound), Nothing);
    }

    #[test]
    fn an_unregisterable_app_is_left_alone() {
        // A bare binary cannot be registered; asking would only produce an error on every launch
        assert_eq!(action_for(true, NotFound), Nothing);
    }

    #[test]
    fn the_system_decides_the_checkbox_when_it_can() {
        // The user switched the item off in System Settings: the stored "true" is stale
        assert!(!effective_setting(true, NotRegistered));
        // ... or on: the stored "false" is stale
        assert!(effective_setting(false, Enabled));
        assert!(effective_setting(false, RequiresApproval));
    }

    #[test]
    fn a_bare_binary_keeps_the_stored_value() {
        assert!(effective_setting(true, NotFound));
        assert!(!effective_setting(false, NotFound));
    }

    #[test]
    fn statuses_map_to_states() {
        assert_eq!(
            LoginItemState::from_status(SMAppServiceStatus::Enabled),
            Enabled
        );
        assert_eq!(
            LoginItemState::from_status(SMAppServiceStatus::NotRegistered),
            NotRegistered
        );
        assert_eq!(
            LoginItemState::from_status(SMAppServiceStatus::RequiresApproval),
            RequiresApproval
        );
        assert_eq!(
            LoginItemState::from_status(SMAppServiceStatus::NotFound),
            NotFound
        );
        // A status added by a future macOS is treated as "not registered"
        assert_eq!(
            LoginItemState::from_status(SMAppServiceStatus(99)),
            NotRegistered
        );
    }

    // Read-only: asks the system, changes nothing. A bare test binary has no app bundle.
    #[test]
    fn the_system_query_does_not_crash() {
        let _ = state();
    }
}
