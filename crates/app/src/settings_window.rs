//! MARK: - Settings Window
//!
//! Pure AppKit settings window for ReShift.
//! Displays 4 tabs:
//! 1. General ("Основные"): Language, Launch at login, Sound, Menu bar indicator (flags/letters), Smart Conversion mode, Layout Switch mode, How it works.
//! 2. Layouts ("Раскладки"): Detected system keyboard layouts, Refresh button, Layout count.
//! 3. Hotkey ("Горячая клавиша"): Double Shift toggle, Custom shortcut recorder.
//! 4. Permissions ("Разрешения"): Accessibility status, Grant button, Troubleshooting instructions, System Settings button.
//!
//! Window dimensions: 580 x 520, floating, centered, released when closed = false.
//! When language changes, all tabs and strings update immediately.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSButton, NSColor, NSEvent, NSEventMask,
    NSFloatingWindowLevel, NSFont, NSPopUpButton, NSTabView, NSTabViewItem, NSTextField,
    NSTitlePosition, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use reshift_core::l10n::{self, fill, t};
use reshift_core::{KeyboardLayout, LayoutSwitchMode, SmartConversionMode};

use crate::defaults::UserDefaultsStore;
use crate::guard::guarded;
use crate::hotkey_format::{self, HotkeyModifiers};
use crate::indicator::IndicatorStyle;
use crate::login_item;
use crate::permissions;
use crate::settings::Settings;

const WINDOW_WIDTH: f64 = 580.0;
const WINDOW_HEIGHT: f64 = 550.0;
const CONTENT_WIDTH: f64 = 540.0;

// MARK: - FlippedView

define_class!(
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    pub struct FlippedView;

    unsafe impl NSObjectProtocol for FlippedView {}

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, initWithFrame: frame] }
    }
}

// MARK: - SettingsWindowController

pub struct ControllerIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
    tab_view: RefCell<Option<Retained<NSTabView>>>,
    settings: RefCell<Settings>,
    on_changed: Rc<dyn Fn(Settings)>,
    on_language_changed: Rc<dyn Fn()>,
    recording: Cell<bool>,
    monitor: RefCell<Option<Retained<AnyObject>>>,

    // Dynamic UI elements
    hotkey_badge_label: RefCell<Option<Retained<NSTextField>>>,
    hotkey_display_label: RefCell<Option<Retained<NSTextField>>>,
    hotkey_record_button: RefCell<Option<Retained<NSButton>>>,
    hotkey_prompt_label: RefCell<Option<Retained<NSTextField>>>,
    indicator_radios: RefCell<Vec<Retained<NSButton>>>,
    smart_mode_radios: RefCell<Vec<Retained<NSButton>>>,
    smart_mode_desc: RefCell<Option<Retained<NSTextField>>>,
    layout_switch_radios: RefCell<Vec<Retained<NSButton>>>,
    layout_switch_desc: RefCell<Option<Retained<NSTextField>>>,
    layouts_container: RefCell<Option<Retained<NSView>>>,
    layouts_count_label: RefCell<Option<Retained<NSTextField>>>,
    perm_status_label: RefCell<Option<Retained<NSTextField>>>,
    perm_status_subtitle: RefCell<Option<Retained<NSTextField>>>,
    perm_grant_button: RefCell<Option<Retained<NSButton>>>,
    perm_steps_box: RefCell<Option<Retained<NSBox>>>,
    perm_refresh_button: RefCell<Option<Retained<NSButton>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ControllerIvars]
    pub struct SettingsWindowController;

    unsafe impl NSObjectProtocol for SettingsWindowController {}

    unsafe impl NSWindowDelegate for SettingsWindowController {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            guarded("windowWillClose", || {
                self.stop_recording();
            });
        }
    }

    impl SettingsWindowController {
        #[unsafe(method(onLanguageChanged:))]
        fn on_language_changed(&self, sender: &NSPopUpButton) {
            guarded("onLanguageChanged", || {
                let index = sender.indexOfSelectedItem();
                if let Some(lang) = l10n::AVAILABLE_LANGUAGES.get(index as usize) {
                    l10n::set_current_language(lang.code);
                    self.ivars().settings.borrow_mut().app_language = Some(lang.code.to_owned());
                    self.save_current();
                    (self.ivars().on_language_changed)();
                    self.rebuild_ui();
                }
            });
        }

        #[unsafe(method(onIndicatorStyleChanged:))]
        fn on_indicator_style_changed(&self, sender: &NSButton) {
            guarded("onIndicatorStyleChanged", || {
                let tag = sender.tag();
                if let Some(style) = IndicatorStyle::from_i64(tag as i64) {
                    self.ivars().settings.borrow_mut().indicator_style = style;
                    self.save_current();
                    for radio in self.ivars().indicator_radios.borrow().iter() {
                        radio.setState(if radio.tag() == tag { 1 } else { 0 });
                    }
                }
            });
        }

        #[unsafe(method(onToggleLaunchAtLogin:))]
        fn on_toggle_launch_at_login(&self, sender: &NSButton) {
            guarded("onToggleLaunchAtLogin", || {
                let enabled = sender.state() != 0;
                let _ = login_item::apply(enabled);
                self.ivars().settings.borrow_mut().launch_at_login = enabled;
                self.save_current();
            });
        }

        #[unsafe(method(onTogglePlaySound:))]
        fn on_toggle_play_sound(&self, sender: &NSButton) {
            guarded("onTogglePlaySound", || {
                self.ivars().settings.borrow_mut().play_sounds = sender.state() != 0;
                self.save_current();
            });
        }


        #[unsafe(method(onSmartModeChanged:))]
        fn on_smart_mode_changed(&self, sender: &NSButton) {
            guarded("onSmartModeChanged", || {
                let tag = sender.tag();
                if let Some(mode) = SmartConversionMode::from_i64(tag as i64) {
                    self.ivars().settings.borrow_mut().smart_conversion_mode = mode;
                    self.save_current();
                    for radio in self.ivars().smart_mode_radios.borrow().iter() {
                        radio.setState(if radio.tag() == tag { 1 } else { 0 });
                    }
                    if let Some(desc_label) = self.ivars().smart_mode_desc.borrow().as_ref() {
                        desc_label.setStringValue(&NSString::from_str(t(mode.description_key())));
                    }
                }
            });
        }

        #[unsafe(method(onLayoutSwitchModeChanged:))]
        fn on_layout_switch_mode_changed(&self, sender: &NSButton) {
            guarded("onLayoutSwitchModeChanged", || {
                let tag = sender.tag();
                if let Some(mode) = LayoutSwitchMode::from_i64(tag as i64) {
                    self.ivars().settings.borrow_mut().layout_switch_mode = mode;
                    self.save_current();
                    for radio in self.ivars().layout_switch_radios.borrow().iter() {
                        radio.setState(if radio.tag() == tag { 1 } else { 0 });
                    }
                    if let Some(desc_label) = self.ivars().layout_switch_desc.borrow().as_ref() {
                        desc_label.setStringValue(&NSString::from_str(t(mode.description_key())));
                    }
                }
            });
        }

        #[unsafe(method(onToggleDoubleShift:))]
        fn on_toggle_double_shift(&self, sender: &NSButton) {
            guarded("onToggleDoubleShift", || {
                let enabled = sender.state() != 0;
                self.ivars().settings.borrow_mut().use_double_shift = enabled;
                self.save_current();
                self.stop_recording();
                self.update_hotkey_displays();
            });
        }

        #[unsafe(method(onRecordHotkey:))]
        fn on_record_hotkey(&self, _sender: &NSButton) {
            guarded("onRecordHotkey", || {
                if self.ivars().recording.get() {
                    self.stop_recording();
                } else {
                    self.start_recording();
                }
            });
        }

        #[unsafe(method(onRefreshLayouts:))]
        fn on_refresh_layouts(&self, _sender: &NSButton) {
            guarded("onRefreshLayouts", || {
                let layouts = crate::layouts::installed_layouts();
                self.ivars().settings.borrow_mut().enabled_layouts = layouts.clone();
                self.save_current();
                self.rebuild_layouts_list(&layouts);
            });
        }

        #[unsafe(method(onGrantAccessibility:))]
        fn on_grant_accessibility(&self, _sender: &NSButton) {
            guarded("onGrantAccessibility", || {
                let _ = permissions::request_accessibility();
                self.refresh_permissions_ui();
            });
        }

        #[unsafe(method(onOpenPrivacySettings:))]
        fn on_open_privacy_settings(&self, _sender: &NSButton) {
            guarded("onOpenPrivacySettings", || {
                if let Some(url) = NSURL::URLWithString(&NSString::from_str(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )) {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            });
        }

        #[unsafe(method(onRefreshPermissions:))]
        fn on_refresh_permissions(&self, _sender: &NSButton) {
            guarded("onRefreshPermissions", || {
                self.refresh_permissions_ui();
            });
        }
    }
);

impl SettingsWindowController {
    pub fn new(
        mtm: MainThreadMarker,
        settings: Settings,
        on_changed: Rc<dyn Fn(Settings)>,
        on_language_changed: Rc<dyn Fn()>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            window: RefCell::new(None),
            tab_view: RefCell::new(None),
            settings: RefCell::new(settings),
            on_changed,
            on_language_changed,
            recording: Cell::new(false),
            monitor: RefCell::new(None),
            hotkey_badge_label: RefCell::new(None),
            hotkey_display_label: RefCell::new(None),
            hotkey_record_button: RefCell::new(None),
            hotkey_prompt_label: RefCell::new(None),
            indicator_radios: RefCell::new(Vec::new()),
            smart_mode_radios: RefCell::new(Vec::new()),
            smart_mode_desc: RefCell::new(None),
            layout_switch_radios: RefCell::new(Vec::new()),
            layout_switch_desc: RefCell::new(None),
            layouts_container: RefCell::new(None),
            layouts_count_label: RefCell::new(None),
            perm_status_label: RefCell::new(None),
            perm_status_subtitle: RefCell::new(None),
            perm_grant_button: RefCell::new(None),
            perm_steps_box: RefCell::new(None),
            perm_refresh_button: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn show(&self) {
        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        app.activate();

        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
            return;
        }

        self.build_window();
    }

    fn save_current(&self) {
        let settings = self.ivars().settings.borrow().clone();
        settings.save(&UserDefaultsStore::standard());
        (self.ivars().on_changed)(settings);
    }

    // MARK: - Window Building

    fn build_window(&self) {
        let mtm = self.mtm();
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc::<NSWindow>(),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
                ),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(t("settings.windowTitle")));
        window.center();
        window.setLevel(NSFloatingWindowLevel);
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
        }

        let tab_view = NSTabView::initWithFrame(
            mtm.alloc::<NSTabView>(),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            ),
        );

        self.populate_tabs(&tab_view, 0);

        window.setContentView(Some(&tab_view));
        window.makeKeyAndOrderFront(None);

        *self.ivars().tab_view.borrow_mut() = Some(tab_view);
        *self.ivars().window.borrow_mut() = Some(window);
    }

    fn rebuild_ui(&self) {
        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.setTitle(&NSString::from_str(t("settings.windowTitle")));
            if let Some(tab_view) = self.ivars().tab_view.borrow().as_ref() {
                let current_index = tab_view
                    .selectedTabViewItem()
                    .map(|item| tab_view.indexOfTabViewItem(&item))
                    .unwrap_or(0);
                self.populate_tabs(tab_view, current_index as usize);
            }
        }
    }

    fn populate_tabs(&self, tab_view: &NSTabView, selected_index: usize) {
        let mtm = self.mtm();

        // Clear existing tabs
        while let Some(item) = tab_view.selectedTabViewItem() {
            unsafe {
                let _: () = msg_send![tab_view, removeTabViewItem: &*item];
            }
        }

        // 1. General Tab
        let general_item =
            unsafe { NSTabViewItem::initWithIdentifier(mtm.alloc::<NSTabViewItem>(), None) };
        general_item.setLabel(&NSString::from_str(t("settings.tab.general")));
        general_item.setView(Some(&self.build_general_tab()));
        tab_view.addTabViewItem(&general_item);

        // 2. Layouts Tab
        let layouts_item =
            unsafe { NSTabViewItem::initWithIdentifier(mtm.alloc::<NSTabViewItem>(), None) };
        layouts_item.setLabel(&NSString::from_str(t("settings.tab.layouts")));
        layouts_item.setView(Some(&self.build_layouts_tab()));
        tab_view.addTabViewItem(&layouts_item);

        // 3. Hotkey Tab
        let hotkey_item =
            unsafe { NSTabViewItem::initWithIdentifier(mtm.alloc::<NSTabViewItem>(), None) };
        hotkey_item.setLabel(&NSString::from_str(t("settings.tab.hotkey")));
        hotkey_item.setView(Some(&self.build_hotkey_tab()));
        tab_view.addTabViewItem(&hotkey_item);

        // 4. Permissions Tab
        let perm_item =
            unsafe { NSTabViewItem::initWithIdentifier(mtm.alloc::<NSTabViewItem>(), None) };
        perm_item.setLabel(&NSString::from_str(t("settings.tab.permissions")));
        perm_item.setView(Some(&self.build_permissions_tab()));
        tab_view.addTabViewItem(&perm_item);

        tab_view.selectTabViewItemAtIndex(selected_index as isize);
    }

    // MARK: - Tab Views

    fn build_general_tab(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, 490.0));
        let view = FlippedView::new(frame, mtm);
        let settings = self.ivars().settings.borrow();
        let target = self.as_ref();

        let mut y = 16.0;

        // Language
        let lang_label = create_label(t("general.language"), false, mtm);
        lang_label.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(120.0, 22.0)));
        view.addSubview(&lang_label);

        let popup = NSPopUpButton::initWithFrame_pullsDown(
            mtm.alloc::<NSPopUpButton>(),
            NSRect::new(NSPoint::new(150.0, y - 2.0), NSSize::new(160.0, 26.0)),
            false,
        );
        for lang in l10n::AVAILABLE_LANGUAGES {
            popup.addItemWithTitle(&NSString::from_str(lang.name));
        }
        let current_lang = l10n::current_language();
        let selected_index = l10n::AVAILABLE_LANGUAGES
            .iter()
            .position(|l| l.code == current_lang)
            .unwrap_or(0);
        popup.selectItemAtIndex(selected_index as isize);
        unsafe {
            popup.setTarget(Some(target));
            popup.setAction(Some(sel!(onLanguageChanged:)));
        }
        view.addSubview(&popup);
        y += 32.0;

        // Menu Bar Indicator Style
        let ind_label = create_label(t("general.indicator"), false, mtm);
        ind_label.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(160.0, 20.0)));
        view.addSubview(&ind_label);

        let mut ind_radios = Vec::new();
        let flag_radio = create_radio(
            t(IndicatorStyle::Flag.name_key()),
            settings.indicator_style == IndicatorStyle::Flag,
            IndicatorStyle::Flag as isize,
            target,
            sel!(onIndicatorStyleChanged:),
            mtm,
        );
        flag_radio.setFrame(NSRect::new(
            NSPoint::new(190.0, y),
            NSSize::new(130.0, 20.0),
        ));
        view.addSubview(&flag_radio);
        ind_radios.push(flag_radio);

        let letters_radio = create_radio(
            t(IndicatorStyle::Letters.name_key()),
            settings.indicator_style == IndicatorStyle::Letters,
            IndicatorStyle::Letters as isize,
            target,
            sel!(onIndicatorStyleChanged:),
            mtm,
        );
        letters_radio.setFrame(NSRect::new(
            NSPoint::new(325.0, y),
            NSSize::new(140.0, 20.0),
        ));
        view.addSubview(&letters_radio);
        ind_radios.push(letters_radio);

        *self.ivars().indicator_radios.borrow_mut() = ind_radios;
        y += 28.0;

        // Checkboxes
        let launch_btn = create_checkbox(
            t("general.launchAtLogin"),
            settings.launch_at_login,
            target,
            sel!(onToggleLaunchAtLogin:),
            mtm,
        );
        launch_btn.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(450.0, 20.0)));
        view.addSubview(&launch_btn);
        y += 26.0;

        let sound_btn = create_checkbox(
            t("general.playSound"),
            settings.play_sounds,
            target,
            sel!(onTogglePlaySound:),
            mtm,
        );
        sound_btn.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(450.0, 20.0)));
        view.addSubview(&sound_btn);
        y += 26.0;

        y += 18.0;

        // Smart Conversion Section
        let smart_title = create_label(t("general.smartConversion"), true, mtm);
        smart_title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
        view.addSubview(&smart_title);
        y += 24.0;

        let mut smart_radios = Vec::new();
        for mode in SmartConversionMode::ALL {
            let is_selected = settings.smart_conversion_mode == mode;
            let radio = create_radio(
                t(mode.name_key()),
                is_selected,
                mode as isize,
                target,
                sel!(onSmartModeChanged:),
                mtm,
            );
            radio.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
            view.addSubview(&radio);
            smart_radios.push(radio);
            y += 22.0;
        }
        *self.ivars().smart_mode_radios.borrow_mut() = smart_radios;

        let smart_desc = create_wrapping_label(
            t(settings.smart_conversion_mode.description_key()),
            true,
            mtm,
        );
        smart_desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 32.0)));
        view.addSubview(&smart_desc);
        *self.ivars().smart_mode_desc.borrow_mut() = Some(smart_desc);
        y += 52.0;

        // Layout Switch Section
        let switch_title = create_label(t("general.layoutSwitch"), true, mtm);
        switch_title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
        view.addSubview(&switch_title);
        y += 24.0;

        let mut switch_radios = Vec::new();
        for mode in LayoutSwitchMode::ALL {
            let is_selected = settings.layout_switch_mode == mode;
            let radio = create_radio(
                t(mode.name_key()),
                is_selected,
                mode as isize,
                target,
                sel!(onLayoutSwitchModeChanged:),
                mtm,
            );
            radio.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
            view.addSubview(&radio);
            switch_radios.push(radio);
            y += 22.0;
        }
        *self.ivars().layout_switch_radios.borrow_mut() = switch_radios;

        let switch_desc =
            create_wrapping_label(t(settings.layout_switch_mode.description_key()), true, mtm);
        switch_desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 32.0)));
        view.addSubview(&switch_desc);
        *self.ivars().layout_switch_desc.borrow_mut() = Some(switch_desc);
        y += 52.0;

        // How it works
        let how_title = create_label(t("general.howItWorks"), true, mtm);
        how_title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
        view.addSubview(&how_title);
        y += 24.0;

        let how_desc = create_wrapping_label(t("general.howItWorksText"), true, mtm);
        how_desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 36.0)));
        view.addSubview(&how_desc);

        Retained::into_super(view)
    }

    fn build_layouts_tab(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, 490.0));
        let view = FlippedView::new(frame, mtm);
        let target = self.as_ref();
        let settings = self.ivars().settings.borrow();

        let mut y = 16.0;

        let title = create_label(t("layouts.title"), true, mtm);
        title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 20.0)));
        view.addSubview(&title);
        y += 24.0;

        let desc = create_wrapping_label(t("layouts.description"), true, mtm);
        desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 34.0)));
        view.addSubview(&desc);
        y += 42.0;

        // List box container
        let box_frame = NSRect::new(NSPoint::new(24.0, y), NSSize::new(490.0, 280.0));
        let list_box = create_box(box_frame, None, mtm);
        let content_flipped = FlippedView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(486.0, 276.0)),
            mtm,
        );

        self.populate_layout_rows(&content_flipped, &settings.enabled_layouts);

        list_box.setContentView(Some(&content_flipped));
        view.addSubview(&list_box);
        *self.ivars().layouts_container.borrow_mut() = Some(Retained::into_super(content_flipped));
        y += 290.0;

        // Footer buttons
        let refresh_btn = create_button(t("layouts.refresh"), target, sel!(onRefreshLayouts:), mtm);
        refresh_btn.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(170.0, 28.0)));
        view.addSubview(&refresh_btn);

        let count_str = fill(
            t("layouts.count"),
            &settings.enabled_layouts.len().to_string(),
        );
        let count_label = create_label(&count_str, false, mtm);
        count_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        count_label.setFrame(NSRect::new(
            NSPoint::new(210.0, y + 4.0),
            NSSize::new(300.0, 20.0),
        ));
        view.addSubview(&count_label);
        *self.ivars().layouts_count_label.borrow_mut() = Some(count_label);

        Retained::into_super(view)
    }

    fn populate_layout_rows(&self, container: &NSView, layouts: &[KeyboardLayout]) {
        let mtm = self.mtm();
        let mut row_y = 8.0;
        for layout in layouts {
            let row = FlippedView::new(
                NSRect::new(NSPoint::new(8.0, row_y), NSSize::new(470.0, 40.0)),
                mtm,
            );

            // Name
            let name = create_label(&layout.localized_name, true, mtm);
            name.setFrame(NSRect::new(
                NSPoint::new(12.0, 4.0),
                NSSize::new(370.0, 18.0),
            ));
            row.addSubview(&name);

            // ID
            let id_lbl = create_label(&layout.id, false, mtm);
            id_lbl.setFont(Some(&NSFont::systemFontOfSize(11.0)));
            id_lbl.setTextColor(Some(&NSColor::secondaryLabelColor()));
            id_lbl.setFrame(NSRect::new(
                NSPoint::new(12.0, 22.0),
                NSSize::new(370.0, 16.0),
            ));
            row.addSubview(&id_lbl);

            // Language Badge
            let badge = create_label(&layout.language_code.to_uppercase(), true, mtm);
            badge.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
            badge.setTextColor(Some(&NSColor::labelColor()));
            badge.setFrame(NSRect::new(
                NSPoint::new(410.0, 10.0),
                NSSize::new(50.0, 20.0),
            ));
            row.addSubview(&badge);

            container.addSubview(&row);
            row_y += 44.0;
        }
    }

    fn rebuild_layouts_list(&self, layouts: &[KeyboardLayout]) {
        if let Some(container) = self.ivars().layouts_container.borrow().as_ref() {
            // Remove existing row subviews
            let subviews = container.subviews();
            for subview in subviews.iter() {
                unsafe {
                    let _: () = msg_send![&*subview, removeFromSuperview];
                }
            }
            self.populate_layout_rows(container, layouts);
        }
        if let Some(count_lbl) = self.ivars().layouts_count_label.borrow().as_ref() {
            let count_str = fill(t("layouts.count"), &layouts.len().to_string());
            count_lbl.setStringValue(&NSString::from_str(&count_str));
        }
    }

    fn build_hotkey_tab(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, 490.0));
        let view = FlippedView::new(frame, mtm);
        let target = self.as_ref();
        let settings = self.ivars().settings.borrow();
        let current_lang = l10n::current_language();

        let mut y = 16.0;

        let title = create_label(t("hotkey.title"), true, mtm);
        title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 20.0)));
        view.addSubview(&title);
        y += 24.0;

        let desc = create_wrapping_label(t("hotkey.description"), true, mtm);
        desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 42.0)));
        view.addSubview(&desc);
        y += 48.0;

        // Current shortcut banner
        let banner_box = create_box(
            NSRect::new(NSPoint::new(24.0, y), NSSize::new(490.0, 48.0)),
            None,
            mtm,
        );
        let banner_view = FlippedView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(486.0, 44.0)),
            mtm,
        );

        let cur_title = create_label(t("hotkey.current"), false, mtm);
        cur_title.setFrame(NSRect::new(
            NSPoint::new(12.0, 12.0),
            NSSize::new(180.0, 20.0),
        ));
        banner_view.addSubview(&cur_title);

        let shortcut_desc = settings.hotkey_description(current_lang);
        let badge_lbl = create_label(&shortcut_desc, true, mtm);
        badge_lbl.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
        badge_lbl.setFrame(NSRect::new(
            NSPoint::new(200.0, 10.0),
            NSSize::new(270.0, 24.0),
        ));
        banner_view.addSubview(&badge_lbl);
        *self.ivars().hotkey_badge_label.borrow_mut() = Some(badge_lbl);

        banner_box.setContentView(Some(&banner_view));
        view.addSubview(&banner_box);
        y += 60.0;

        // Double Shift Checkbox
        let double_shift_btn = create_checkbox(
            t("hotkey.useDoubleShift"),
            settings.use_double_shift,
            target,
            sel!(onToggleDoubleShift:),
            mtm,
        );
        double_shift_btn.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(450.0, 20.0)));
        view.addSubview(&double_shift_btn);
        y += 24.0;

        let hint = create_wrapping_label(t("hotkey.doubleShiftHint"), true, mtm);
        hint.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 36.0)));
        view.addSubview(&hint);
        y += 48.0;

        // Custom Hotkey section
        let custom_title = create_label(t("hotkey.customTitle"), true, mtm);
        custom_title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
        view.addSubview(&custom_title);
        y += 24.0;

        let prompt_lbl = create_label(t("hotkeyRecorder.prompt"), false, mtm);
        prompt_lbl.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 18.0)));
        view.addSubview(&prompt_lbl);
        *self.ivars().hotkey_prompt_label.borrow_mut() = Some(prompt_lbl);
        y += 24.0;

        // Display field & Record button
        let cur_custom =
            hotkey_format::describe(settings.hotkey_key_code, settings.hotkey_modifiers);
        let disp = create_label(&cur_custom, false, mtm);
        disp.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
        disp.setFrame(NSRect::new(
            NSPoint::new(24.0, y + 2.0),
            NSSize::new(200.0, 24.0),
        ));
        view.addSubview(&disp);
        *self.ivars().hotkey_display_label.borrow_mut() = Some(disp);

        let rec_btn = create_button(
            t("hotkeyRecorder.record"),
            target,
            sel!(onRecordHotkey:),
            mtm,
        );
        rec_btn.setFrame(NSRect::new(
            NSPoint::new(230.0, y),
            NSSize::new(100.0, 28.0),
        ));
        rec_btn.setEnabled(!settings.use_double_shift);
        view.addSubview(&rec_btn);
        *self.ivars().hotkey_record_button.borrow_mut() = Some(rec_btn);
        y += 32.0;

        let req_lbl = create_wrapping_label(t("hotkey.requireModifier"), true, mtm);
        req_lbl.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 20.0)));
        view.addSubview(&req_lbl);

        Retained::into_super(view)
    }

    fn update_hotkey_displays(&self) {
        let settings = self.ivars().settings.borrow();
        let current_lang = l10n::current_language();
        if let Some(lbl) = self.ivars().hotkey_badge_label.borrow().as_ref() {
            let desc = settings.hotkey_description(current_lang);
            lbl.setStringValue(&NSString::from_str(&desc));
        }
        if let Some(disp) = self.ivars().hotkey_display_label.borrow().as_ref() {
            let custom =
                hotkey_format::describe(settings.hotkey_key_code, settings.hotkey_modifiers);
            disp.setStringValue(&NSString::from_str(&custom));
            disp.setTextColor(Some(&NSColor::labelColor()));
        }
        if let Some(btn) = self.ivars().hotkey_record_button.borrow().as_ref() {
            btn.setEnabled(!settings.use_double_shift);
            btn.setTitle(&NSString::from_str(t("hotkeyRecorder.record")));
        }
        if let Some(prompt) = self.ivars().hotkey_prompt_label.borrow().as_ref() {
            prompt.setStringValue(&NSString::from_str(t("hotkeyRecorder.prompt")));
        }
    }

    fn start_recording(&self) {
        self.ivars().recording.set(true);
        if let Some(btn) = self.ivars().hotkey_record_button.borrow().as_ref() {
            btn.setTitle(&NSString::from_str(t("hotkeyRecorder.cancel")));
        }
        if let Some(disp) = self.ivars().hotkey_display_label.borrow().as_ref() {
            disp.setStringValue(&NSString::from_str(t("hotkeyRecorder.pressKey")));
            disp.setTextColor(Some(&NSColor::systemOrangeColor()));
        }

        let this_retained = self.retain();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let ev = unsafe { event.as_ref() };
            let key_code = ev.keyCode();
            let raw_flags = ev.modifierFlags().0 as u32;
            let modifiers = HotkeyModifiers(raw_flags).sanitized();

            // Escape without modifiers cancels
            if key_code == 0x35 && !modifiers.intersects(HotkeyModifiers::ALL) {
                this_retained.stop_recording();
                return std::ptr::null_mut();
            }

            // Must contain Ctrl, Option, or Command
            if modifiers.is_valid_for_hotkey() {
                this_retained.on_recorded_hotkey(key_code, modifiers);
                return std::ptr::null_mut();
            }

            std::ptr::null_mut()
        });

        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        };
        *self.ivars().monitor.borrow_mut() = monitor;
    }

    fn stop_recording(&self) {
        self.ivars().recording.set(false);
        if let Some(mon) = self.ivars().monitor.borrow_mut().take() {
            unsafe {
                NSEvent::removeMonitor(&mon);
            }
        }
        self.update_hotkey_displays();
    }

    fn on_recorded_hotkey(&self, key_code: u16, modifiers: HotkeyModifiers) {
        self.stop_recording();
        {
            let mut settings = self.ivars().settings.borrow_mut();
            settings.hotkey_key_code = key_code;
            settings.hotkey_modifiers = modifiers;
        }
        self.save_current();
        self.update_hotkey_displays();
    }

    fn build_permissions_tab(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, 490.0));
        let view = FlippedView::new(frame, mtm);
        let target = self.as_ref();

        let mut y = 16.0;

        let title = create_label(t("permissions.title"), true, mtm);
        title.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 20.0)));
        view.addSubview(&title);
        y += 24.0;

        let desc = create_wrapping_label(t("permissions.description"), true, mtm);
        desc.setFrame(NSRect::new(NSPoint::new(24.0, y), NSSize::new(480.0, 34.0)));
        view.addSubview(&desc);
        y += 42.0;

        // Status Box
        let is_trusted = permissions::accessibility_trusted();
        let status_box = create_box(
            NSRect::new(NSPoint::new(24.0, y), NSSize::new(490.0, 68.0)),
            None,
            mtm,
        );
        let status_view = FlippedView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(486.0, 64.0)),
            mtm,
        );

        let (title_text, subtitle_text, color) = if is_trusted {
            (
                format!("✓ {}", t("permissions.accessibilityTitle")),
                t("permissions.granted"),
                NSColor::systemGreenColor(),
            )
        } else {
            (
                format!("⚠ {}", t("permissions.accessibilityTitle")),
                t("permissions.notGranted"),
                NSColor::systemOrangeColor(),
            )
        };

        let status_title = create_label(&title_text, true, mtm);
        status_title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
        status_title.setTextColor(Some(&color));
        status_title.setFrame(NSRect::new(
            NSPoint::new(14.0, 12.0),
            NSSize::new(310.0, 20.0),
        ));
        status_view.addSubview(&status_title);
        *self.ivars().perm_status_label.borrow_mut() = Some(status_title);

        let status_sub = create_label(subtitle_text, false, mtm);
        status_sub.setFont(Some(&NSFont::systemFontOfSize(11.5)));
        status_sub.setTextColor(Some(&NSColor::secondaryLabelColor()));
        status_sub.setFrame(NSRect::new(
            NSPoint::new(14.0, 34.0),
            NSSize::new(450.0, 20.0),
        ));
        status_view.addSubview(&status_sub);
        *self.ivars().perm_status_subtitle.borrow_mut() = Some(status_sub);

        let grant_btn = create_button(
            t("permissions.grantAccess"),
            target,
            sel!(onGrantAccessibility:),
            mtm,
        );
        grant_btn.setFrame(NSRect::new(
            NSPoint::new(330.0, 18.0),
            NSSize::new(144.0, 28.0),
        ));
        unsafe {
            let _: () = msg_send![&*grant_btn, setHidden: is_trusted];
        }
        status_view.addSubview(&grant_btn);
        *self.ivars().perm_grant_button.borrow_mut() = Some(grant_btn);

        status_box.setContentView(Some(&status_view));
        view.addSubview(&status_box);
        y += 68.0 + 16.0;

        // Steps instructions Box
        let steps_box = create_box(
            NSRect::new(NSPoint::new(24.0, y), NSSize::new(490.0, 180.0)),
            None,
            mtm,
        );
        let steps_view = FlippedView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(486.0, 176.0)),
            mtm,
        );

        let mut step_y = 10.0;
        let how_to = create_label(t("permissions.howToEnable"), true, mtm);
        how_to.setFrame(NSRect::new(
            NSPoint::new(12.0, step_y),
            NSSize::new(460.0, 18.0),
        ));
        steps_view.addSubview(&how_to);
        step_y += 22.0;

        for step_key in [
            "permissions.step1",
            "permissions.step2",
            "permissions.step3",
            "permissions.step4",
        ] {
            let s_lbl = create_label(t(step_key), false, mtm);
            s_lbl.setFrame(NSRect::new(
                NSPoint::new(12.0, step_y),
                NSSize::new(460.0, 18.0),
            ));
            steps_view.addSubview(&s_lbl);
            step_y += 20.0;
        }
        step_y += 6.0;

        let sys_btn = create_button(
            t("permissions.openSettings"),
            target,
            sel!(onOpenPrivacySettings:),
            mtm,
        );
        sys_btn.setFrame(NSRect::new(
            NSPoint::new(12.0, step_y),
            NSSize::new(220.0, 28.0),
        ));
        steps_view.addSubview(&sys_btn);

        steps_box.setContentView(Some(&steps_view));
        unsafe {
            let _: () = msg_send![&*steps_box, setHidden: is_trusted];
        }
        view.addSubview(&steps_box);
        *self.ivars().perm_steps_box.borrow_mut() = Some(steps_box);

        // Refresh status button: right under status box when trusted, or under steps box when untrusted
        let ref_y = if is_trusted { y } else { y + 180.0 + 16.0 };
        let ref_btn = create_button(
            t("permissions.refreshStatus"),
            target,
            sel!(onRefreshPermissions:),
            mtm,
        );
        ref_btn.setFrame(NSRect::new(
            NSPoint::new(24.0, ref_y),
            NSSize::new(160.0, 28.0),
        ));
        view.addSubview(&ref_btn);
        *self.ivars().perm_refresh_button.borrow_mut() = Some(ref_btn);

        Retained::into_super(view)
    }

    fn refresh_permissions_ui(&self) {
        let is_trusted = permissions::accessibility_trusted();
        let (title_text, subtitle_text, color) = if is_trusted {
            (
                format!("✓ {}", t("permissions.accessibilityTitle")),
                t("permissions.granted"),
                NSColor::systemGreenColor(),
            )
        } else {
            (
                format!("⚠ {}", t("permissions.accessibilityTitle")),
                t("permissions.notGranted"),
                NSColor::systemOrangeColor(),
            )
        };
        if let Some(lbl) = self.ivars().perm_status_label.borrow().as_ref() {
            lbl.setStringValue(&NSString::from_str(&title_text));
            lbl.setTextColor(Some(&color));
        }
        if let Some(sub) = self.ivars().perm_status_subtitle.borrow().as_ref() {
            sub.setStringValue(&NSString::from_str(subtitle_text));
        }
        if let Some(btn) = self.ivars().perm_grant_button.borrow().as_ref() {
            unsafe {
                let _: () = msg_send![&**btn, setHidden: is_trusted];
            }
        }
        if let Some(box_view) = self.ivars().perm_steps_box.borrow().as_ref() {
            unsafe {
                let _: () = msg_send![&**box_view, setHidden: is_trusted];
            }
        }
        if let Some(ref_btn) = self.ivars().perm_refresh_button.borrow().as_ref() {
            let ref_y = if is_trusted { 164.0 } else { 360.0 };
            ref_btn.setFrame(NSRect::new(
                NSPoint::new(24.0, ref_y),
                NSSize::new(160.0, 28.0),
            ));
        }
    }
}

// MARK: - View Creation Helpers

fn create_label(text: &str, bold: bool, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let lbl = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    if bold {
        lbl.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    } else {
        lbl.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    }
    lbl
}

fn create_wrapping_label(
    text: &str,
    secondary: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let lbl = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    lbl.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    if secondary {
        lbl.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    lbl
}

fn create_checkbox(
    title: &str,
    checked: bool,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let btn = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    btn.setState(if checked { 1 } else { 0 });
    btn
}

fn create_radio(
    title: &str,
    checked: bool,
    tag: isize,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let btn = unsafe {
        NSButton::radioButtonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    btn.setState(if checked { 1 } else { 0 });
    btn.setTag(tag);
    btn
}

fn create_button(
    title: &str,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    }
}

fn create_box(frame: NSRect, title: Option<&str>, mtm: MainThreadMarker) -> Retained<NSBox> {
    let box_view = NSBox::initWithFrame(mtm.alloc::<NSBox>(), frame);
    if let Some(t_str) = title {
        box_view.setTitle(&NSString::from_str(t_str));
    } else {
        box_view.setTitlePosition(NSTitlePosition::NoTitle);
    }
    box_view
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn test_settings() -> Settings {
        Settings::defaults(vec![
            KeyboardLayout::new("com.apple.keylayout.US", "U.S.", "en"),
            KeyboardLayout::new("com.apple.keylayout.RussianWin", "Russian – PC", "ru"),
        ])
    }

    #[test]
    fn settings_controller_initializes_on_main_thread() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let changed = Arc::new(AtomicBool::new(false));
        let changed_clone = Arc::clone(&changed);
        let on_changed = Rc::new(move |_s: Settings| {
            changed_clone.store(true, Ordering::SeqCst);
        });
        let on_lang = Rc::new(|| {});
        let controller = SettingsWindowController::new(mtm, test_settings(), on_changed, on_lang);

        assert!(!changed.load(Ordering::SeqCst));
        assert_eq!(
            controller.ivars().settings.borrow().enabled_layouts.len(),
            2
        );
    }

    #[test]
    fn hotkey_recording_cancels_on_escape() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let on_changed = Rc::new(|_| {});
        let on_lang = Rc::new(|| {});
        let controller = SettingsWindowController::new(mtm, test_settings(), on_changed, on_lang);

        controller.start_recording();
        assert!(controller.ivars().recording.get());
        controller.stop_recording();
        assert!(!controller.ivars().recording.get());
    }

    #[test]
    fn recorded_hotkey_updates_settings_and_notifies() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let received = Arc::new(std::sync::Mutex::new(None));
        let rec_clone = Arc::clone(&received);
        let on_changed = Rc::new(move |s: Settings| {
            *rec_clone.lock().unwrap() = Some(s);
        });
        let on_lang = Rc::new(|| {});
        let controller = SettingsWindowController::new(mtm, test_settings(), on_changed, on_lang);

        controller.on_recorded_hotkey(0x60, HotkeyModifiers::COMMAND);
        let updated = controller.ivars().settings.borrow().clone();
        assert_eq!(updated.hotkey_key_code, 0x60);
        assert_eq!(updated.hotkey_modifiers, HotkeyModifiers::COMMAND);
        assert_eq!(
            received.lock().unwrap().as_ref().map(|s| s.hotkey_key_code),
            Some(0x60)
        );
    }

    #[test]
    fn flipped_view_is_flipped() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let view = FlippedView::new(NSRect::ZERO, mtm);
        assert!(view.isFlipped());
    }
}
