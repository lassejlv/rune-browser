//! Chromium expects the `NSApplication` to answer `isHandlingSendEvent` and
//! `setHandlingSendEvent:` (its `CrAppProtocol` and `CrAppControlProtocol`), and to hold the
//! flag up while an event is being sent. The application here is winit's, so those two
//! methods are added to its class and its `sendEvent:` is wrapped.

use std::cell::Cell;
use std::ffi::{CStr, c_char};
use std::sync::OnceLock;

use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};

thread_local! {
    static HANDLING: Cell<bool> = const { Cell::new(false) };
}

type SendEvent = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);
static ORIGINAL_SEND_EVENT: OnceLock<SendEvent> = OnceLock::new();

unsafe extern "C-unwind" fn is_handling_send_event(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    Bool::new(HANDLING.get())
}

unsafe extern "C-unwind" fn set_handling_send_event(_this: *mut AnyObject, _cmd: Sel, handling: Bool) {
    HANDLING.set(handling.as_bool());
}

unsafe extern "C-unwind" fn send_event(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let was = HANDLING.replace(true);
    if let Some(original) = ORIGINAL_SEND_EVENT.get() {
        // SAFETY: the implementation `sendEvent:` had before, called with its own arguments.
        unsafe { original(this, cmd, event) };
    }
    HANDLING.set(was);
}

#[cfg(target_arch = "aarch64")]
const BOOL: &str = "B";
#[cfg(not(target_arch = "aarch64"))]
const BOOL: &str = "c";

/// Teaches the running application Chromium's protocols. Main thread, once, after the
/// application exists and before Chromium starts.
pub fn install() {
    if ORIGINAL_SEND_EVENT.get().is_some() {
        return;
    }
    // SAFETY: Objective-C runtime calls on the live application's class, on the main thread,
    // with implementations whose signatures match the type encodings given.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let class = ffi::object_getClass(app.cast()).cast_mut().cast::<AnyClass>();

        let getter = format!("{BOOL}@:\0");
        let setter = format!("v@:{BOOL}\0");
        ffi::class_addMethod(
            class,
            sel!(isHandlingSendEvent),
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool, Imp>(is_handling_send_event),
            getter.as_ptr().cast::<c_char>(),
        );
        ffi::class_addMethod(
            class,
            sel!(setHandlingSendEvent:),
            std::mem::transmute::<unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Bool), Imp>(set_handling_send_event),
            setter.as_ptr().cast::<c_char>(),
        );

        let method = ffi::class_getInstanceMethod(class, sel!(sendEvent:));
        if let Some(original) = ffi::method_getImplementation(method) {
            let _ = ORIGINAL_SEND_EVENT.set(std::mem::transmute::<Imp, SendEvent>(original));
            let wrapped = std::mem::transmute::<SendEvent, Imp>(send_event);
            // The class has its own `sendEvent:` (winit's) or inherits AppKit's: add ours
            // when it inherits, replace when it has one.
            if !ffi::class_addMethod(class, sel!(sendEvent:), wrapped, c"v@:@".as_ptr()).as_bool() {
                ffi::method_setImplementation(method.cast_mut(), wrapped);
            }
        }

        for name in [c"CrAppProtocol", c"CrAppControlProtocol", c"CefAppProtocol"] {
            add_protocol(class, name);
        }
    }
}

/// Declares conformance where the protocol is registered (the framework registers them).
unsafe fn add_protocol(class: *mut AnyClass, name: &CStr) {
    // SAFETY: `name` is NUL-terminated and `class` is a live class.
    unsafe {
        let protocol = ffi::objc_getProtocol(name.as_ptr());
        if !protocol.is_null() {
            ffi::class_addProtocol(class, protocol);
        }
    }
}
