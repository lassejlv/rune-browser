//! Chromium's own extension manager, reached through its extensions page.
//!
//! Turning an extension off, removing it and loading one from a folder are things only
//! Chrome's `chrome://extensions` page may ask for, and an embedded Chromium shows that page
//! only in a window it draws itself. So one such window is kept, never shown: requests run in
//! its page as scripts, and what the page answers comes back as console messages.

use std::cell::RefCell;
use std::sync::atomic::Ordering;

use cef::*;

use super::{Event, LIVE_BROWSERS, emit};

const PAGE: &str = "chrome://extensions";
/// What a console message from the page starts with when it is an answer.
const MARK: &str = "rune-extensions:";

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static VIEW: RefCell<Option<BrowserView>> = const { RefCell::new(None) };
    /// Whether the page has loaded, and the scripts asked for before it had.
    static READY: RefCell<(bool, Vec<String>)> = const { RefCell::new((false, Vec::new())) };
    /// When a removal was asked for: Chromium answers it with a dialog of its own.
    static REMOVING: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Opens the hidden window. On Chromium's own turn of the loop: making one of its windows
/// from inside one of the app's handlers crashes it.
pub fn start() {
    let mut task = Start::new();
    post_task(ThreadId::UI, Some(&mut task));
}

wrap_task! {
    struct Start;

    impl Task {
        fn execute(&self) {
            if VIEW.with_borrow(Option::is_some) {
                return;
            }
            let mut client = ManagerClient::new();
            let mut delegate = ManagerViewDelegate::new();
            let view = browser_view_create(Some(&mut client), Some(&CefString::from(PAGE)), Some(&BrowserSettings::default()), None, None, Some(&mut delegate));
            let Some(view) = view else { return };
            VIEW.set(Some(view.clone()));
            let mut delegate = ManagerWindowDelegate::new(RefCell::new(Some(view)));
            WINDOW.set(window_create_top_level(Some(&mut delegate)));
        }
    }
}

/// Closes the hidden window, for quitting.
pub fn close() {
    VIEW.set(None);
    READY.set((false, Vec::new()));
    if let Some(window) = WINDOW.take() {
        window.close();
    }
}

/// Runs `script` in the extensions page, once it has loaded.
fn run(script: String) {
    let waits = READY.with_borrow_mut(|(ready, waiting)| {
        if !*ready {
            waiting.push(script.clone());
        }
        !*ready
    });
    if waits {
        return;
    }
    // Through the developer tools' protocol rather than as a plain script: it can say the
    // script acts for the person, which Chromium demands before it removes an extension.
    let Some(browser_host) = VIEW.with_borrow(Clone::clone).and_then(|view| view.browser()).and_then(|browser| browser.host()) else { return };
    let Some(mut params) = dictionary_value_create() else { return };
    params.set_string(Some(&CefString::from("expression")), Some(&CefString::from(script.as_str())));
    params.set_bool(Some(&CefString::from("userGesture")), 1);
    if browser_host.execute_dev_tools_method(0, Some(&CefString::from("Runtime.evaluate")), Some(&mut params)) == 0 {
        eprintln!("rune-browser: the extensions page did not take a request");
    }
}

/// The script that reports every extension, as [`Event::Extensions`].
const LIST: &str = r#"chrome.developerPrivate.getExtensionsInfo({includeDisabled: true, includeTerminated: true}).then(all => console.log("rune-extensions:" + JSON.stringify(all.map(e => ({id: e.id, name: e.name, version: e.version, enabled: e.state === "ENABLED", icon: e.iconUrl, options: e.optionsPage ? e.optionsPage.url : null, unpacked: e.location === "UNPACKED", removable: !!e.userMayModify, path: e.path || null})))));"#;

/// Asks for the list of extensions.
pub fn list() {
    run(LIST.to_owned());
}

fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// Turns the extension `id` on or off, then reports the list.
pub fn set_enabled(id: &str, enabled: bool) {
    run(format!("chrome.management.setEnabled({}, {enabled}).then(() => {{ {LIST} }}, error => console.log('rune-extensions-error:' + error));", quoted(id)));
}

/// Whether a dialog from the extensions page is due. Its window is hidden, so the dialog
/// would be too.
pub fn expects_dialog() -> bool {
    REMOVING.get().is_some_and(|since| since.elapsed().as_secs() < 5)
}

/// Removes the extension `id` once the person agrees in the dialog Chromium shows for it,
/// then reports the list.
pub fn remove(id: &str) {
    REMOVING.set(Some(std::time::Instant::now()));
    run(format!("chrome.management.uninstall({}, {{showConfirmDialog: false}}).then(() => {{ {LIST} }}, error => console.log('rune-extensions-error:' + error));", quoted(id)));
}

wrap_browser_view_delegate! {
    struct ManagerViewDelegate {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }

        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::NONE
        }
    }
}

wrap_window_delegate! {
    struct ManagerWindowDelegate {
        view: RefCell<Option<BrowserView>>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 900, height: 700 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        // Never shown: the window is only a place for the page to live.
        fn on_window_created(&self, window: Option<&mut Window>) {
            let view = self.view.borrow();
            let (Some(window), Some(view)) = (window, view.as_ref()) else { return };
            let mut view = View::from(view);
            window.add_child_view(Some(&mut view));
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            ShowState::HIDDEN
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.view.replace(None);
            VIEW.set(None);
            WINDOW.set(None);
        }

        fn can_close(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            match self.view.borrow().as_ref().and_then(|view| view.browser()).and_then(|browser| browser.host()) {
                Some(host) => host.try_close_browser(),
                None => 1,
            }
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

wrap_client! {
    struct ManagerClient;

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(ManagerLife::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(ManagerDisplay::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(ManagerLoad::new())
        }
    }
}

wrap_life_span_handler! {
    struct ManagerLife;

    impl LifeSpanHandler {
        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_add(1, Ordering::SeqCst);
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

wrap_load_handler! {
    struct ManagerLoad;

    impl LoadHandler {
        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status_code: ::std::os::raw::c_int) {
            if frame.is_none_or(|frame| frame.is_main() != 1) {
                return;
            }
            let waiting = READY.with_borrow_mut(|(ready, waiting)| {
                *ready = true;
                std::mem::take(waiting)
            });
            // The list now, and again whenever Chromium installs, removes or changes one.
            run(format!("chrome.developerPrivate.onItemStateChanged.addListener(() => {{ {LIST} }}); {LIST}"));
            waiting.into_iter().for_each(run);
        }
    }
}

wrap_display_handler! {
    struct ManagerDisplay;

    impl DisplayHandler {
        fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity, message: Option<&CefString>, _source: Option<&CefString>, _line: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let message = message.map(CefString::to_string).unwrap_or_default();
            if let Some(json) = message.strip_prefix(MARK) {
                emit(Event::Extensions(json.to_owned()));
            } else if crate::state::logging() {
                eprintln!("rune-browser: extensions page: {message}");
            }
            1
        }
    }
}
