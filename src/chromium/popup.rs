//! An extension's popup: its page in a small window of its own, hung under the toolbar.
//!
//! Chrome draws these from its toolbar, which an embedded Chromium doesn't have. Here the
//! popup's page is an ordinary embedded browser inside a borderless-looking window that
//! takes the size the page asks for and closes when it loses the keyboard, as Chrome's does.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::Ordering;

use cef::*;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::{BACKGROUND, Event, FRAME, LIVE_BROWSERS, emit, host, view_of};

/// The gap between the popup and the edges of the page area it hangs in.
const MARGIN: f64 = 8.0;
/// The size a popup has until its page says how big it is, and the largest Chrome allows.
const FIRST: (f64, f64) = (320.0, 240.0);
const LARGEST: Size = Size { width: 800, height: 600 };
/// A page narrower than this has no width of its own: it fills whatever it is given, and
/// expects Chrome to have given it a popup (Bitwarden's does). It gets `STANDARD` instead.
const NARROWEST: i32 = 120;
const STANDARD: (f64, f64) = (380.0, 600.0);

struct Popup {
    window: Retained<NSWindow>,
    browser: Option<Browser>,
    url: String,
    /// The screen point its top corner is held to, and whether that is its right corner.
    corner: NSPoint,
    right: bool,
    /// It has had the keyboard: losing it from then on closes it.
    was_key: bool,
    /// Its page has no size of its own, so the window keeps `STANDARD`.
    fixed: bool,
    /// Whether its page has loaded. Until it has, the sizes it asks for are those of a page
    /// still putting itself together, and the window stays unseen.
    loaded: bool,
    shown: bool,
    opened: std::time::Instant,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// Windows of popups that are closing, kept until their browsers are gone.
    static CLOSING: RefCell<Vec<Retained<NSWindow>>> = const { RefCell::new(Vec::new()) };
}

fn frame_for(corner: NSPoint, right: bool, (width, height): (f64, f64)) -> NSRect {
    let x = if right { corner.x - width } else { corner.x };
    NSRect::new(NSPoint::new(x, corner.y - height), NSSize::new(width, height))
}

/// The tab a popup is opened over, which it is told about (see `popup_shim.js`).
pub struct Over {
    pub tab: super::TabId,
    pub url: String,
    pub title: String,
}

/// The ids the popup is given for the app's tabs and for its window. Chromium's own tab ids
/// count up from a random start, so these are kept well clear of small numbers and of each
/// other, not of Chromium's: the popup's calls with them never reach Chromium.
const TAB_IDS: i64 = 1_900_000_000;
const WINDOW_ID: i64 = 1_899_999_999;

/// The script that tells a popup about `over`, with its values put in as JSON.
fn shim(over: &Over, (width, height): (f32, f32)) -> String {
    let tab = serde_json::json!({"id": TAB_IDS + over.tab as i64, "windowId": WINDOW_ID, "url": over.url, "title": over.title, "width": width, "height": height});
    include_str!("popup_shim.js").replace("__TAB__", &tab.to_string())
}

/// What a popup asked for the tab it is over, from a console message of the shim's.
fn asked(message: &str) -> Option<Event> {
    let request: serde_json::Value = serde_json::from_str(message.strip_prefix("rune-popup:")?).ok()?;
    match request.get("do")?.as_str()? {
        "reload" => Some(Event::Command(if request.get("hard")?.as_bool()? { super::Command::HardReload } else { super::Command::Reload })),
        // As Chrome does for an extension's `tabs.update`: pages and extension pages only.
        "go" => request.get("url")?.as_str().filter(|url| ["https://", "http://", "chrome-extension://"].iter().any(|scheme| url.starts_with(scheme))).map(|url| Event::Navigate(url.to_owned())),
        _ => None,
    }
}

/// Opens the popup page `url` at the top of the page area: at its right end when `right`,
/// else its left. `over` is the tab on screen, when it is a web page. Opening the popup
/// already open closes it, as pressing its button again does.
pub fn open(url: &str, right: bool, over: Option<Over>) {
    let same = POPUP.with_borrow(|popup| popup.as_ref().is_some_and(|popup| popup.url == url));
    close();
    if same {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(parent) = host(|host| host.parent.window()).flatten() else { return };
    let [x, y, w, h] = FRAME.get().rect;
    let home = parent.frame();
    let corner = NSPoint::new(home.origin.x + f64::from(x) + if right { f64::from(w) - MARGIN } else { MARGIN }, home.origin.y + home.size.height - f64::from(y) - MARGIN);
    let frame = frame_for(corner, right, FIRST);
    // Titled, so it may take the keyboard and has rounded corners, with the title bar clear
    // and the page drawn under it.
    const TITLED_FULL_SIZE: usize = 1 | 1 << 15;
    const BUFFERED: usize = 2;
    // SAFETY: plain AppKit calls on the main thread; the window is released by `Retained`,
    // not by closing.
    let window: Retained<NSWindow> = unsafe {
        let window: Retained<NSWindow> = msg_send![mtm.alloc::<NSWindow>(), initWithContentRect: frame, styleMask: TITLED_FULL_SIZE, backing: BUFFERED, defer: false];
        let _: () = msg_send![&*window, setReleasedWhenClosed: false];
        let _: () = msg_send![&*window, setTitlebarAppearsTransparent: true];
        let _: () = msg_send![&*window, setTitleVisibility: 1isize];
        let _: () = msg_send![&*window, setMovable: false];
        let _: () = msg_send![&*window, setExcludedFromWindowsMenu: true];
        let _: () = msg_send![&*window, setAnimationBehavior: 2isize];
        let _: () = msg_send![&*window, setAlphaValue: 0.0f64];
        window
    };
    let Some(content) = window.contentView() else { return };
    let bounds = Rect { x: 0, y: 0, width: FIRST.0 as i32, height: FIRST.1 as i32 };
    let info = WindowInfo { runtime_style: RuntimeStyle::ALLOY, ..Default::default() }.set_as_child(Retained::as_ptr(&content).cast_mut().cast::<c_void>(), &bounds);
    let mut client = PopupClient::new();
    // Blank first: the script that tells the popup its tab must be in place before the
    // popup's page starts to load.
    let first = if over.is_some() { "about:blank" } else { url };
    let browser = browser_host_create_browser_sync(Some(&info), Some(&mut client), Some(&CefString::from(first)), Some(&BrowserSettings { background_color: BACKGROUND.get(), ..Default::default() }), None, None);
    if let (Some(browser), Some(over)) = (&browser, &over) {
        if let (Some(browser_host), Some(mut params)) = (browser.host(), dictionary_value_create()) {
            params.set_string(Some(&CefString::from("source")), Some(&CefString::from(shim(over, (w, h)).as_str())));
            browser_host.execute_dev_tools_method(0, Some(&CefString::from("Page.enable")), None);
            browser_host.execute_dev_tools_method(0, Some(&CefString::from("Page.addScriptToEvaluateOnNewDocument")), Some(&mut params));
        }
        if let Some(frame) = browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }
    if let Some(browser) = &browser {
        if let Some(view) = view_of(browser) {
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            view.setFrame(content.bounds());
        }
        if let Some(browser_host) = browser.host() {
            browser_host.set_auto_resize_enabled(1, Some(&Size { width: 25, height: 25 }), Some(&LARGEST));
        }
    }
    // SAFETY: as above.
    unsafe {
        let _: () = msg_send![&*parent, addChildWindow: &*window, ordered: 1isize];
        let _: () = msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
    if let Some(browser) = &browser {
        if let Some(view) = view_of(browser) {
            window.makeFirstResponder(Some(&view));
        }
        if let Some(browser_host) = browser.host() {
            browser_host.set_focus(1);
        }
    }
    POPUP.set(Some(Popup { window, browser, url: url.to_owned(), corner, right, was_key: false, fixed: false, loaded: false, shown: false, opened: std::time::Instant::now() }));
}

/// Closes the popup, if one is open.
pub fn close() {
    let Some(popup) = POPUP.take() else { return };
    // SAFETY: plain AppKit calls on the main thread.
    unsafe {
        let parent: *mut AnyObject = msg_send![&*popup.window, parentWindow];
        if !parent.is_null() {
            let _: () = msg_send![parent, removeChildWindow: &*popup.window];
        }
        let _: () = msg_send![&*popup.window, orderOut: std::ptr::null::<AnyObject>()];
    }
    match popup.browser.as_ref().and_then(|browser| browser.host()) {
        // The window outlives this call: the browser's view is in it until Chromium is done.
        Some(browser_host) => {
            CLOSING.with_borrow_mut(|closing| closing.push(popup.window));
            browser_host.close_browser(1);
        }
        None => popup.window.close(),
    }
}

/// Each turn of the loop: the popup closes once it loses the keyboard, and one whose page
/// is slow to load or never says its size is shown anyway.
pub(super) fn tend() {
    let (lost, overdue) = POPUP.with_borrow_mut(|popup| {
        let Some(popup) = popup else { return (false, false) };
        let key = popup.window.isKeyWindow();
        popup.was_key |= key;
        let overdue = !popup.shown && popup.opened.elapsed().as_millis() > 1500;
        popup.loaded |= overdue;
        (popup.was_key && !key, overdue)
    });
    if lost {
        close();
    } else if overdue {
        // No size of its own: the standard one.
        resize(&Size { width: 0, height: 0 });
    }
}

/// The page asked for `size`: the window takes it, held by its top corner.
fn resize(size: &Size) {
    POPUP.with_borrow_mut(|popup| {
        let Some(popup) = popup else { return };
        if popup.fixed || !popup.loaded {
            return;
        }
        let size = if size.width < NARROWEST {
            popup.fixed = true;
            // From now on the page fills the window rather than the window fitting the page.
            let mut task = StopFitting::new();
            post_task(ThreadId::UI, Some(&mut task));
            STANDARD
        } else {
            (f64::from(size.width.min(LARGEST.width)), f64::from(size.height.clamp(25, LARGEST.height)))
        };
        popup.window.setFrame_display(frame_for(popup.corner, popup.right, size), true);
        popup.window.setAlphaValue(1.0);
        popup.shown = true;
    });
}

wrap_task! {
    struct StopFitting;

    impl Task {
        fn execute(&self) {
            let Some((browser, window)) = POPUP.with_borrow(|popup| popup.as_ref().and_then(|popup| Some((popup.browser.clone()?, popup.window.clone())))) else { return };
            let Some(browser_host) = browser.host() else { return };
            // The sizes are required even to turn it off: without them the call is dropped.
            browser_host.set_auto_resize_enabled(0, Some(&Size { width: 0, height: 0 }), Some(&Size { width: 0, height: 0 }));
            // While it fitted itself, the page's own view (inside the browser's) took the
            // page's size, and it is still that narrow. Give it the browser view's size.
            if let (Some(view), Some(content)) = (view_of(&browser), window.contentView()) {
                view.setFrame(content.bounds());
                let bounds = view.bounds();
                for inner in view.subviews().iter() {
                    inner.setFrame(bounds);
                }
            }
            browser_host.was_resized();
        }
    }
}

wrap_task! {
    struct Close;

    impl Task {
        fn execute(&self) {
            close();
        }
    }
}

/// Closes the popup after the callback asking for it has returned.
fn close_soon() {
    let mut task = Close::new();
    post_task(ThreadId::UI, Some(&mut task));
}

wrap_client! {
    struct PopupClient;

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(PopupLife::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(PopupDisplay::new())
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(PopupKeys::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(PopupLoad::new())
        }
    }
}

wrap_load_handler! {
    struct PopupLoad;

    impl LoadHandler {
        fn on_loading_state_change(&self, browser: Option<&mut Browser>, is_loading: ::std::os::raw::c_int, _can_go_back: ::std::os::raw::c_int, _can_go_forward: ::std::os::raw::c_int) {
            // The blank page a popup starts on doesn't count.
            let blank = browser.and_then(|browser| browser.main_frame()).is_none_or(|frame| CefString::from(&frame.url()).to_string().starts_with("about:"));
            if is_loading == 0 && !blank {
                // The next size the page asks for is its own.
                POPUP.with_borrow_mut(|popup| {
                    if let Some(popup) = popup {
                        popup.loaded = true;
                    }
                });
            }
        }
    }
}

wrap_life_span_handler! {
    struct PopupLife;

    impl LifeSpanHandler {
        // A link out of the popup opens as a tab, and the popup has done its job.
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let url = target_url.map(CefString::to_string).unwrap_or_default();
            if !url.is_empty() {
                emit(Event::OpenTab { opener: 0, url, background: false });
                close_soon();
            }
            1
        }

        // The page closed itself (`window.close()`), or `close` asked: either way the view
        // comes out of its window, which is what lets Chromium finish.
        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            if let Some(browser) = browser {
                let ours = POPUP.with_borrow(|popup| popup.as_ref().and_then(|popup| popup.browser.as_ref()).is_some_and(|open| open.identifier() == browser.identifier()));
                if ours {
                    close_soon();
                } else {
                    let mut task = RemoveView::new(browser.clone());
                    post_task(ThreadId::UI, Some(&mut task));
                }
            }
            1
        }

        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_add(1, Ordering::SeqCst);
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_sub(1, Ordering::SeqCst);
            for window in CLOSING.take() {
                window.close();
            }
        }
    }
}

wrap_task! {
    struct RemoveView {
        browser: Browser,
    }

    impl Task {
        fn execute(&self) {
            super::remove_view(&self.browser);
        }
    }
}

wrap_display_handler! {
    struct PopupDisplay;

    impl DisplayHandler {
        fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity, message: Option<&CefString>, _source: Option<&CefString>, _line: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let message = message.map(CefString::to_string).unwrap_or_default();
            match asked(&message) {
                Some(event) => {
                    emit(event);
                    1
                }
                None => 0,
            }
        }

        fn on_auto_resize(&self, _browser: Option<&mut Browser>, new_size: Option<&Size>) -> ::std::os::raw::c_int {
            if let Some(size) = new_size {
                resize(size);
            }
            1
        }
    }
}

wrap_keyboard_handler! {
    struct PopupKeys;

    impl KeyboardHandler {
        fn on_pre_key_event(&self, _browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: *mut u8, _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            const COMMAND: u32 = 1 << 7;
            let Some(event) = event else { return 0 };
            // Escape and ⌘W close it.
            let closes = event.windows_key_code == 0x1B || (event.modifiers & COMMAND != 0 && event.windows_key_code == i32::from(b'W'));
            if event.type_ != KeyEventType::RAWKEYDOWN || !closes {
                return 0;
            }
            close_soon();
            1
        }
    }
}
