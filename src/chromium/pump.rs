//! Runs Chromium's work on the app's own main loop. Chromium asks for a turn through
//! `on_schedule_message_pump_work` (from any thread); each turn is one `do_message_loop_work`
//! on the main thread, queued through [`native::after`]: the main dispatch queue on macOS, so
//! it also runs while AppKit tracks a resize or a menu, and the app's event loop elsewhere.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::native;

/// The longest Chromium waits for a turn while idle, as CEF's reference pump does.
const MAX_DELAY_MS: i64 = 1000 / 30;

static GENERATION: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

thread_local! {
    static WORKING: Cell<bool> = const { Cell::new(false) };
    static AGAIN: Cell<bool> = const { Cell::new(false) };
}

/// Starts the turns; `post` runs work on the main thread from any thread.
pub fn start(post: native::Post) {
    native::set_post(post);
    RUNNING.store(true, Ordering::SeqCst);
    schedule(0);
}

pub fn stop() {
    RUNNING.store(false, Ordering::SeqCst);
}

/// Asks for a turn in `delay_ms`, replacing any turn asked for earlier. Any thread.
pub fn schedule(delay_ms: i64) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    native::after(delay_ms.min(MAX_DELAY_MS), Box::new(move || turn(generation)));
}

fn turn(generation: u64) {
    // A later request replaced this one.
    if generation != GENERATION.load(Ordering::SeqCst) {
        return;
    }
    work();
}

/// One turn of Chromium's loop, on the main thread.
pub fn work() {
    if !RUNNING.load(Ordering::SeqCst) {
        return;
    }
    // Chromium can ask for a turn from inside one (a nested menu or dialog loop).
    if WORKING.get() {
        AGAIN.set(true);
        return;
    }
    let before = GENERATION.load(Ordering::SeqCst);
    WORKING.set(true);
    cef::do_message_loop_work();
    native::dispatch();
    WORKING.set(false);
    super::tend_dialogs();
    super::popup::tend();
    if AGAIN.take() {
        schedule(0);
    } else if GENERATION.load(Ordering::SeqCst) == before {
        // Chromium asked for nothing more: check back at the idle rate.
        schedule(MAX_DELAY_MS);
    }
}
