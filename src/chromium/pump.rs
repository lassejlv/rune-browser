//! Runs Chromium's work on the app's own main loop. Chromium asks for a turn through
//! `on_schedule_message_pump_work` (from any thread); each turn is one `do_message_loop_work`
//! on the main thread, queued through the main dispatch queue so it also runs while AppKit
//! tracks a resize or a menu.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The longest Chromium waits for a turn while idle, as CEF's reference pump does.
const MAX_DELAY_MS: i64 = 1000 / 30;

static GENERATION: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

thread_local! {
    static WORKING: Cell<bool> = const { Cell::new(false) };
    static AGAIN: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_after_f(when: u64, queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_time(when: u64, delta: i64) -> u64;
}

pub fn start() {
    RUNNING.store(true, Ordering::SeqCst);
    schedule(0);
}

pub fn stop() {
    RUNNING.store(false, Ordering::SeqCst);
}

/// Asks for a turn in `delay_ms`, replacing any turn asked for earlier. Any thread.
pub fn schedule(delay_ms: i64) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let context = generation as usize as *mut c_void;
    // SAFETY: the main queue lives for the whole process and `turn` only reads its context
    // as a number.
    unsafe {
        let queue = &raw const _dispatch_main_q;
        if delay_ms <= 0 {
            dispatch_async_f(queue, context, turn);
        } else {
            let when = dispatch_time(0, delay_ms.min(MAX_DELAY_MS) * 1_000_000);
            dispatch_after_f(when, queue, context, turn);
        }
    }
}

extern "C" fn turn(context: *mut c_void) {
    // A later request replaced this one.
    if context as usize as u64 != GENERATION.load(Ordering::SeqCst) {
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
