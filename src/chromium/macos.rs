//! macOS: the pages' views are AppKit views inside the Rune window's content view, Chromium's
//! turns run on the main dispatch queue, and the framework is loaded from the app bundle.

use std::ffi::c_void;

use cef::*;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use rune::raw_window_handle::RawWindowHandle;

pub type View = Retained<NSView>;
pub type PopupWindow = Retained<NSWindow>;
/// Keeps the framework loaded.
pub type Loader = library_loader::LibraryLoader;

/// Loads the framework from the app bundle and teaches the application Chromium's protocols.
pub fn load() -> Result<Loader, String> {
    MainThreadMarker::new().ok_or("Chromium starts on the main thread")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let frameworks = exe.parent().map(|dir| dir.join("../Frameworks/Chromium Embedded Framework.framework"));
    if !frameworks.is_some_and(|path| path.exists()) {
        return Err("not running from Rune Browser.app (use scripts/run.sh)".into());
    }
    let loader = library_loader::LibraryLoader::new(&exe, false);
    if !loader.load() {
        return Err("the Chromium Embedded Framework failed to load".into());
    }
    super::app_protocol::install();
    Ok(loader)
}

/// The settings only this platform needs: none, the bundle says where the helpers are.
pub fn settings() -> Settings {
    Settings::default()
}

/// The window's content view.
pub fn parent(window: &rune::WindowHandle) -> Result<View, String> {
    window
        .with_raw_handle(|handle| match handle.as_raw() {
            // SAFETY: the handle's view is the window's live content view.
            RawWindowHandle::AppKit(handle) => unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) },
            _ => None,
        })?
        .ok_or_else(|| "the window has no AppKit view".into())
}

/// What Chromium is told to parent a browser to.
pub fn handle(view: &View) -> sys::cef_window_handle_t {
    Retained::as_ptr(view).cast_mut().cast::<c_void>()
}

/// The event a key event came from, as Chromium hands it to a keyboard handler.
pub type OsEvent<'a> = *mut u8;

/// A hidden, layer-backed view over the window's content view. Every tab's browser view goes
/// inside it, so one frame and one corner radius place them all.
pub fn container(parent: &View) -> View {
    let mtm = MainThreadMarker::new().expect("main thread");
    let view = NSView::initWithFrame(mtm.alloc(), NSRect::ZERO);
    view.setWantsLayer(true);
    view.setHidden(true);
    parent.addSubview(&view);
    view
}

/// A view's size in points.
pub fn size(view: &View) -> (f64, f64) {
    let bounds = view.bounds();
    (bounds.size.width, bounds.size.height)
}

/// Moves the container to `rect` (window points from the top-left, as Rune lays out), less
/// the `cover_top` points at its top the app draws on, and rounds its corners: all four, or
/// only the bottom two when something sits flush above it. `shown` is false while the tab on
/// screen has no page.
pub fn place(container: &NSView, parent: &NSView, [x, y, w, h]: [f32; 4], radius: f32, square_top: bool, cover_top: f32, shown: bool) {
    let cover = f64::from(cover_top.clamp(0.0, h.max(0.0)));
    let (x, y, w, h) = (f64::from(x), f64::from(y) + cover, f64::from(w), f64::from(h) - cover);
    let y = if parent.isFlipped() { y } else { parent.bounds().size.height - y - h };
    container.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
    // CACornerMask: min-x min-y, max-x min-y, min-x max-y, max-x max-y. A layer's y runs up
    // unless its view is flipped, so "min y" is the bottom edge there.
    let (bottom, top): (usize, usize) = if container.isFlipped() { (4 | 8, 1 | 2) } else { (1 | 2, 4 | 8) };
    let corners = if square_top || cover > 0.0 { bottom } else { bottom | top };
    // SAFETY: a layer-backed view's layer is a CALayer, which has these properties.
    unsafe {
        let layer: *mut AnyObject = msg_send![container, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: f64::from(radius)];
            let _: () = msg_send![layer, setMaskedCorners: corners];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
    container.setHidden(!shown || w < 1.0 || h < 1.0);
}

/// Sizes a browser's view to the whole page area, held to the container's bottom edge: when
/// the container is cut short at the top, the page keeps its size and is clipped there.
pub fn fill(browser_view: &NSView, (width, height): (f64, f64)) {
    // SAFETY: the superview is the live container, used on the main thread.
    let container = unsafe { browser_view.superview() };
    let container_height = container.as_ref().map_or(height, |container| container.bounds().size.height);
    let flipped = container.as_ref().is_some_and(|container| container.isFlipped());
    let y = if flipped { container_height - height } else { 0.0 };
    browser_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
    browser_view.setFrame(NSRect::new(NSPoint::new(0.0, y), NSSize::new(width.max(1.0), height.max(1.0))));
}

pub fn set_hidden(view: &NSView, hidden: bool) {
    view.setHidden(hidden);
}

/// Gives `view` the keyboard in its window.
pub fn focus(view: &NSView) {
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(view));
    }
}

/// A windowed browser's view.
pub fn view_of(browser: &Browser) -> Option<View> {
    let handle = browser.host()?.window_handle();
    // SAFETY: a windowed browser's handle is its live NSView, used on the main thread.
    unsafe { Retained::retain(handle.cast::<NSView>()) }
}

/// Takes a closing browser's view out of its window, which lets Chromium finish destroying it.
pub fn remove(view: &NSView) {
    view.removeFromSuperview();
}

/// Runs `f` in an autorelease pool of its own, so views released in it are freed in it.
pub fn drain(f: impl FnOnce()) {
    objc2::rc::autoreleasepool(|_| f());
}

/// Sets the opacity of the window `view` is in.
pub fn set_window_alpha(view: &View, alpha: f64) {
    if let Some(window) = view.window() {
        window.setAlphaValue(alpha);
    }
}

/// Chromium's native events come through AppKit's own loop: nothing to dispatch.
pub fn dispatch() {}

/// Work for the main thread from any thread.
pub type Post = Box<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// The main dispatch queue takes Chromium's turns here, so they also run while AppKit tracks
/// a resize or a menu: the app's own way to post work isn't needed.
pub fn set_post(_post: Post) {}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_after_f(when: u64, queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_time(when: u64, delta: i64) -> u64;
}

/// Runs `job` on the main thread in `delay_ms`. Any thread.
pub fn after(delay_ms: i64, job: Box<dyn FnOnce() + Send>) {
    type Job = Box<dyn FnOnce() + Send>;
    extern "C" fn run(context: *mut c_void) {
        // SAFETY: `context` is the box `after` leaked, delivered exactly once.
        let job = unsafe { Box::from_raw(context.cast::<Job>()) };
        job();
    }
    let context = Box::into_raw(Box::new(job)).cast::<c_void>();
    // SAFETY: the main queue lives for the whole process and `run` frees `context`.
    unsafe {
        let queue = &raw const _dispatch_main_q;
        if delay_ms <= 0 {
            dispatch_async_f(queue, context, run);
        } else {
            dispatch_after_f(dispatch_time(0, delay_ms * 1_000_000), queue, context, run);
        }
    }
}

/// Chromium's own dialogs (adding an extension, a passkey prompt) are windows it makes
/// without the app's say. Embedded like this it leaves them with an opaque title bar over
/// their top edge and places them by a window it doesn't have. Each one is given a clear
/// title bar, tied to the app's window so it moves and stays with it, and centred near the
/// top of the page area, `page` (window points from the top-left). Called every turn of the
/// loop: a dialog is corrected as it appears and again whenever Chromium moves or resizes it.
pub fn tend_dialogs(parent: &View, [x, y, w, h]: [f32; 4], reveal: bool) {
    const TITLED: usize = 1;
    const FRAME_BITS: usize = 0xf;
    const FULL_SIZE_CONTENT: usize = 1 << 15;
    let Some(window) = parent.window() else { return };
    // SAFETY: plain AppKit calls on the main thread, on windows the app owns.
    unsafe {
        let Some(class) = AnyClass::get(c"NSApplication") else { return };
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        let home = window.frame();
        for index in 0..count {
            let dialog: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let visible: bool = msg_send![dialog, isVisible];
            let mask: usize = msg_send![dialog, styleMask];
            // Chromium's own windows that aren't ordinary ones: no close button, no resizing.
            if mask & FRAME_BITS > TITLED || (*dialog).class().name().to_bytes() != b"NativeWidgetMacNSWindow" {
                continue;
            }
            // A dialog the hidden extensions page asked for is as hidden as that page's
            // window: `reveal` says one is expected, and it is brought out over this window.
            if !visible {
                if !reveal || mask & FRAME_BITS != TITLED {
                    continue;
                }
                let _: () = msg_send![dialog, setAlphaValue: 1.0f64];
                let _: () = msg_send![&*window, addChildWindow: dialog, ordered: 1isize];
                let _: () = msg_send![dialog, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            }
            // Without a parent it can open behind the browser window and never be seen.
            let parent: *mut AnyObject = msg_send![dialog, parentWindow];
            if parent.is_null() {
                let _: () = msg_send![&*window, addChildWindow: dialog, ordered: 1isize];
            }
            // The rest is for a titled dialog. A borderless one (a passkey prompt, a tooltip)
            // is drawn whole and placed by Chromium.
            if mask & FRAME_BITS != TITLED || mask & FULL_SIZE_CONTENT == 0 {
                continue;
            }
            let clear: bool = msg_send![dialog, titlebarAppearsTransparent];
            if !clear {
                let _: () = msg_send![dialog, setTitlebarAppearsTransparent: true];
                let _: () = msg_send![dialog, setTitleVisibility: 1isize];
            }
            let frame: NSRect = msg_send![dialog, frame];
            let drop = ((f64::from(h) - frame.size.height) / 2.0).clamp(0.0, 72.0);
            let left = home.origin.x + f64::from(x) + (f64::from(w) - frame.size.width) / 2.0;
            let bottom = home.origin.y + home.size.height - f64::from(y) - drop - frame.size.height;
            if (frame.origin.x - left).abs() > 0.5 || (frame.origin.y - bottom).abs() > 0.5 {
                let _: () = msg_send![dialog, setFrameOrigin: NSPoint::new(left.round(), bottom.round())];
            }
        }
    }
}

/// `frame` (window points from the top-left: x, y, width, height) on the screen.
fn on_screen(parent: &View, [x, y, w, h]: [f64; 4]) -> NSRect {
    let home = parent.window().map_or(NSRect::ZERO, |window| window.frame());
    NSRect::new(NSPoint::new(home.origin.x + x, home.origin.y + home.size.height - y - h), NSSize::new(w, h))
}

/// An extension popup's window at `frame` (window points from the top-left), unseen until
/// [`popup_resize`], and the view its browser goes in.
pub fn popup_open(parent: &View, frame: [f64; 4]) -> Option<(PopupWindow, View)> {
    let mtm = MainThreadMarker::new()?;
    // Titled, so it may take the keyboard and has rounded corners, with the title bar clear
    // and the page drawn under it.
    const TITLED_FULL_SIZE: usize = 1 | 1 << 15;
    const BUFFERED: usize = 2;
    // SAFETY: plain AppKit calls on the main thread; the window is released by `Retained`,
    // not by closing.
    let window: Retained<NSWindow> = unsafe {
        let window: Retained<NSWindow> = msg_send![mtm.alloc::<NSWindow>(), initWithContentRect: on_screen(parent, frame), styleMask: TITLED_FULL_SIZE, backing: BUFFERED, defer: false];
        let _: () = msg_send![&*window, setReleasedWhenClosed: false];
        let _: () = msg_send![&*window, setTitlebarAppearsTransparent: true];
        let _: () = msg_send![&*window, setTitleVisibility: 1isize];
        let _: () = msg_send![&*window, setMovable: false];
        let _: () = msg_send![&*window, setExcludedFromWindowsMenu: true];
        let _: () = msg_send![&*window, setAnimationBehavior: 2isize];
        let _: () = msg_send![&*window, setAlphaValue: 0.0f64];
        window
    };
    let content = window.contentView()?;
    Some((window, content))
}

/// Puts the popup over the app's window with the keyboard in `view`, its browser's. It stays
/// clear until [`popup_resize`].
pub fn popup_present(parent: &View, popup: &PopupWindow, view: Option<&View>) {
    let Some(window) = parent.window() else { return };
    // SAFETY: plain AppKit calls on the main thread.
    unsafe {
        let _: () = msg_send![&*window, addChildWindow: &**popup, ordered: 1isize];
        let _: () = msg_send![&**popup, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
    if let Some(view) = view {
        popup.makeFirstResponder(Some(view));
    }
}

/// Moves the popup to `frame` (window points from the top-left) and shows it.
pub fn popup_resize(parent: &View, popup: &PopupWindow, frame: [f64; 4]) {
    popup.setFrame_display(on_screen(parent, frame), true);
    popup.setAlphaValue(1.0);
}

/// Makes `view`, a popup's browser, fill the popup. While a page fits itself, the page's own
/// view (inside the browser's) takes the page's size: it is given the browser view's too.
pub fn popup_fill(popup: &PopupWindow, view: &View) {
    let Some(content) = popup.contentView() else { return };
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    view.setFrame(content.bounds());
    let bounds = view.bounds();
    for inner in view.subviews().iter() {
        inner.setFrame(bounds);
    }
}

/// Takes the popup off the screen.
pub fn popup_hide(popup: &PopupWindow) {
    // SAFETY: plain AppKit calls on the main thread.
    unsafe {
        let parent: *mut AnyObject = msg_send![&**popup, parentWindow];
        if !parent.is_null() {
            let _: () = msg_send![parent, removeChildWindow: &**popup];
        }
        let _: () = msg_send![&**popup, orderOut: std::ptr::null::<AnyObject>()];
    }
}

pub fn popup_close(popup: PopupWindow) {
    popup.close();
}

/// Whether the popup has the keyboard.
pub fn popup_is_key(popup: &PopupWindow) -> bool {
    popup.isKeyWindow()
}
