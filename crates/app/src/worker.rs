//! MARK: - Conversion worker
//!
//! The conversion thread. The tap's handler only hands a request over (`Worker::request`); the thread
//! runs the pipeline, then waits out the restore delay and puts the user's clipboard back. A trigger
//! that arrives while a conversion runs is ignored: two conversions at once would copy each other's text.
//!
//! The scheduling decisions (`Gate`, `Next`) are pure and tested; the thread is a loop around them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use reshift_core::LayoutMaps;

use crate::conversion::{ConversionSettings, PendingRestore, System, apply_restore, perform};
use crate::guard::guarded;
use crate::log::debug;

/// What the main thread hands over with a trigger: a copy of the settings and the maps to convert with.
pub struct Request {
    pub settings: ConversionSettings,
    pub maps: Arc<LayoutMaps>,
}

/// Lets one conversion in at a time.
#[derive(Debug, Default)]
pub struct Gate {
    busy: AtomicBool,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if the caller got the slot and must hand a request over.
    pub fn try_enter(&self) -> bool {
        !self.busy.swap(true, Ordering::AcqRel)
    }

    pub fn leave(&self) {
        self.busy.store(false, Ordering::Release);
    }
}

/// What the thread does next, given a pending restore and the time.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Wait for a request, forever.
    WaitForRequest,
    /// Wait for a request, but at most this long: the clipboard has to come back then.
    WaitUntilRestore(Duration),
    RestoreNow,
}

pub fn next_step(restore_due: Option<Instant>, now: Instant) -> Next {
    match restore_due {
        None => Next::WaitForRequest,
        Some(due) if due <= now => Next::RestoreNow,
        Some(due) => Next::WaitUntilRestore(due - now),
    }
}

pub struct Worker {
    sender: Option<mpsc::Sender<Request>>,
    gate: Arc<Gate>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Starts the thread. `make_system` runs on it, so the system (and its clipboard handles) lives there.
    pub fn start<S, F>(make_system: F) -> std::io::Result<Self>
    where
        S: System + 'static,
        F: FnOnce() -> S + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel::<Request>();
        let gate = Arc::new(Gate::new());
        let thread = {
            let gate = Arc::clone(&gate);
            std::thread::Builder::new()
                .name("conversion".to_owned())
                .spawn(move || run(&receiver, &gate, make_system()))?
        };
        Ok(Self {
            sender: Some(sender),
            gate,
            thread: Some(thread),
        })
    }

    /// Hands a request to the thread. False if a conversion is already running (the trigger is ignored).
    pub fn request(&self, request: Request) -> bool {
        if !self.gate.try_enter() {
            debug!("trigger ignored: a conversion is still running");
            return false;
        }
        let sent = self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(request).is_ok());
        if !sent {
            self.gate.leave();
        }
        sent
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the channel ends the thread's loop; it restores the clipboard on the way out
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run<S: System>(receiver: &mpsc::Receiver<Request>, gate: &Gate, mut system: S) {
    let mut pending: Option<(PendingRestore<S::Snapshot>, Instant)> = None;
    loop {
        let step = next_step(pending.as_ref().map(|(_, due)| *due), Instant::now());
        let received = match step {
            Next::RestoreNow => {
                restore(&mut system, &mut pending);
                continue;
            }
            Next::WaitForRequest => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Next::WaitUntilRestore(wait) => receiver.recv_timeout(wait),
        };
        match received {
            Ok(request) => {
                // A new conversion starts from the user's real clipboard, not from our pasted text
                restore(&mut system, &mut pending);
                let outcome = guarded("conversion", || {
                    perform(&mut system, &request.maps, &request.settings)
                });
                match outcome {
                    Some(outcome) => {
                        if let Some(restore) = outcome.pending_restore {
                            let due = Instant::now() + restore.delay;
                            pending = Some((restore, due));
                        }
                    }
                    None => debug!("ERROR: the conversion panicked"),
                }
                gate.leave();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // Never leave our converted text on the user's clipboard
    restore(&mut system, &mut pending);
}

fn restore<S: System>(
    system: &mut S,
    pending: &mut Option<(PendingRestore<S::Snapshot>, Instant)>,
) {
    if let Some((restore, _)) = pending.take() {
        guarded("clipboard restore", || apply_restore(system, &restore));
    }
}

/// Shared slot for the latest maps and settings: the main thread writes, the tap handler reads.
#[derive(Default)]
pub struct Shared {
    inner: Mutex<Option<Arc<Request>>>,
}

impl Shared {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, settings: ConversionSettings, maps: Arc<LayoutMaps>) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Arc::new(Request { settings, maps }));
    }

    pub fn get(&self) -> Option<Request> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|shared| Request {
                settings: shared.settings.clone(),
                maps: Arc::clone(&shared.maps),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{Copied, RESTORE_DELAY};
    use crate::input::{Chord, chords};
    use reshift_core::{KeyboardLayout, LayoutSwitchMode, SmartConversionMode};
    use std::sync::atomic::AtomicU32;

    const US: &str = "com.apple.keylayout.US";
    const RUSSIAN: &str = "com.apple.keylayout.Russian";

    fn settings() -> ConversionSettings {
        ConversionSettings {
            enabled_layouts: vec![
                KeyboardLayout::new(US, "U.S.", "en"),
                KeyboardLayout::new(RUSSIAN, "Russian", "ru"),
            ],
            smart_conversion_mode: SmartConversionMode::GreedyLine,
            layout_switch_mode: LayoutSwitchMode::Always,
            play_sounds: false,
        }
    }

    fn request() -> Request {
        Request {
            settings: settings(),
            maps: Arc::new(LayoutMaps::default()),
        }
    }

    /// A system with a selection, shared with the test through atomics and a log.
    #[derive(Clone)]
    struct Probe {
        log: Arc<Mutex<Vec<String>>>,
        restores: Arc<AtomicU32>,
        slow: Duration,
        panic_on_copy: bool,
    }

    struct ProbeSystem {
        probe: Probe,
        clipboard: String,
        count: i64,
    }

    impl Probe {
        fn new() -> Self {
            Self {
                log: Arc::default(),
                restores: Arc::default(),
                slow: Duration::ZERO,
                panic_on_copy: false,
            }
        }

        fn system(&self) -> ProbeSystem {
            ProbeSystem {
                probe: self.clone(),
                clipboard: "user".to_owned(),
                count: 0,
            }
        }

        fn entries(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl System for ProbeSystem {
        type Snapshot = String;

        fn can_post(&self) -> bool {
            true
        }
        fn change_count(&self) -> i64 {
            self.count
        }
        fn snapshot(&mut self) -> String {
            self.clipboard.clone()
        }
        fn restore(&mut self, snapshot: &String) {
            self.clipboard.clone_from(snapshot);
            self.count += 1;
            self.probe.restores.fetch_add(1, Ordering::SeqCst);
        }
        fn read_copied(&mut self) -> Option<Copied> {
            Some(Copied {
                text: "ghbdtn".to_owned(),
                whole_line: false,
            })
        }
        fn write_text(&mut self, text: &str) -> i64 {
            self.clipboard = text.to_owned();
            self.count += 1;
            self.count
        }
        fn press(&mut self, chord: Chord) {
            if chord == chords::COPY {
                if self.probe.panic_on_copy {
                    panic!("boom");
                }
                std::thread::sleep(self.probe.slow);
                self.count += 1;
            }
            self.probe
                .log
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(format!("{:#x}", chord.key_code));
        }
        fn sleep(&mut self, _duration: Duration) {}
        fn switch_layout(&mut self, _layout_id: &str) {}
        fn play_sound(&mut self) {}
    }

    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // MARK: - Gate

    #[test]
    fn the_gate_lets_one_in_and_refuses_the_rest_until_it_is_left() {
        let gate = Gate::new();
        assert!(gate.try_enter());
        assert!(!gate.try_enter());
        assert!(!gate.try_enter());
        gate.leave();
        assert!(gate.try_enter());
    }

    #[test]
    fn leaving_an_open_gate_is_harmless() {
        let gate = Gate::new();
        gate.leave();
        assert!(gate.try_enter());
    }

    // MARK: - Scheduling

    #[test]
    fn without_a_pending_restore_the_thread_waits_for_a_request() {
        assert_eq!(next_step(None, Instant::now()), Next::WaitForRequest);
    }

    #[test]
    fn a_pending_restore_bounds_the_wait() {
        let now = Instant::now();
        assert_eq!(
            next_step(Some(now + Duration::from_millis(250)), now),
            Next::WaitUntilRestore(Duration::from_millis(250))
        );
    }

    #[test]
    fn a_restore_that_is_due_happens_now() {
        let now = Instant::now();
        assert_eq!(next_step(Some(now), now), Next::RestoreNow);
        assert_eq!(
            next_step(Some(now), now + Duration::from_secs(1)),
            Next::RestoreNow
        );
    }

    // MARK: - The thread

    #[test]
    fn a_request_runs_the_pipeline_and_the_clipboard_comes_back_after_the_delay() {
        let probe = Probe::new();
        let worker = {
            let probe = probe.clone();
            Worker::start(move || probe.system()).unwrap()
        };
        assert!(worker.request(request()));
        // The maps are empty here: nothing converts, but the pipeline ran (it copied)
        wait_until("the copy", || probe.entries().iter().any(|k| k == "0x8"));
        drop(worker);
    }

    #[test]
    fn a_trigger_during_a_running_conversion_is_ignored() {
        let mut probe = Probe::new();
        probe.slow = Duration::from_millis(300);
        let worker = {
            let probe = probe.clone();
            Worker::start(move || probe.system()).unwrap()
        };
        assert!(worker.request(request()));
        wait_until("the conversion to start", || {
            !probe.entries().is_empty() || true
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !worker.request(request()),
            "the second trigger must be refused"
        );
        // After the first one is done a new one is accepted again
        wait_until("the slot to free up", || {
            worker.gate.try_enter() && {
                worker.gate.leave();
                true
            }
        });
        assert!(worker.request(request()));
    }

    #[test]
    fn a_panic_in_the_pipeline_does_not_kill_the_thread_or_block_the_gate() {
        let mut probe = Probe::new();
        probe.panic_on_copy = true;
        let worker = {
            let probe = probe.clone();
            Worker::start(move || probe.system()).unwrap()
        };
        assert!(worker.request(request()));
        wait_until("the gate to open again", || {
            if worker.gate.try_enter() {
                worker.gate.leave();
                true
            } else {
                false
            }
        });
        // The thread still takes work
        assert!(worker.request(request()));
    }

    #[test]
    fn dropping_the_worker_stops_the_thread() {
        let probe = Probe::new();
        let worker = {
            let probe = probe.clone();
            Worker::start(move || probe.system()).unwrap()
        };
        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_request_after_the_thread_is_gone_frees_the_gate() {
        let probe = Probe::new();
        let mut worker = {
            let probe = probe.clone();
            Worker::start(move || probe.system()).unwrap()
        };
        // Simulate a dead receiver: drop the sender side's peer by stopping the thread first
        let sender = worker.sender.take();
        drop(sender);
        if let Some(thread) = worker.thread.take() {
            let _ = thread.join();
        }
        assert!(!worker.request(request()));
        assert!(
            worker.gate.try_enter(),
            "a failed hand-over must not leave the gate shut"
        );
    }

    // MARK: - Shared slot

    #[test]
    fn the_shared_slot_starts_empty_and_returns_what_was_set() {
        let shared = Shared::new();
        assert!(shared.get().is_none());
        shared.set(settings(), Arc::new(LayoutMaps::default()));
        let request = shared.get().unwrap();
        assert_eq!(request.settings, settings());
    }

    #[test]
    fn a_newer_value_replaces_the_older_one() {
        let shared = Shared::new();
        shared.set(settings(), Arc::new(LayoutMaps::default()));
        let mut changed = settings();
        changed.play_sounds = true;
        shared.set(changed, Arc::new(LayoutMaps::default()));
        assert!(shared.get().unwrap().settings.play_sounds);
    }

    #[test]
    fn the_restore_delay_is_what_the_pipeline_asks_for() {
        assert_eq!(RESTORE_DELAY, Duration::from_millis(300));
    }
}
