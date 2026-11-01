//! ReShift for macOS: a menu bar app that converts text typed in the wrong keyboard layout.

mod about;
mod app;
mod cli;
mod conversion;
mod defaults;
mod guard;
mod hotkey_format;
mod indicator;
mod input;
mod instance;
mod keymap;
mod layouts;
mod log;
mod login_item;
mod menu;
mod pasteboard;
mod permissions;
mod settings;
mod settings_window;
mod sound;
mod startup;
mod supervisor;
mod system;
mod tap;
mod trigger;
mod worker;

use std::process::ExitCode;

use objc2::MainThreadMarker;
use objc2_foundation::NSLocale;
use reshift_core::l10n;

use crate::defaults::UserDefaultsStore;
use crate::instance::{InstanceLock, LockError};
use crate::log::debug;

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("ReShift: {message}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = log::init(options.log_file.as_deref()) {
        eprintln!("ReShift: cannot open the log file: {error}");
        return ExitCode::from(2);
    }

    // Held until the process ends; a second copy would convert every text twice
    let _lock = match InstanceLock::acquire(&instance::lock_path(startup::LOCK_NAME)) {
        Ok(lock) => Some(lock),
        Err(LockError::AlreadyRunning) => {
            debug!("Already running - quitting");
            return ExitCode::SUCCESS;
        }
        Err(LockError::Io(error)) => {
            // Better to run unprotected than not at all
            debug!("ERROR: cannot take the instance lock: {error}");
            None
        }
    };

    let Some(mtm) = MainThreadMarker::new() else {
        debug!("ERROR: not on the main thread");
        return ExitCode::FAILURE;
    };

    let store = UserDefaultsStore::standard();
    let preferred: Vec<String> = NSLocale::preferredLanguages()
        .iter()
        .map(|language| language.to_string())
        .collect();
    let prepared = startup::prepare(
        &store,
        layouts::installed_layouts,
        preferred.iter().map(String::as_str),
        login_item::state(),
    );
    if prepared.needs_save {
        prepared.settings.save(&store);
    }
    if let Some(language) = &prepared.settings.app_language {
        l10n::set_current_language(language);
    }
    debug!(
        "Settings loaded: {} layouts, language {}",
        prepared.settings.enabled_layouts.len(),
        l10n::current_language()
    );

    app::run(
        mtm,
        prepared.settings,
        options.open_settings,
        options.open_about,
    );
    ExitCode::SUCCESS
}
