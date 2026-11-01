//! MARK: - App
//!
//! The AppKit shell: the application delegate, the status item with its menu, the About window, the
//! event tap's life cycle and the connection to the conversion worker. Everything here runs on the main thread. The
//! decisions behind it live in the tested modules (`supervisor`, `trigger`, `menu`, `about`, `conversion`,
//! `keymap`); this file carries them out.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem, NSTextInputContextKeyboardSelectionDidChangeNotification,
    NSWorkspace, NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidWakeNotification,
    NSWorkspaceSessionDidBecomeActiveNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSNotificationSuspensionBehavior, NSObject, NSObjectNSDelayedPerforming, NSObjectProtocol,
    NSString, NSTimer,
};
use reshift_core::l10n::{self, fill, t};

use crate::conversion::ConversionSettings;
use crate::guard::guarded;
use crate::keymap;
use crate::log::debug;
use crate::menu::{self, Action, Entry};
use crate::permissions;
use crate::settings::Settings;
use crate::settings_window::SettingsWindowController;
use crate::supervisor::{self, Observation, Supervisor};
use crate::system::MacSystem;
use crate::tap::{EventTap, Handler};
use crate::trigger::{HotkeyConfig, Trigger, needs_new_tap};
use crate::worker::{Shared, Worker};

pub struct Ivars {
    settings: RefCell<Settings>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    tap: RefCell<Option<EventTap>>,
    supervisor: RefCell<Supervisor>,
    /// What the tap calls on a trigger: hands the latest settings and maps to the conversion thread.
    handler: Handler,
    /// The latest settings and maps for the handler; the main thread writes, the tap thread reads.
    shared: Arc<Shared>,
    /// Held so the conversion thread lives as long as the app.
    _worker: Arc<Worker>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    /// The permission prompt is shown once per run if not granted.
    asked_for_permission: Cell<bool>,
    open_settings_on_start: Cell<bool>,
    open_about_on_start: Cell<bool>,
    settings_controller: OnceCell<Retained<SettingsWindowController>>,
    about_controller: OnceCell<Retained<crate::about::AboutWindowController>>,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `AppDelegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct AppDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for AppDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSApplicationDelegate for AppDelegate {
        // SAFETY: the signature is correct
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            guarded("applicationDidFinishLaunching", || self.setup());
        }

        // SAFETY: the signature is correct
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            guarded("applicationWillTerminate", || self.shut_down());
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(
            &self,
            _sender: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            guarded("applicationShouldHandleReopen", || {
                self.show_settings();
            });
            true
        }
    }

    impl AppDelegate {
        // SAFETY: the signature is correct
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            guarded("openSettings", || self.show_settings());
        }

        // SAFETY: the signature is correct
        #[unsafe(method(openAbout:))]
        fn open_about(&self, _sender: Option<&AnyObject>) {
            guarded("openAbout", || {
                let controller = self
                    .ivars()
                    .about_controller
                    .get_or_init(|| crate::about::AboutWindowController::new(self.mtm()));
                controller.show();
            });
        }

        // SAFETY: the signature is correct
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            guarded("quit", || {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            });
        }

        // SAFETY: the signature is correct
        #[unsafe(method(superviseTap:))]
        fn supervise_tap(&self, _timer: Option<&AnyObject>) {
            guarded("superviseTap", || self.supervise());
        }

        // SAFETY: the signature is correct
        #[unsafe(method(sessionResumed:))]
        fn session_resumed(&self, _notification: &NSNotification) {
            guarded("sessionResumed", || {
                debug!("wake or session change: the event tap will be rebuilt");
                self.ivars().supervisor.borrow_mut().request_rebuild();
                self.supervise();
                self.update_status_item_icon();
                unsafe {
                    self.performSelector_withObject_afterDelay(sel!(refreshLayoutIcon:), None, 0.05);
                }
            });
        }

        #[unsafe(method(layoutChanged:))]
        fn layout_changed(&self, _notification: &NSNotification) {
            guarded("layoutChanged", || {
                self.update_status_item_icon();
                unsafe {
                    self.performSelector_withObject_afterDelay(sel!(refreshLayoutIcon:), None, 0.05);
                }
            });
        }

        #[unsafe(method(refreshLayoutIcon:))]
        fn refresh_layout_icon(&self, _sender: Option<&AnyObject>) {
            guarded("refreshLayoutIcon", || {
                self.update_status_item_icon();
            });
        }
    }
);

impl AppDelegate {
    pub fn new(
        mtm: MainThreadMarker,
        settings: Settings,
        worker: Arc<Worker>,
        shared: Arc<Shared>,
    ) -> Retained<Self> {
        let handler: Handler = {
            let worker = Arc::clone(&worker);
            let shared = Arc::clone(&shared);
            Arc::new(move |trigger: Trigger| {
                debug!("Trigger: {trigger:?}");
                match shared.get() {
                    Some(request) => {
                        worker.request(request);
                    }
                    None => debug!("trigger ignored: no settings yet"),
                }
            })
        };
        let this = Self::alloc(mtm).set_ivars(Ivars {
            settings: RefCell::new(settings),
            status_item: OnceCell::new(),
            tap: RefCell::new(None),
            supervisor: RefCell::new(Supervisor::new()),
            handler,
            shared,
            _worker: worker,
            timer: RefCell::new(None),
            asked_for_permission: Cell::new(false),
            open_settings_on_start: Cell::new(false),
            open_about_on_start: Cell::new(false),
            settings_controller: OnceCell::new(),
            about_controller: OnceCell::new(),
        });
        // SAFETY: the signature of `NSObject`'s `init` method is correct
        unsafe { msg_send![super(this), init] }
    }

    fn language(&self) -> String {
        self.ivars()
            .settings
            .borrow()
            .app_language
            .clone()
            .unwrap_or_else(|| l10n::current_language().to_owned())
    }

    fn show_settings(&self) {
        let mtm = self.mtm();
        let controller = self.ivars().settings_controller.get_or_init(|| {
            let this_ref = self.retain();
            let on_changed = {
                let this = this_ref.clone();
                Rc::new(move |s| {
                    this.hotkey_settings_changed(s);
                })
            };
            let on_lang = {
                let this = this_ref.clone();
                Rc::new(move || {
                    this.refresh_menu_and_tooltip();
                    if let Some(about) = this.ivars().about_controller.get() {
                        about.refresh_if_visible();
                    }
                })
            };
            let settings = self.ivars().settings.borrow().clone();
            SettingsWindowController::new(mtm, settings, on_changed, on_lang)
        });
        controller.show();
    }

    fn hotkey_config(&self) -> HotkeyConfig {
        HotkeyConfig::from_settings(&self.ivars().settings.borrow())
    }

    // MARK: - Setup

    fn setup(&self) {
        debug!("App launched. PID: {}", std::process::id());
        self.refresh_conversion();
        self.setup_status_item();
        self.observe_session();

        let trusted = permissions::accessibility_trusted();
        debug!(
            "Initial permissions: accessibility={trusted}, input monitoring={}",
            permissions::input_monitoring_granted()
        );
        if !trusted {
            // Ask once at launch; the supervisor keeps checking afterwards, so
            // the shortcut starts working as soon as the user grants the permission
            self.ask_for_permission();
        }
        self.supervise();
        if self.ivars().open_settings_on_start.get() {
            self.show_settings();
        }
        if self.ivars().open_about_on_start.get() {
            let controller = self
                .ivars()
                .about_controller
                .get_or_init(|| crate::about::AboutWindowController::new(self.mtm()));
            controller.show();
        }
    }

    /// Builds the character maps for the enabled layouts and hands them, with the settings, to the slot the
    /// tap's handler reads. The maps come from the system (`UCKeyTranslate`); a layout the system gives no
    /// data for keeps its built-in table.
    fn refresh_conversion(&self) {
        let settings = self.ivars().settings.borrow();
        let started = std::time::Instant::now();
        let maps = keymap::system_maps(&settings.enabled_layouts);
        debug!(
            "Character maps for {} layouts built in {:?}",
            settings.enabled_layouts.len(),
            started.elapsed()
        );
        self.ivars()
            .shared
            .set(ConversionSettings::from(&*settings), Arc::new(maps));
    }

    fn shut_down(&self) {
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
        // Dropping the tap stops its thread
        self.ivars().tap.borrow_mut().take();
    }

    fn ask_for_permission(&self) {
        if self.ivars().asked_for_permission.replace(true) {
            return;
        }
        let granted = permissions::request_accessibility();
        debug!("Accessibility prompt shown, granted right now: {granted}");
    }

    fn observe_session(&self) {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        unsafe {
            for name in [
                NSWorkspaceDidWakeNotification,
                NSWorkspaceSessionDidBecomeActiveNotification,
            ] {
                center.addObserver_selector_name_object(
                    self.as_ref(),
                    sel!(sessionResumed:),
                    Some(name),
                    None,
                );
            }
            center.addObserver_selector_name_object(
                self.as_ref(),
                sel!(layoutChanged:),
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
            );

            // Keyboard layout switch notification from Carbon TIS (DeliverImmediately)
            let dist_center = NSDistributedNotificationCenter::defaultCenter();
            let tis_notif =
                NSString::from_str("com.apple.Carbon.TISNotifySelectedKeyboardInputSourceChanged");
            dist_center.addObserver_selector_name_object_suspensionBehavior(
                self.as_ref(),
                sel!(layoutChanged:),
                Some(&tis_notif),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );

            // Local AppKit text input context change notification
            let local_center = NSNotificationCenter::defaultCenter();
            local_center.addObserver_selector_name_object(
                self.as_ref(),
                sel!(layoutChanged:),
                Some(NSTextInputContextKeyboardSelectionDidChangeNotification),
                None,
            );
        }
    }

    // MARK: - Event tap

    /// Looks at the tap and the permission and carries out what the supervisor decides, then schedules
    /// the next look.
    fn supervise(&self) {
        let ivars = self.ivars();
        let config = self.hotkey_config();
        let trusted = permissions::accessibility_trusted();
        let tap_state = ivars.tap.borrow().as_ref().map(EventTap::health);
        let observation = Observation {
            trusted,
            tap: tap_state,
        };
        let action = ivars.supervisor.borrow_mut().decide(observation);
        self.update_status_item_icon();
        match action {
            supervisor::Action::Nothing => {}
            supervisor::Action::Reenable => {
                if let Some(tap) = ivars.tap.borrow().as_ref() {
                    tap.reenable();
                }
                debug!("event tap was off, re-enabled by the supervisor");
            }
            supervisor::Action::Destroy => {
                ivars.tap.borrow_mut().take();
                debug!("Accessibility permission is gone: the event tap was dropped");
            }
            supervisor::Action::Create | supervisor::Action::Recreate => {
                // The old tap goes first: two taps would deliver every trigger twice
                ivars.tap.borrow_mut().take();
                self.create_tap(config);
            }
        }
        let running = ivars.tap.borrow().is_some();
        let interval = ivars.supervisor.borrow().next_interval(running);
        self.schedule(interval);
    }

    fn create_tap(&self, config: HotkeyConfig) {
        let ivars = self.ivars();
        match EventTap::start(config, Arc::clone(&ivars.handler)) {
            Ok(tap) => {
                debug!("Event tap started: {:?}", tap.mode());
                ivars.supervisor.borrow_mut().creation_result(true);
                *ivars.tap.borrow_mut() = Some(tap);
            }
            Err(error) => {
                if ivars.supervisor.borrow_mut().creation_result(false) {
                    debug!("Event tap could not start: {error:?}");
                }
            }
        }
    }

    /// The settings window calls this after the hotkey settings changed (stage 5).
    pub fn hotkey_settings_changed(&self, settings: Settings) {
        let ivars = self.ivars();
        *ivars.settings.borrow_mut() = settings;
        self.refresh_conversion();
        let config = self.hotkey_config();
        let needs_new = ivars
            .tap
            .borrow()
            .as_ref()
            .is_some_and(|tap| needs_new_tap(tap.mode(), config));
        if needs_new {
            ivars.tap.borrow_mut().take();
            self.create_tap(config);
        } else if let Some(tap) = ivars.tap.borrow().as_ref() {
            tap.set_config(config);
        }
        self.refresh_menu_and_tooltip();
        self.update_status_item_icon();
    }

    fn schedule(&self, interval: std::time::Duration) {
        let ivars = self.ivars();
        if let Some(old) = ivars.timer.borrow_mut().take() {
            old.invalidate();
        }
        // SAFETY: the selector belongs to this class; the delegate outlives the timer
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval.as_secs_f64(),
                self.as_ref(),
                sel!(superviseTap:),
                None,
                false,
            )
        };
        // Nobody notices a second of slack, and the system can batch the wake-ups
        timer.setTolerance(interval.as_secs_f64() * 0.2);
        *ivars.timer.borrow_mut() = Some(timer);
    }

    // MARK: - Status item

    fn setup_status_item(&self) {
        let mtm = self.mtm();
        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = status_item.button(mtm) {
            let hotkey = self
                .ivars()
                .settings
                .borrow()
                .hotkey_description(&self.language());
            let tooltip = fill(t("menu.tooltip"), &hotkey);
            button.setToolTip(Some(&NSString::from_str(&tooltip)));
        }
        let menu = self.build_menu();
        status_item.setMenu(Some(&menu));
        log_status_item(&status_item, &menu, mtm);
        let _ = self.ivars().status_item.set(status_item);
        self.update_status_item_icon();
    }

    fn update_status_item_icon(&self) {
        let mtm = self.mtm();
        let Some(status_item) = self.ivars().status_item.get() else {
            return;
        };
        let Some(button) = status_item.button(mtm) else {
            return;
        };
        let settings = self.ivars().settings.borrow();
        let current_id = crate::layouts::current_layout_id().unwrap_or_default();
        let lang = settings
            .enabled_layouts
            .iter()
            .find(|l| l.id.eq_ignore_ascii_case(&current_id))
            .map(|l| l.language_code.as_str())
            .unwrap_or("unknown");
        let text = crate::indicator::indicator_text(settings.indicator_style, &current_id, lang);
        button.setImage(None);
        button.setTitle(&NSString::from_str(&text));
        debug!("Status item icon updated: layout='{current_id}' lang='{lang}' text='{text}'");
    }

    fn refresh_menu_and_tooltip(&self) {
        let mtm = self.mtm();
        if let Some(status_item) = self.ivars().status_item.get() {
            let menu = self.build_menu();
            status_item.setMenu(Some(&menu));
            if let Some(button) = status_item.button(mtm) {
                let hotkey = self
                    .ivars()
                    .settings
                    .borrow()
                    .hotkey_description(&self.language());
                let tooltip = fill(t("menu.tooltip"), &hotkey);
                button.setToolTip(Some(&NSString::from_str(&tooltip)));
            }
        }
    }

    fn build_menu(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        for entry in menu::ENTRIES {
            match entry {
                Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Entry::Item {
                    title_key,
                    action,
                    key,
                } => {
                    // SAFETY: the selector belongs to a method of this class
                    let item = unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            mtm.alloc::<NSMenuItem>(),
                            &NSString::from_str(t(title_key)),
                            Some(selector(action)),
                            &NSString::from_str(key),
                        )
                    };
                    // SAFETY: the delegate lives as long as the app
                    unsafe { item.setTarget(Some(self.as_ref())) };
                    menu.addItem(&item);
                }
            }
        }
        menu
    }
}

/// The menu bar of this Mac may be hidden or full of other icons, so the log records what the system
/// really created: where the icon's window is and what the menu holds (titles only, no user text).
fn log_status_item(status_item: &NSStatusItem, menu: &NSMenu, mtm: MainThreadMarker) {
    if crate::log::logger().is_none() {
        return;
    }
    let window = status_item.button(mtm).and_then(|button| button.window());
    match window {
        Some(window) => {
            let frame = window.frame();
            debug!(
                "Status item ready: icon window x={:.0} y={:.0} w={:.0} h={:.0}, visible={}",
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
                window.isVisible()
            );
        }
        None => debug!("Status item ready: the icon has no window"),
    }
    let titles: Vec<String> = menu
        .itemArray()
        .iter()
        .map(|item| {
            if item.isSeparatorItem() {
                "---".to_owned()
            } else {
                item.title().to_string()
            }
        })
        .collect();
    debug!("Menu: {}", titles.join(" | "));
}

fn selector(action: Action) -> Sel {
    match action {
        Action::OpenSettings => sel!(openSettings:),
        Action::OpenAbout => sel!(openAbout:),
        Action::Quit => sel!(quit:),
    }
}

/// Starts the app: a menu bar agent without a Dock icon. Does not return until the app quits.
pub fn run(mtm: MainThreadMarker, settings: Settings, open_settings: bool, open_about: bool) {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let worker = match Worker::start(MacSystem::new) {
        Ok(worker) => Arc::new(worker),
        Err(error) => {
            debug!("ERROR: cannot start the conversion thread: {error}");
            return;
        }
    };
    let shared = Arc::new(Shared::new());
    let delegate = AppDelegate::new(mtm, settings, worker, shared);
    delegate.ivars().open_settings_on_start.set(open_settings);
    delegate.ivars().open_about_on_start.set(open_about);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
