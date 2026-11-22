//! Linux: the pages' windows are X11 child windows of the Rune window, and Chromium's turns
//! are posted to the app's event loop. libcef.so sits beside the binary, which finds it there.
//!
//! The windows are placed in pixels, Rune lays out in points: the two differ by the scale
//! winit gives the window, which comes from `WINIT_X11_SCALE_FACTOR` or the `Xft.dpi` resource.
//! A Wayland session runs the app through XWayland (see `main`): Chromium can only be given an
//! X11 window to draw in.

use std::ffi::{c_char, c_void};
use std::sync::{Condvar, LazyLock, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use cef::*;
use rune::raw_window_handle::RawWindowHandle;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, InputFocus, MapState, PropMode, StackMode, WindowClass};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

/// An X11 window.
#[derive(Clone, PartialEq)]
pub struct View(u32);

pub type PopupWindow = View;
/// libcef.so is linked: nothing to keep loaded.
pub struct Loader;

/// The app's own connection to the X server, for the windows it places.
fn x() -> Option<&'static XCBConnection> {
    static X: OnceLock<Option<XCBConnection>> = OnceLock::new();
    X.get_or_init(|| XCBConnection::connect(None).ok().map(|(connection, _)| connection)).as_ref()
}

/// Sends what was asked of the server.
fn flush() {
    if let Some(x) = x() {
        let _ = x.flush();
    }
}

/// Pixels per point.
fn scale() -> f64 {
    static SCALE: OnceLock<f64> = OnceLock::new();
    *SCALE.get_or_init(|| {
        if let Some(factor) = std::env::var("WINIT_X11_SCALE_FACTOR").ok().and_then(|value| value.parse::<f64>().ok()).filter(|factor| *factor > 0.0) {
            return factor;
        }
        let Some(x) = x() else { return 1.0 };
        let root = x.setup().roots[0].root;
        let resources = x.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 20).ok().and_then(|cookie| cookie.reply().ok());
        let dpi = resources.and_then(|reply| {
            String::from_utf8_lossy(&reply.value).lines().find_map(|line| line.strip_prefix("Xft.dpi:").and_then(|dpi| dpi.trim().parse::<f64>().ok()))
        });
        dpi.filter(|dpi| *dpi > 0.0).map_or(1.0, |dpi| dpi / 96.0)
    })
}

fn pixels(points: f64) -> i32 {
    (points * scale()).round() as i32
}

pub fn load() -> Result<Loader, String> {
    x().map(|_| Loader).ok_or_else(|| "no X server to show them on".into())
}

/// Where the helper is (beside the app), and no sandbox: Chromium's needs a setuid helper of
/// its own installed by root.
pub fn settings() -> Settings {
    let helper = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("rune-browser-helper")));
    Settings {
        browser_subprocess_path: helper.map(|helper| CefString::from(helper.to_string_lossy().as_ref())).unwrap_or_default(),
        no_sandbox: 1,
        ..Default::default()
    }
}

/// The Rune window.
pub fn parent(window: &rune::WindowHandle) -> Result<View, String> {
    window
        .with_raw_handle(|handle| match handle.as_raw() {
            RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok().map(View),
            RawWindowHandle::Xcb(handle) => Some(View(handle.window.get())),
            _ => None,
        })?
        .ok_or_else(|| "the window isn't an X11 window".into())
}

/// What Chromium is told to parent a browser to.
pub fn handle(view: &View) -> sys::cef_window_handle_t {
    sys::cef_window_handle_t::from(view.0)
}

/// The X event a key event came from, as Chromium hands it to a keyboard handler.
pub type OsEvent<'a> = Option<&'a mut sys::XEvent>;

/// An unmapped window inside the Rune window. Every tab's browser window goes inside it, so
/// one frame places them all.
pub fn container(parent: &View) -> View {
    let Some(x) = x() else { return View(NONE) };
    let Ok(id) = x.generate_id() else { return View(NONE) };
    let _ = x.create_window(COPY_DEPTH_FROM_PARENT, id, parent.0, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, COPY_FROM_PARENT, &CreateWindowAux::new());
    flush();
    View(id)
}

/// A window's size in points.
pub fn size(view: &View) -> (f64, f64) {
    let geometry = x().and_then(|x| x.get_geometry(view.0).ok()?.reply().ok());
    geometry.map_or((0.0, 0.0), |geometry| (f64::from(geometry.width) / scale(), f64::from(geometry.height) / scale()))
}

/// Moves the container to `rect` (window points from the top-left, as Rune lays out), less
/// the `cover_top` points at its top the app draws on. `shown` is false while the tab on
/// screen has no page. X11 windows are rectangles, so the corners stay square.
pub fn place(container: &View, _parent: &View, [left, top, width, height]: [f32; 4], _radius: f32, _square_top: bool, cover_top: f32, shown: bool) {
    let Some(connection) = x() else { return };
    let cover = f64::from(cover_top.clamp(0.0, height.max(0.0)));
    let (left, top, width, height) = (pixels(f64::from(left)), pixels(f64::from(top) + cover), pixels(f64::from(width)), pixels(f64::from(height) - cover));
    // Placed even while hidden: a page made then takes the container's size.
    if width >= 1 && height >= 1 {
        let _ = connection.configure_window(container.0, &ConfigureWindowAux::new().x(left).y(top).width(width as u32).height(height as u32).stack_mode(StackMode::ABOVE));
    }
    let _ = if shown && width >= 1 && height >= 1 { connection.map_window(container.0) } else { connection.unmap_window(container.0) };
    flush();
}

/// Sizes a browser's window to the whole page area, held to the container's bottom edge: when
/// the container is cut short at the top, the page keeps its size and is clipped there.
pub fn fill(browser_view: &View, (width, height): (f64, f64)) {
    let Some(x) = x() else { return };
    let container = x.query_tree(browser_view.0).ok().and_then(|cookie| cookie.reply().ok()).map(|tree| tree.parent);
    let container_height = container.and_then(|container| x.get_geometry(container).ok()?.reply().ok()).map_or(pixels(height), |geometry| i32::from(geometry.height));
    let (width, height) = (pixels(width).max(1), pixels(height).max(1));
    let _ = x.configure_window(browser_view.0, &ConfigureWindowAux::new().x(0).y(container_height - height).width(width as u32).height(height as u32));
    flush();
}

pub fn set_hidden(view: &View, hidden: bool) {
    let Some(x) = x() else { return };
    let _ = if hidden { x.unmap_window(view.0) } else { x.map_window(view.0) };
    flush();
}

/// Gives `view` the keyboard, when it is on screen.
pub fn focus(view: &View) {
    let Some(x) = x() else { return };
    let viewable = x.get_window_attributes(view.0).ok().and_then(|cookie| cookie.reply().ok()).is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE);
    if viewable {
        let _ = x.set_input_focus(InputFocus::PARENT, view.0, CURRENT_TIME);
        flush();
    }
}

/// A windowed browser's window.
pub fn view_of(browser: &Browser) -> Option<View> {
    let handle = browser.host()?.window_handle();
    u32::try_from(handle).ok().filter(|id| *id != NONE).map(View)
}

/// Destroys a closing browser's window, which lets Chromium finish destroying the browser.
pub fn remove(view: &View) {
    let Some(x) = x() else { return };
    let _ = x.destroy_window(view.0);
    flush();
}

pub fn drain(f: impl FnOnce()) {
    f();
}

/// The top-level window `view` is in.
fn top_level(view: &View) -> Option<u32> {
    let x = x()?;
    let mut window = view.0;
    loop {
        let tree = x.query_tree(window).ok()?.reply().ok()?;
        if tree.parent == tree.root || tree.parent == NONE {
            return Some(window);
        }
        window = tree.parent;
    }
}

/// Sets the opacity of the window `view` is in, where a compositing manager honours it.
pub fn set_window_alpha(view: &View, alpha: f64) {
    let (Some(x), Some(window)) = (x(), top_level(view)) else { return };
    let Some(atom) = x.intern_atom(false, b"_NET_WM_WINDOW_OPACITY").ok().and_then(|cookie| cookie.reply().ok()).map(|reply| reply.atom) else { return };
    let opacity = (alpha.clamp(0.0, 1.0) * f64::from(u32::MAX)) as u32;
    let _ = x.change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[opacity]);
    flush();
}

/// Dispatches what is waiting in GLib's main context: Chromium reads its X11 events (a page's
/// input, resizes, exposures) through a source there, which only a GLib loop runs. GLib is
/// libcef's own dependency, so it is looked up in the process rather than linked.
pub fn dispatch() {
    type Iteration = unsafe extern "C" fn(*mut c_void, i32) -> i32;
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    static ITERATION: OnceLock<Option<Iteration>> = OnceLock::new();
    let iteration = ITERATION.get_or_init(|| {
        // SAFETY: RTLD_DEFAULT (null) searches the loaded libraries; the symbol, when there,
        // is GLib's g_main_context_iteration, which has this signature.
        let symbol = unsafe { dlsym(std::ptr::null_mut(), c"g_main_context_iteration".as_ptr()) };
        (!symbol.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, Iteration>(symbol) })
    });
    let Some(iteration) = iteration else { return };
    // A bounded number of rounds, so a busy source can't hold up the app.
    for _ in 0..64 {
        // SAFETY: the default context (null), without blocking, on the main thread.
        if unsafe { iteration(std::ptr::null_mut(), 0) } == 0 {
            break;
        }
    }
}

/// Work for the main thread from any thread.
pub type Post = Box<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;
type Job = Box<dyn FnOnce() + Send>;

static POST: OnceLock<Post> = OnceLock::new();
/// A job and when it is due.
type Due = Option<(Instant, Job)>;

/// The one delayed job. A newer one replaces it.
static DUE: LazyLock<(Mutex<Due>, Condvar)> = LazyLock::new(|| (Mutex::new(None), Condvar::new()));

/// How work reaches the main thread: through the app's event loop.
pub fn set_post(post: Post) {
    let _ = POST.set(post);
}

fn post(job: Job) {
    if let Some(post) = POST.get() {
        post(job);
    }
}

/// Runs `job` on the main thread in `delay_ms`, in place of any job still waiting. Any thread.
pub fn after(delay_ms: i64, job: Job) {
    if delay_ms <= 0 {
        return post(job);
    }
    static TIMER: Once = Once::new();
    TIMER.call_once(|| {
        std::thread::Builder::new()
            .name("chromium-pump".into())
            .spawn(|| {
                let (due, wake) = &*DUE;
                let mut slot = due.lock().unwrap();
                loop {
                    match slot.take() {
                        None => slot = wake.wait(slot).unwrap(),
                        Some((when, job)) if when <= Instant::now() => {
                            drop(slot);
                            post(job);
                            slot = due.lock().unwrap();
                        }
                        Some((when, job)) => {
                            let wait = when.saturating_duration_since(Instant::now());
                            *slot = Some((when, job));
                            slot = wake.wait_timeout(slot, wait).unwrap().0;
                        }
                    }
                }
            })
            .expect("a thread for Chromium's timer");
    });
    let (due, wake) = &*DUE;
    *due.lock().unwrap() = Some((Instant::now() + Duration::from_millis(delay_ms as u64), job));
    wake.notify_one();
}

/// Chromium's dialogs are top-level windows the window manager places: nothing to put right.
pub fn tend_dialogs(_parent: &View, _page: [f32; 4], _reveal: bool) {}

/// An extension popup's window at `frame` (window points from the top-left), a child of the
/// app's window, unmapped until [`popup_resize`]. Its browser goes straight in it.
pub fn popup_open(parent: &View, frame: [f64; 4]) -> Option<(PopupWindow, View)> {
    let x = x()?;
    let id = x.generate_id().ok()?;
    let [left, top, width, height] = frame.map(pixels);
    x.create_window(COPY_DEPTH_FROM_PARENT, id, parent.0, left as i16, top as i16, width.max(1) as u16, height.max(1) as u16, 0, WindowClass::INPUT_OUTPUT, COPY_FROM_PARENT, &CreateWindowAux::new()).ok()?;
    flush();
    Some((View(id), View(id)))
}

/// Nothing to do until the popup knows its size: it is mapped then.
pub fn popup_present(_parent: &View, _popup: &PopupWindow, _view: Option<&View>) {}

/// Moves the popup to `frame` (window points from the top-left), its browser filling it, and
/// shows it with the keyboard in its page.
pub fn popup_resize(_parent: &View, popup: &PopupWindow, [left, top, width, height]: [f64; 4]) {
    let Some(x) = x() else { return };
    let (width, height) = (pixels(width).max(1) as u32, pixels(height).max(1) as u32);
    let _ = x.configure_window(popup.0, &ConfigureWindowAux::new().x(pixels(left)).y(pixels(top)).width(width).height(height).stack_mode(StackMode::ABOVE));
    let children = x.query_tree(popup.0).ok().and_then(|cookie| cookie.reply().ok()).map(|tree| tree.children).unwrap_or_default();
    for child in &children {
        let _ = x.configure_window(*child, &ConfigureWindowAux::new().x(0).y(0).width(width).height(height));
    }
    let mapped = x.get_window_attributes(popup.0).ok().and_then(|cookie| cookie.reply().ok()).is_some_and(|attributes| attributes.map_state != MapState::UNMAPPED);
    if !mapped {
        let _ = x.map_window(popup.0);
        if let Some(page) = children.first() {
            let _ = x.set_input_focus(InputFocus::PARENT, *page, CURRENT_TIME);
        }
    }
    flush();
}

/// Makes `view`, a popup's browser, fill the popup.
pub fn popup_fill(popup: &PopupWindow, view: &View) {
    let Some(x) = x() else { return };
    let Some(geometry) = x.get_geometry(popup.0).ok().and_then(|cookie| cookie.reply().ok()) else { return };
    let _ = x.configure_window(view.0, &ConfigureWindowAux::new().x(0).y(0).width(u32::from(geometry.width)).height(u32::from(geometry.height)));
    flush();
}

pub fn popup_hide(popup: &PopupWindow) {
    set_hidden(popup, true);
}

pub fn popup_close(popup: PopupWindow) {
    remove(&popup);
}

/// Whether the keyboard is in the popup or a window inside it.
pub fn popup_is_key(popup: &PopupWindow) -> bool {
    let Some(x) = x() else { return false };
    let Some(mut window) = x.get_input_focus().ok().and_then(|cookie| cookie.reply().ok()).map(|reply| reply.focus) else { return false };
    while window != NONE {
        if window == popup.0 {
            return true;
        }
        let Some(tree) = x.query_tree(window).ok().and_then(|cookie| cookie.reply().ok()) else { return false };
        if window == tree.root {
            return false;
        }
        window = tree.parent;
    }
    false
}
