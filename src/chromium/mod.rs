//! Chromium, embedded through CEF.
//!
//! Each tab is a Chromium browser drawing into a native view of its own. The views sit in one
//! container laid over the part of the window the Rune view leaves for the page, so Chromium
//! handles its own input, scrolling, menus and video there, and Rune draws the rest. What
//! those views are is [`native`]'s: AppKit views on macOS, X11 windows on Linux.
//!
//! Everything here runs on the main thread. Chromium calls back into [`client`] from inside
//! calls made here, so nothing holds the host borrowed across a call into Chromium, and the
//! callbacks only [`emit`] events.

#[cfg(target_os = "macos")]
mod app_protocol;
mod client;
pub mod manager;
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "x11.rs")]
mod native;
pub mod popup;
mod pump;
mod stray;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cef::*;

pub use native::Post;

pub type TabId = u64;

/// What Chromium tells the app.
#[derive(Debug, Clone)]
pub enum Event {
    /// Chromium is up: browsers can be created.
    Ready,
    Title(TabId, String),
    Address(TabId, String),
    Loading { tab: TabId, loading: bool, can_go_back: bool, can_go_forward: bool },
    /// A page asked for a new window.
    OpenTab { opener: TabId, url: String, background: bool },
    /// A shortcut pressed while the page had the keyboard.
    Command(Command),
    /// The page's icon, as PNG bytes.
    Favicon(TabId, Vec<u8>),
    /// The page took the keyboard (a click in it).
    PageFocused,
    /// A download began (`done` false) or ended.
    Download { id: u32, done: bool },
    /// How many matches a find has, and which one is selected (from 1).
    FindResult { tab: TabId, count: i32, active: i32 },
    /// The page asked to fill the screen (a video's full-screen button), or stopped.
    Fullscreen(TabId, bool),
    /// The page's main document failed to load.
    LoadFailed { tab: TabId, url: String, error: String },
    /// The page's process died.
    Crashed(TabId),
    /// An extension's popup asked for the tab it is over to go to an address.
    Navigate(String),
    /// The installed extensions, as JSON from Chromium's extension manager.
    Extensions(String),
    /// A site asks for something that needs the person's say. Answer with
    /// [`answer_permission`].
    Permission { id: u64, tab: TabId, origin: String, what: String },
}

/// The app's keyboard commands, wherever the keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    CloseTab,
    FocusAddress,
    Reload,
    HardReload,
    Stop,
    Back,
    Forward,
    NextTab,
    PreviousTab,
    SelectTab(usize),
    LastTab,
    ToggleSidebar,
    DevTools,
    Find,
    FindNext,
    FindPrevious,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Print,
    History,
    Extensions,
    Settings,
}

/// Where events go: the app's handler.
type Sink = Rc<dyn Fn(Event)>;

/// Browsers Chromium still has open, counted by the life-span handler.
static LIVE_BROWSERS: AtomicUsize = AtomicUsize::new(0);

struct Host {
    /// Keeps the framework loaded; dropped last, after Chromium shut down.
    _loader: native::Loader,
    parent: native::View,
    container: native::View,
    browsers: HashMap<TabId, Browser>,
    active: Option<TabId>,
}

impl Host {
    /// Puts the container where the page area is, hidden while the active tab has no page
    /// or the app draws something in its place, and sizes every page to the whole area.
    fn layout(&self) {
        let (frame, overlay) = (FRAME.get(), OVERLAY.get());
        let shown = !overlay.hidden && self.active.is_some_and(|tab| self.browsers.contains_key(&tab));
        native::place(&self.container, &self.parent, frame.rect, frame.radius, frame.square_top, overlay.cover_top, shown);
        let size = (f64::from(frame.rect[2]), f64::from(frame.rect[3]));
        for browser in self.browsers.values() {
            if let Some(view) = native::view_of(browser) {
                native::fill(&view, size);
            }
        }
    }
}

/// Where the page area is in the window.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Frame {
    /// Window points from the top-left: x, y, width, height.
    pub rect: [f32; 4],
    pub radius: f32,
    /// Only the bottom corners are round (a toolbar sits flush above the page).
    pub square_top: bool,
}

/// What the app draws over the page area.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Overlay {
    /// Points at the top of the page area the app draws on (address suggestions). The page
    /// keeps its size and is clipped there, so it doesn't lay itself out again.
    pub cover_top: f32,
    /// The app draws the whole area (a load error, the history list).
    pub hidden: bool,
}

/// A permission request waiting for an answer.
enum Pending {
    Media(MediaAccessCallback, u32),
    Prompt(PermissionPromptCallback),
}

thread_local! {
    /// The tab whose page is in HTML full screen.
    static FULLSCREEN: Cell<Option<TabId>> = const { Cell::new(None) };
    static OVERLAY: Cell<Overlay> = const { Cell::new(Overlay { cover_top: 0.0, hidden: false }) };
    static PENDING: RefCell<HashMap<u64, Pending>> = RefCell::new(HashMap::new());
    static NEXT_REQUEST: Cell<u64> = const { Cell::new(1) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// Where the page area is, kept from before Chromium starts: layout reports a rect only
    /// when it changes, and the first report comes with the first frame.
    /// The colour behind a page until it paints, as 0xAARRGGBB.
    static BACKGROUND: Cell<u32> = const { Cell::new(0xFFFF_FFFF) };
    static FRAME: Cell<Frame> = const { Cell::new(Frame { rect: [0.0; 4], radius: 0.0, square_top: false }) };
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

fn emit(event: Event) {
    let sink = SINK.with_borrow(Clone::clone);
    if let Some(sink) = sink {
        sink(event);
    }
}

fn host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with_borrow_mut(|host| host.as_mut().map(f))
}

fn browser(tab: TabId) -> Option<Browser> {
    host(|host| host.browsers.get(&tab).cloned()).flatten()
}

/// Takes a closing browser's view out of the window. Losing its view is what lets Chromium
/// finish destroying the browser.
fn remove_view(browser: &Browser) {
    if let Some(view) = native::view_of(browser) {
        native::remove(&view);
    }
}

wrap_app! {
    struct BrowserApp;

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            // Only the browser process (no type) takes these.
            if process_type.is_some_and(|kind| !kind.to_string().is_empty()) {
                return;
            }
            let Some(command_line) = command_line else { return };
            // A debug build is signed anew on every build, so macOS would ask for the login
            // keychain password each time Chromium reads its cookie key. Debug builds keep
            // that key in the profile instead; release builds use the keychain.
            #[cfg(all(target_os = "macos", debug_assertions))]
            command_line.append_switch(Some(&CefString::from("use-mock-keychain")));
            // Linux has no one keychain: the cookie key stays in the profile, rather than
            // waiting on a desktop's password store that may not be there.
            #[cfg(target_os = "linux")]
            command_line.append_switch_with_value(Some(&CefString::from("password-store")), Some(&CefString::from("basic")));
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct ProcessHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            emit(Event::Ready);
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            pump::schedule(delay_ms);
        }

        // For a window Chromium opens on its own.
        fn default_client(&self) -> Option<Client> {
            Some(stray::StrayClient::new())
        }
    }
}

pub fn is_running() -> bool {
    HOST.with_borrow(Option::is_some)
}

/// Starts Chromium inside `window` and sends what it reports to `sink`; `post` runs work on
/// the main thread from any thread. Call once the window is on screen. Fails where there is no
/// Chromium to load (outside the app bundle, on macOS); the app then runs with an empty page
/// area.
pub fn start(window: &rune::WindowHandle, sink: impl Fn(Event) + 'static, post: Post) -> Result<(), String> {
    if is_running() {
        return Ok(());
    }
    let loader = native::load()?;
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let parent = native::parent(window)?;
    SINK.set(Some(Rc::new(sink)));

    let profile = crate::store::folder();
    let settings = Settings {
        external_message_pump: 1,
        root_cache_path: CefString::from(profile.to_string_lossy().as_ref()),
        cache_path: CefString::from(profile.join("Default").to_string_lossy().as_ref()),
        persist_session_cookies: 1,
        log_severity: LogSeverity::WARNING,
        ..native::settings()
    };
    let args = args::Args::new();
    let mut app = BrowserApp::new();
    if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        SINK.set(None);
        return Err("Chromium failed to initialize".into());
    }

    let container = native::container(&parent);
    HOST.set(Some(Host { _loader: loader, parent, container, browsers: HashMap::new(), active: None }));
    pump::start(post);
    Ok(())
}

/// Opens a browser for `tab` at `url`, hidden until the tab is [`activate`]d.
pub fn create_tab(tab: TabId, url: &str) {
    let Some(container) = host(|host| host.container.clone()) else { return };
    if browser(tab).is_some() {
        return;
    }
    let (width, height) = native::size(&container);
    let bounds = Rect { x: 0, y: 0, width: (width as i32).max(1), height: (height as i32).max(1) };
    let info = WindowInfo { runtime_style: RuntimeStyle::ALLOY, ..Default::default() }.set_as_child(native::handle(&container), &bounds);
    let mut client = client::TabClient::new(tab);
    let created = browser_host_create_browser_sync(
        Some(&info),
        Some(&mut client),
        Some(&CefString::from(url)),
        Some(&BrowserSettings { background_color: BACKGROUND.get(), ..Default::default() }),
        None,
        None,
    );
    let Some(created) = created else { return };
    let active = host(|host| host.active == Some(tab)).unwrap_or(false);
    if let Some(view) = native::view_of(&created) {
        native::set_hidden(&view, !active);
    }
    if !active && let Some(browser_host) = created.host() {
        browser_host.was_hidden(1);
    }
    host(|host| {
        host.browsers.insert(tab, created);
        host.layout();
    });
}

/// Shows `tab`'s page and hides the others.
pub fn activate(tab: TabId) {
    let Some(browsers) = host(|host| {
        host.active = Some(tab);
        host.layout();
        host.browsers.iter().map(|(id, browser)| (*id, browser.clone())).collect::<Vec<_>>()
    }) else {
        return;
    };
    for (id, browser) in browsers {
        let shown = id == tab;
        if let Some(view) = native::view_of(&browser) {
            native::set_hidden(&view, !shown);
        }
        if let Some(browser_host) = browser.host() {
            browser_host.was_hidden(i32::from(!shown));
        }
    }
}

pub fn close_tab(tab: TabId) {
    let Some(browser) = host(|host| {
        if host.active == Some(tab) {
            host.active = None;
        }
        host.browsers.remove(&tab)
    })
    .flatten() else {
        return;
    };
    if let Some(browser_host) = browser.host() {
        browser_host.close_browser(1);
    }
}

pub fn navigate(tab: TabId, url: &str) {
    if let Some(frame) = browser(tab).and_then(|browser| browser.main_frame()) {
        frame.load_url(Some(&CefString::from(url)));
    }
}

pub fn back(tab: TabId) {
    if let Some(browser) = browser(tab) {
        browser.go_back();
    }
}

pub fn forward(tab: TabId) {
    if let Some(browser) = browser(tab) {
        browser.go_forward();
    }
}

pub fn reload(tab: TabId, ignore_cache: bool) {
    if let Some(browser) = browser(tab) {
        if ignore_cache { browser.reload_ignore_cache() } else { browser.reload() }
    }
}

pub fn stop(tab: TabId) {
    if let Some(browser) = browser(tab) {
        browser.stop_load();
    }
}

pub fn show_dev_tools(tab: TabId) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.show_dev_tools(None, None, None, None);
    }
}

/// Says where the page area is. Kept from before Chromium starts.
pub fn set_frame(frame: Frame) {
    if FRAME.replace(frame) != frame {
        host(|host| host.layout());
    }
}

/// Says what the app draws over the page area.
pub fn set_overlay(overlay: Overlay) {
    if OVERLAY.replace(overlay) != overlay {
        host(|host| host.layout());
    }
}

/// Finds `text` in `tab`'s page: a new search, or the next (or previous) match of the same.
pub fn find(tab: TabId, text: &str, forward: bool, next: bool) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.find(Some(&CefString::from(text)), i32::from(forward), 0, i32::from(next));
    }
}

/// Ends a find, clearing its highlights.
pub fn stop_find(tab: TabId) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.stop_finding(1);
    }
}

/// Zooms `tab`'s page in (positive), out (negative) or back to its default (zero).
pub fn zoom(tab: TabId, direction: i32) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.zoom(match direction {
            0 => ZoomCommand::RESET,
            d if d > 0 => ZoomCommand::IN,
            _ => ZoomCommand::OUT,
        });
    }
}

pub fn print(tab: TabId) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.print();
    }
}

/// Notes that `tab`'s page went into HTML full screen, or left it.
fn note_fullscreen(tab: TabId, on: bool) {
    if on {
        FULLSCREEN.set(Some(tab));
    } else if FULLSCREEN.get() == Some(tab) {
        FULLSCREEN.set(None);
    }
}

fn is_fullscreen(tab: TabId) -> bool {
    FULLSCREEN.get() == Some(tab)
}

/// Takes `tab`'s page out of HTML full screen.
pub fn exit_fullscreen(tab: TabId) {
    if let Some(browser_host) = browser(tab).and_then(|browser| browser.host()) {
        browser_host.exit_fullscreen(1);
    }
}

/// Keeps a permission request until the person answers, and tells the app about it.
fn ask_permission(tab: TabId, origin: String, what: String, pending: Pending) {
    let id = NEXT_REQUEST.get();
    NEXT_REQUEST.set(id + 1);
    PENDING.with_borrow_mut(|requests| requests.insert(id, pending));
    emit(Event::Permission { id, tab, origin, what });
}

/// Answers the permission request `id`.
pub fn answer_permission(id: u64, allow: bool) {
    let Some(pending) = PENDING.with_borrow_mut(|requests| requests.remove(&id)) else { return };
    match pending {
        Pending::Media(callback, permissions) => {
            if allow { callback.cont(permissions) } else { callback.cancel() }
        }
        Pending::Prompt(callback) => callback.cont(if allow { PermissionRequestResult::ACCEPT } else { PermissionRequestResult::DENY }),
    }
}

/// Sets the colour new pages show before they paint (0xAARRGGBB), so a tab opened in a dark
/// window doesn't flash white.
pub fn set_background(argb: u32) {
    BACKGROUND.set(argb);
}

/// Whether `tab` has a browser.
pub fn has_page(tab: TabId) -> bool {
    browser(tab).is_some()
}

/// Gives the keyboard to `tab`'s page.
pub fn focus_page(tab: TabId) {
    let Some(browser) = browser(tab) else { return };
    if let Some(view) = native::view_of(&browser) {
        native::focus(&view);
    }
    if let Some(browser_host) = browser.host() {
        browser_host.set_focus(1);
    }
}

/// Takes the keyboard back from the page, for the app's own fields.
pub fn focus_app() {
    let Some((parent, active)) = host(|host| (host.parent.clone(), host.active.and_then(|tab| host.browsers.get(&tab).cloned()))) else {
        return;
    };
    if let Some(browser_host) = active.and_then(|browser| browser.host()) {
        browser_host.set_focus(0);
    }
    native::focus(&parent);
}

/// Closes every browser and shuts Chromium down, so cookies and the profile reach the disk.
/// Blocks for at most two seconds while the browsers close.
pub fn shutdown() {
    let Some(mut host) = HOST.take() else { return };
    manager::close();
    popup::close();
    for browser in host.browsers.drain().map(|(_, browser)| browser) {
        if let Some(browser_host) = browser.host() {
            browser_host.close_browser(1);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while LIVE_BROWSERS.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        // Each turn gets its own pool, so views released in it are freed in it.
        native::drain(do_message_loop_work);
        std::thread::sleep(Duration::from_millis(5));
    }
    pump::stop();
    SINK.set(None);
    native::remove(&host.container);
    if LIVE_BROWSERS.load(Ordering::SeqCst) == 0 {
        cef::shutdown();
        drop(host);
    } else {
        // Chromium aborts if it is shut down around a live browser, and unloading the
        // framework under one is no better: leave both to the process exiting.
        eprintln!("rune-browser: {} page(s) were still closing at quit; Chromium's own shutdown was skipped", LIVE_BROWSERS.load(Ordering::SeqCst));
        std::mem::forget(host);
    }
}

thread_local! {
    static DOWNLOADS: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// Says where downloads go. `~/Downloads` until said.
pub fn set_downloads_folder(folder: std::path::PathBuf) {
    DOWNLOADS.set(Some(folder));
}

fn downloads_folder() -> std::path::PathBuf {
    DOWNLOADS.with_borrow(Clone::clone).unwrap_or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Downloads"))
}

fn running() -> bool {
    HOST.with_borrow(Option::is_some)
}

/// Tells pages which colour scheme to prefer: dark, light, or (`None`) as the system is.
pub fn set_color_scheme(dark: Option<bool>) {
    if !running() {
        return;
    }
    let variant = match dark {
        None => ColorVariant::SYSTEM,
        Some(false) => ColorVariant::LIGHT,
        Some(true) => ColorVariant::DARK,
    };
    if let Some(context) = request_context_get_global_context() {
        context.set_chrome_color_scheme(variant, 0);
    }
}

/// Sets one of Chromium's own preferences, which it keeps in the profile.
fn set_preference(name: &str, set: impl FnOnce(&Value)) {
    if !running() {
        return;
    }
    let (Some(context), Some(mut value)) = (request_context_get_global_context(), value_create()) else { return };
    set(&value);
    let mut error = CefString::from("");
    if context.set_preference(Some(&CefString::from(name)), Some(&mut value), Some(&mut error)) != 1 {
        eprintln!("rune-browser: Chromium refused the preference {name}: {error}");
    }
}

/// Whether pages may read cookies of sites other than the one in the address field.
pub fn block_third_party_cookies(block: bool) {
    // 0 allows them, 1 blocks them.
    set_preference("profile.cookie_controls_mode", |value| {
        value.set_int(i32::from(block));
    });
}

/// Whether requests carry the "Do Not Track" header.
pub fn send_do_not_track(send: bool) {
    set_preference("enable_do_not_track", |value| {
        value.set_bool(i32::from(send));
    });
}

/// Deletes every cookie, which signs the person out everywhere.
pub fn clear_cookies() {
    if !running() {
        return;
    }
    if let Some(cookies) = cookie_manager_get_global_manager(None) {
        cookies.delete_cookies(None, None, None);
        cookies.flush_store(None);
    }
}

/// Empties the cache of pages, images and scripts. False when no page is open to ask it of.
pub fn clear_cache() -> bool {
    let Some(browser_host) = host(|host| host.browsers.values().next().cloned()).flatten().and_then(|browser| browser.host()) else { return false };
    browser_host.execute_dev_tools_method(0, Some(&CefString::from("Network.clearBrowserCache")), None) != 0
}

/// Puts right the dialogs Chromium opened on its own. Each turn of the loop.
fn tend_dialogs() {
    host(|host| native::tend_dialogs(&host.parent, FRAME.get().rect, manager::expects_dialog()));
}
