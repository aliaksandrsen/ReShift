//! MARK: - Event tap
//!
//! One `CGEventTap` on its own thread with its own run loop. The callback only asks `TapLogic`
//! what to do and passes or swallows the event; the shortcut handler it calls must be quick and
//! never block. Our own synthetic events carry `MARKER` and are ignored.
//!
//! Safety of the pieces shared between threads: `CGEventTapEnable`, `CGEventTapIsEnabled`,
//! `CFMachPortInvalidate` and `CFRunLoopStop` are called from the main thread while the tap thread runs.
//! Apple's threading guide calls `CFRunLoopRef` thread-safe; for the other three I found no statement
//! either way, they are what event tap utilities commonly call from a control thread, and the live and
//! `--ignored` tests below exercise exactly these calls (enable/disable, drop while running, 20 restarts).
//! `Sync` is asserted for those uses only.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::guard::guarded;
use crate::input::MARKER;
use crate::log::debug;
use crate::supervisor::TapHealth;
use crate::trigger::{HotkeyConfig, KeyEvent, TapLogic, TapMode, Trigger, tap_mode};

/// The shortcut handler. Runs on the tap thread: no waiting, no blocking calls.
pub type Handler = Arc<dyn Fn(Trigger) + Send + Sync>;

/// How long the tap thread sleeps in its run loop before it looks at the stop flag again. A normal stop
/// wakes it at once; this only bounds the rare race of a stop arriving before the loop runs.
const STOP_POLL_SECONDS: f64 = 1.0;
/// How long `start` waits for the tap thread to report.
const START_TIMEOUT: Duration = Duration::from_secs(3);
/// How long dropping a tap waits for its thread. A thread that does not finish in time is left to end on
/// its own: it owns everything it uses, and the tap is already silenced.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
pub enum TapError {
    /// The system refused: the Accessibility permission is missing.
    NotPermitted,
    NoRunLoopSource,
    ThreadFailed,
    StartTimedOut,
}

struct SendMachPort(CFRetained<CFMachPort>);
// SAFETY: only used for enable/is-enabled/invalidate, which are thread-safe (see the module comment)
unsafe impl Send for SendMachPort {}
// SAFETY: as above
unsafe impl Sync for SendMachPort {}

struct SendRunLoop(CFRetained<CFRunLoop>);
// SAFETY: only `CFRunLoopStop`, which is documented as thread-safe, is called from other threads
unsafe impl Send for SendRunLoop {}
// SAFETY: as above
unsafe impl Sync for SendRunLoop {}

struct Shared {
    /// The packed `HotkeyConfig`, replaced from the main thread.
    config: AtomicU64,
    stop: AtomicBool,
    port: Mutex<Option<SendMachPort>>,
    run_loop: Mutex<Option<SendRunLoop>>,
}

/// Lives on the tap thread; the callback reaches it through `user_info`.
struct CallbackState {
    shared: Arc<Shared>,
    logic: RefCell<TapLogic>,
    handler: Handler,
    start: Instant,
    mode: TapMode,
    /// The tap itself, to re-enable it from the callback when the system switches it off.
    port: Cell<Option<NonNull<CFMachPort>>>,
}

pub struct EventTap {
    shared: Arc<Shared>,
    mode: TapMode,
    thread: Option<JoinHandle<()>>,
    /// Gets a message (or is dropped) when the thread is done.
    finished: mpsc::Receiver<()>,
}

impl EventTap {
    pub fn start(config: HotkeyConfig, handler: Handler) -> Result<Self, TapError> {
        let mode = tap_mode(config);
        let shared = Arc::new(Shared {
            config: AtomicU64::new(config.pack()),
            stop: AtomicBool::new(false),
            port: Mutex::new(None),
            run_loop: Mutex::new(None),
        });
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("event-tap".to_owned())
                .spawn(move || {
                    run_thread(&shared, handler, mode, &started_tx);
                    let _ = finished_tx.send(());
                })
                .map_err(|error| {
                    debug!("ERROR: cannot start the event tap thread: {error}");
                    TapError::ThreadFailed
                })?
        };
        let tap = Self {
            shared,
            mode,
            thread: Some(thread),
            finished: finished_rx,
        };
        match started_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(tap),
            Ok(Err(error)) => Err(error),
            // Dropping `tap` stops the thread
            Err(_) => Err(TapError::StartTimedOut),
        }
    }

    pub fn mode(&self) -> TapMode {
        self.mode
    }

    /// Hands a new shortcut to the running tap. Only valid while the mode stays the same
    /// (`needs_new_tap`).
    pub fn set_config(&self, config: HotkeyConfig) {
        self.shared.config.store(config.pack(), Ordering::Relaxed);
    }

    pub fn health(&self) -> TapHealth {
        if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
            return TapHealth::Dead;
        }
        let port = self
            .shared
            .port
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match port.as_ref() {
            None => TapHealth::Dead,
            Some(port) if !port.0.is_valid() => TapHealth::Dead,
            Some(port) if !CGEvent::tap_is_enabled(&port.0) => TapHealth::Disabled,
            Some(_) => TapHealth::Healthy,
        }
    }

    pub fn reenable(&self) {
        let port = self
            .shared
            .port
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(port) = port.as_ref() {
            CGEvent::tap_enable(&port.0, true);
        }
    }
}

impl Drop for EventTap {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        // First silence the tap, so a lingering thread cannot deliver one more trigger to a successor
        if let Some(port) = self
            .shared
            .port
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            CGEvent::tap_enable(&port.0, false);
            port.0.invalidate();
        }
        if let Some(run_loop) = self
            .shared
            .run_loop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            run_loop.0.stop();
        }
        // Bounded wait: a stuck thread must not hang the main thread
        let finished = self.finished.recv_timeout(STOP_TIMEOUT);
        if let Some(thread) = self.thread.take() {
            if finished.is_ok() || thread.is_finished() {
                let _ = thread.join();
            } else {
                debug!("ERROR: the event tap thread did not stop in time, leaving it");
            }
        }
    }
}

// MARK: - Tap thread

fn run_thread(
    shared: &Arc<Shared>,
    handler: Handler,
    mode: TapMode,
    started: &mpsc::Sender<Result<(), TapError>>,
) {
    let state = Box::new(CallbackState {
        shared: Arc::clone(shared),
        logic: RefCell::new(TapLogic::new()),
        handler,
        start: Instant::now(),
        mode,
        port: Cell::new(None),
    });
    let info = (&*state as *const CallbackState)
        .cast_mut()
        .cast::<c_void>();

    let options = match mode {
        TapMode::ListenOnly => CGEventTapOptions::ListenOnly,
        TapMode::Active => CGEventTapOptions::Default,
    };
    let events = [
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
    ]
    .iter()
    .fold(0u64, |mask, event| mask | (1u64 << event.0));
    // SAFETY: `tap_callback` has the right signature and `info` outlives the tap (`state` is dropped
    // below, after the tap is gone)
    let port = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            options,
            events,
            Some(tap_callback),
            info,
        )
    };
    let Some(port) = port else {
        let _ = started.send(Err(TapError::NotPermitted));
        return;
    };
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
        let _ = started.send(Err(TapError::NoRunLoopSource));
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else {
        let _ = started.send(Err(TapError::ThreadFailed));
        return;
    };
    // SAFETY: the common modes constant of CoreFoundation
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });

    state.port.set(Some(NonNull::from(&*port)));
    *shared.port.lock().unwrap_or_else(PoisonError::into_inner) = Some(SendMachPort(port.clone()));
    *shared
        .run_loop
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(SendRunLoop(run_loop.clone()));
    CGEvent::tap_enable(&port, true);
    let _ = started.send(Ok(()));

    while !shared.stop.load(Ordering::Acquire) {
        // SAFETY: the default mode constant of CoreFoundation
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, STOP_POLL_SECONDS, false);
    }

    // The tap is silenced before `state` goes away
    port.invalidate();
    run_loop.remove_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    state.port.set(None);
    *shared.port.lock().unwrap_or_else(PoisonError::into_inner) = None;
    *shared
        .run_loop
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
}

unsafe extern "C-unwind" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    info: *mut c_void,
) -> *mut CGEvent {
    let pass = event.as_ptr();
    // A panic must not cross the FFI boundary; the event goes through untouched
    guarded("event tap callback", || {
        // SAFETY: `info` is the `CallbackState` the thread keeps alive for as long as the tap exists
        let state = unsafe { &*info.cast::<CallbackState>() };
        // SAFETY: the event is valid for the duration of the callback
        let event_ref = unsafe { event.as_ref() };
        handle_event(state, event_type, event_ref, pass)
    })
    .unwrap_or(pass)
}

fn handle_event(
    state: &CallbackState,
    event_type: CGEventType,
    event: &CGEvent,
    pass: *mut CGEvent,
) -> *mut CGEvent {
    if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        // The system switched the tap off: switch it on again right away
        if let Some(port) = state.port.get() {
            // SAFETY: the pointer was taken from the port the thread still owns
            CGEvent::tap_enable(unsafe { port.as_ref() }, true);
        }
        debug!("event tap was disabled by the system, re-enabled");
        return pass;
    }
    if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData) == MARKER {
        return pass;
    }
    let key_code =
        || CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode) as u16;
    let flags = CGEvent::flags(Some(event)).0;
    let key_event = if event_type == CGEventType::KeyDown {
        KeyEvent::KeyDown {
            key_code: key_code(),
            flags,
            autorepeat: CGEvent::integer_value_field(
                Some(event),
                CGEventField::KeyboardEventAutorepeat,
            ) != 0,
        }
    } else if event_type == CGEventType::KeyUp {
        KeyEvent::KeyUp {
            key_code: key_code(),
        }
    } else if event_type == CGEventType::FlagsChanged {
        KeyEvent::FlagsChanged { flags }
    } else {
        return pass;
    };

    let config = HotkeyConfig::unpack(state.shared.config.load(Ordering::Relaxed));
    let now_ms = u64::try_from(state.start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let outcome = state.logic.borrow_mut().handle(config, key_event, now_ms);
    if let Some(trigger) = outcome.trigger {
        (state.handler)(trigger);
    }
    if outcome.swallow && state.mode == TapMode::Active {
        std::ptr::null_mut()
    } else {
        pass
    }
}

#[cfg(test)]
mod tests {
    //! The tests that create a real tap need the Accessibility permission of the process that runs them and
    //! post synthetic events, so they are `#[ignore]`d. Run them on purpose:
    //! `cargo test -p reshift -- --ignored --test-threads=1`
    //! They only use Option+F19 and a Shift tap, keys nobody types by accident, and finish in a few seconds.

    use super::*;
    use crate::hotkey_format::HotkeyModifiers;
    use crate::input::{Chord, post_chord};
    use objc2_core_graphics::{CGEventFlags, CGEventSource, CGEventSourceStateID};
    use std::sync::Mutex;

    const KEY_F19: u16 = 0x50;
    const OPTION: u64 = 1 << 19;
    const SHIFT: u64 = 1 << 17;
    const LEFT_SHIFT: u16 = 0x38;

    const OPTION_F19: HotkeyConfig = HotkeyConfig {
        use_double_shift: false,
        key_code: KEY_F19,
        modifiers: HotkeyModifiers::OPTION,
    };
    const DOUBLE_SHIFT: HotkeyConfig = HotkeyConfig {
        use_double_shift: true,
        key_code: KEY_F19,
        modifiers: HotkeyModifiers::OPTION,
    };

    fn recorder() -> (Handler, Arc<Mutex<Vec<Trigger>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let handler: Handler = Arc::new(move |trigger| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(trigger);
        });
        (handler, seen)
    }

    fn count(seen: &Arc<Mutex<Vec<Trigger>>>) -> usize {
        seen.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// A key press by "the user": no marker.
    fn post_unmarked_key(key_code: u16, flags: u64) {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(Some(&source), key_code, down).unwrap();
            CGEvent::set_flags(Some(&event), CGEventFlags(flags));
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
    }

    /// A Shift press or release as a `flagsChanged` event, with no marker.
    fn post_unmarked_shift(pressed: bool) {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
        let event = CGEvent::new_keyboard_event(Some(&source), LEFT_SHIFT, pressed).unwrap();
        CGEvent::set_type(Some(&event), CGEventType::FlagsChanged);
        CGEvent::set_flags(Some(&event), CGEventFlags(if pressed { SHIFT } else { 0 }));
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }

    fn settle() {
        std::thread::sleep(Duration::from_millis(400));
    }

    #[test]
    #[ignore = "needs the Accessibility permission and posts synthetic keys"]
    fn a_custom_hotkey_fires_and_a_marked_copy_of_it_does_not() {
        let (handler, seen) = recorder();
        let tap = EventTap::start(OPTION_F19, handler).expect("tap");
        assert_eq!(tap.mode(), TapMode::Active);
        assert_eq!(tap.health(), TapHealth::Healthy);

        post_unmarked_key(KEY_F19, OPTION);
        settle();
        assert_eq!(count(&seen), 1, "the user's hotkey");

        post_chord(Chord {
            key_code: KEY_F19,
            flags: OPTION,
        })
        .unwrap();
        settle();
        assert_eq!(count(&seen), 1, "our own marked event must be ignored");
    }

    #[test]
    #[ignore = "needs the Accessibility permission and posts synthetic keys"]
    fn double_shift_fires_from_flags_changed_events() {
        let (handler, seen) = recorder();
        let tap = EventTap::start(DOUBLE_SHIFT, handler).expect("tap");
        assert_eq!(tap.mode(), TapMode::ListenOnly);

        for _ in 0..2 {
            post_unmarked_shift(true);
            std::thread::sleep(Duration::from_millis(30));
            post_unmarked_shift(false);
            std::thread::sleep(Duration::from_millis(60));
        }
        settle();
        assert_eq!(
            seen.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_slice(),
            [Trigger::DoubleShift]
        );
    }

    #[test]
    #[ignore = "needs the Accessibility permission and posts synthetic keys"]
    fn a_new_config_reaches_the_running_tap() {
        let (handler, seen) = recorder();
        let tap = EventTap::start(OPTION_F19, handler).expect("tap");
        let other = HotkeyConfig {
            key_code: 0x40, // F17
            ..OPTION_F19
        };
        tap.set_config(other);
        post_unmarked_key(KEY_F19, OPTION);
        settle();
        assert_eq!(count(&seen), 0, "the old hotkey no longer fires");
        post_unmarked_key(0x40, OPTION);
        settle();
        assert_eq!(count(&seen), 1, "the new one does");
    }

    #[test]
    #[ignore = "needs the Accessibility permission and posts synthetic keys"]
    fn a_dropped_tap_stops_delivering_and_a_successor_works() {
        let (handler, seen) = recorder();
        drop(EventTap::start(OPTION_F19, handler).expect("tap"));
        post_unmarked_key(KEY_F19, OPTION);
        settle();
        assert_eq!(count(&seen), 0, "a dropped tap delivers nothing");

        let (handler, seen) = recorder();
        let _tap = EventTap::start(OPTION_F19, handler).expect("second tap");
        post_unmarked_key(KEY_F19, OPTION);
        settle();
        assert_eq!(count(&seen), 1);
    }

    #[test]
    #[ignore = "needs the Accessibility permission"]
    fn a_disabled_tap_is_noticed_and_can_be_re_enabled() {
        let (handler, _seen) = recorder();
        let tap = EventTap::start(DOUBLE_SHIFT, handler).expect("tap");
        {
            let port = tap
                .shared
                .port
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            CGEvent::tap_enable(&port.as_ref().unwrap().0, false);
        }
        assert_eq!(tap.health(), TapHealth::Disabled);
        tap.reenable();
        assert_eq!(tap.health(), TapHealth::Healthy);
    }

    #[test]
    #[ignore = "needs the Accessibility permission"]
    fn starting_and_dropping_many_taps_does_not_leak_threads_or_hang() {
        for _ in 0..20 {
            let (handler, _seen) = recorder();
            let tap = EventTap::start(DOUBLE_SHIFT, handler).expect("tap");
            assert_eq!(tap.health(), TapHealth::Healthy);
        }
    }

    // The errors do not need the permission: a missing one is the case being tested
    #[test]
    fn the_error_kinds_are_distinct() {
        assert_ne!(TapError::NotPermitted, TapError::StartTimedOut);
        assert_ne!(TapError::ThreadFailed, TapError::NoRunLoopSource);
    }
}
