//! MARK: - About Window
//!
//! Custom native AppKit About window for ReShift.
//! Displays the ReShift icon, app name, "Версия 0.1.0",
//! tagline, usage instructions, GitHub repository link, and MIT license with clean, tight margins.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSFloatingWindowLevel,
    NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImageScaling, NSImageView,
    NSTextAlignment, NSTextField, NSWindow, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    NSMutableAttributedString, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize,
    NSString, NSURL,
};
use reshift_core::l10n::{fill, t};

use crate::guard::guarded;
use crate::settings_window::FlippedView;

const WINDOW_WIDTH: f64 = 300.0;
const WINDOW_HEIGHT: f64 = 302.0;
const CONTENT_WIDTH: f64 = 268.0;
const CONTENT_X: f64 = (WINDOW_WIDTH - CONTENT_WIDTH) / 2.0;
const GITHUB_URL: &str = "https://github.com/aliaksandrsen/ReShift";

pub struct AboutIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AboutIvars]
    pub struct AboutWindowController;

    unsafe impl NSObjectProtocol for AboutWindowController {}

    impl AboutWindowController {
        #[unsafe(method(openGithub:))]
        fn open_github(&self, _sender: Option<&AnyObject>) {
            guarded("openGithub", || {
                let url = NSURL::URLWithString(&NSString::from_str(GITHUB_URL));
                if let Some(url) = url {
                    let _ = NSWorkspace::sharedWorkspace().openURL(&url);
                }
            });
        }
    }
);

impl AboutWindowController {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AboutIvars {
            window: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn show(&self) {
        guarded("showAboutWindow", || {
            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            app.activate();

            let window = if let Some(window) = self.ivars().window.borrow().as_ref() {
                window.clone()
            } else {
                let window = self.create_window(mtm);
                *self.ivars().window.borrow_mut() = Some(window.clone());
                window
            };

            window.setTitle(&NSString::from_str(t("about.windowTitle")));
            let view = self.build_content_view(mtm);
            window.setContentView(Some(&view));
            window.makeKeyAndOrderFront(None);
        });
    }

    pub fn refresh_if_visible(&self) {
        guarded("refreshAboutWindow", || {
            if let Some(window) = self.ivars().window.borrow().as_ref()
                && window.isVisible()
            {
                let mtm = self.mtm();
                window.setTitle(&NSString::from_str(t("about.windowTitle")));
                let view = self.build_content_view(mtm);
                window.setContentView(Some(&view));
            }
        });
    }

    fn create_window(&self, mtm: MainThreadMarker) -> Retained<NSWindow> {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc::<NSWindow>(),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
                ),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.center();
        window.setLevel(NSFloatingWindowLevel);
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window
    }

    fn build_content_view(&self, mtm: MainThreadMarker) -> Retained<FlippedView> {
        let content_frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        let view = FlippedView::new(content_frame, mtm);

        let mut y = 14.0;

        // 1. App Icon (64 x 64)
        let app = NSApplication::sharedApplication(mtm);
        if let Some(icon) = app.applicationIconImage() {
            let iv = NSImageView::imageViewWithImage(&icon, mtm);
            iv.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            let icon_x = (WINDOW_WIDTH - 64.0) / 2.0;
            iv.setFrame(NSRect::new(
                NSPoint::new(icon_x, y),
                NSSize::new(64.0, 64.0),
            ));
            view.addSubview(&iv);
        }
        y += 64.0 + 8.0;

        // 2. App Name
        let name_lbl = NSTextField::labelWithString(&NSString::from_str(t("about.appName")), mtm);
        name_lbl.setFont(Some(&NSFont::boldSystemFontOfSize(18.0)));
        name_lbl.setAlignment(NSTextAlignment::Center);
        name_lbl.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X, y),
            NSSize::new(CONTENT_WIDTH, 22.0),
        ));
        view.addSubview(&name_lbl);
        y += 22.0 + 2.0;

        // 3. Version (clean, no duplicate in parentheses)
        let version_str = fill(t("about.version"), "0.1.0");
        let ver_lbl = NSTextField::labelWithString(&NSString::from_str(&version_str), mtm);
        ver_lbl.setFont(Some(&NSFont::systemFontOfSize(11.5)));
        ver_lbl.setTextColor(Some(&NSColor::secondaryLabelColor()));
        ver_lbl.setAlignment(NSTextAlignment::Center);
        ver_lbl.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X, y),
            NSSize::new(CONTENT_WIDTH, 15.0),
        ));
        view.addSubview(&ver_lbl);
        y += 15.0 + 4.0;

        // 4. Tagline
        let tag_lbl =
            NSTextField::wrappingLabelWithString(&NSString::from_str(t("about.tagline")), mtm);
        tag_lbl.setFont(Some(&NSFont::systemFontOfSize(10.5)));
        tag_lbl.setTextColor(Some(&NSColor::secondaryLabelColor()));
        tag_lbl.setAlignment(NSTextAlignment::Center);
        tag_lbl.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X, y),
            NSSize::new(CONTENT_WIDTH, 28.0),
        ));
        view.addSubview(&tag_lbl);
        y += 28.0 + 8.0;

        // 5. Separator
        let sep1 = NSBox::initWithFrame(
            mtm.alloc::<NSBox>(),
            NSRect::new(NSPoint::new(CONTENT_X, y), NSSize::new(CONTENT_WIDTH, 1.0)),
        );
        sep1.setBoxType(NSBoxType::Separator);
        view.addSubview(&sep1);
        y += 1.0 + 7.0;

        // 6. How to use section
        let how_lbl = NSTextField::labelWithString(&NSString::from_str(t("about.howToUse")), mtm);
        how_lbl.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
        how_lbl.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X + 6.0, y),
            NSSize::new(CONTENT_WIDTH - 12.0, 15.0),
        ));
        view.addSubview(&how_lbl);
        y += 15.0 + 3.0;

        for step_key in ["about.step1", "about.step2", "about.step3"] {
            let step_lbl = NSTextField::labelWithString(&NSString::from_str(t(step_key)), mtm);
            step_lbl.setFont(Some(&NSFont::systemFontOfSize(10.0)));
            step_lbl.setTextColor(Some(&NSColor::labelColor()));
            step_lbl.setFrame(NSRect::new(
                NSPoint::new(CONTENT_X + 6.0, y),
                NSSize::new(CONTENT_WIDTH - 12.0, 15.0),
            ));
            view.addSubview(&step_lbl);
            y += 15.0 + 1.5;
        }
        y += 6.5;

        // 7. Separator
        let sep2 = NSBox::initWithFrame(
            mtm.alloc::<NSBox>(),
            NSRect::new(NSPoint::new(CONTENT_X, y), NSSize::new(CONTENT_WIDTH, 1.0)),
        );
        sep2.setBoxType(NSBoxType::Separator);
        view.addSubview(&sep2);
        y += 1.0 + 7.0;

        // 8. Footer: GitHub link button and License
        let full_text = "GitHub: aliaksandrsen/ReShift ↗";
        let prefix = "GitHub: ";
        let link_text = "aliaksandrsen/ReShift ↗";
        let github_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(full_text),
                Some(self.as_ref()),
                Some(sel!(openGithub:)),
                mtm,
            )
        };
        github_btn.setBordered(false);
        let attr_title = NSMutableAttributedString::initWithString(
            mtm.alloc::<NSMutableAttributedString>(),
            &NSString::from_str(full_text),
        );
        unsafe {
            attr_title.addAttribute_value_range(
                NSFontAttributeName,
                &NSFont::systemFontOfSize(10.5),
                NSRange::new(0, full_text.encode_utf16().count()),
            );
            attr_title.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &NSColor::secondaryLabelColor(),
                NSRange::new(0, prefix.encode_utf16().count()),
            );
            attr_title.addAttribute_value_range(
                NSForegroundColorAttributeName,
                &NSColor::linkColor(),
                NSRange::new(
                    prefix.encode_utf16().count(),
                    link_text.encode_utf16().count(),
                ),
            );
        }
        github_btn.setAttributedTitle(&attr_title);
        github_btn.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X, y),
            NSSize::new(CONTENT_WIDTH, 17.0),
        ));
        view.addSubview(&github_btn);
        y += 17.0 + 2.0;

        let lic_lbl = NSTextField::labelWithString(&NSString::from_str(t("about.license")), mtm);
        lic_lbl.setFont(Some(&NSFont::systemFontOfSize(10.0)));
        lic_lbl.setTextColor(Some(&NSColor::secondaryLabelColor()));
        lic_lbl.setAlignment(NSTextAlignment::Center);
        lic_lbl.setFrame(NSRect::new(
            NSPoint::new(CONTENT_X, y),
            NSSize::new(CONTENT_WIDTH, 14.0),
        ));
        view.addSubview(&lic_lbl);

        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_controller_initializes_on_main_thread() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let controller = AboutWindowController::new(mtm);
        assert!(controller.ivars().window.borrow().is_none());
    }

    #[test]
    fn version_string_has_no_duplicate_parentheses() {
        let en = fill(reshift_core::l10n::lookup("en", "about.version"), "0.1.0");
        let ru = fill(reshift_core::l10n::lookup("ru", "about.version"), "0.1.0");
        assert_eq!(en, "Version 0.1.0");
        assert_eq!(ru, "Версия 0.1.0");
        assert!(!en.contains('(') && !en.contains(')'));
        assert!(!ru.contains('(') && !ru.contains(')'));
    }
}
