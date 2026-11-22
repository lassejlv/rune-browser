//! What Chromium reports about one tab, turned into [`Event`]s.

use std::sync::atomic::Ordering;

use cef::*;

use super::{Command, Event, LIVE_BROWSERS, TabId, emit};

fn text(value: Option<&CefString>) -> String {
    value.map(CefString::to_string).unwrap_or_default()
}

/// The strings of a list Chromium lent for the length of a callback. Read through the
/// list's own pointer: the wrapper's `Clone` and `IntoIterator` copy the opaque list struct
/// by value and so read an empty list.
fn strings(list: &mut CefStringList) -> Vec<String> {
    let raw: *mut sys::_cef_string_list_t = list.into();
    if raw.is_null() {
        return Vec::new();
    }
    // SAFETY: `raw` is the live list Chromium passed; each value is copied out before it
    // is read again.
    unsafe {
        (0..sys::cef_string_list_size(raw))
            .filter_map(|index| {
                let mut value: sys::cef_string_t = std::mem::zeroed();
                (sys::cef_string_list_value(raw, index, &mut value) > 0).then(|| CefString::from(std::ptr::from_ref(&value)).to_string())
            })
            .collect()
    }
}

wrap_client! {
    pub struct TabClient {
        tab: TabId,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(TabDisplay::new(self.tab))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(TabLoad::new(self.tab))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(TabLife::new(self.tab))
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            Some(TabKeys::new(self.tab))
        }

        fn focus_handler(&self) -> Option<FocusHandler> {
            Some(TabFocus::new())
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(TabDownloads::new())
        }

        fn find_handler(&self) -> Option<FindHandler> {
            Some(TabFind::new(self.tab))
        }

        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(TabPermissions::new(self.tab))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(TabRequests::new(self.tab))
        }
    }
}

wrap_display_handler! {
    struct TabDisplay {
        tab: TabId,
    }

    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.is_some_and(|frame| frame.is_main() != 0) {
                emit(Event::Address(self.tab, text(url)));
            }
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            emit(Event::Title(self.tab, text(title)));
        }

        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: ::std::os::raw::c_int) {
            super::note_fullscreen(self.tab, fullscreen != 0);
            emit(Event::Fullscreen(self.tab, fullscreen != 0));
        }

        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            let Some(url) = icon_urls.and_then(|urls| strings(urls).into_iter().next()) else { return };
            let Some(browser_host) = browser.and_then(|browser| browser.host()) else { return };
            let mut callback = FaviconDownloaded::new(self.tab);
            // Up to 64 px, so a 16 pt favicon is sharp on a Retina display.
            browser_host.download_image(Some(&CefString::from(url.as_str())), 1, 64, 0, Some(&mut callback));
        }
    }
}

wrap_download_image_callback! {
    struct FaviconDownloaded {
        tab: TabId,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(&self, _image_url: Option<&CefString>, _http_status_code: ::std::os::raw::c_int, image: Option<&mut Image>) {
            let Some(image) = image else { return };
            // Chromium writes the size through both pointers, so both must be given.
            let (mut width, mut height) = (0, 0);
            let Some(png) = image.as_png(1.0, 1, Some(&mut width), Some(&mut height)) else { return };
            let mut bytes = vec![0u8; png.size()];
            let written = png.data(Some(&mut bytes), 0);
            bytes.truncate(written);
            if !bytes.is_empty() {
                emit(Event::Favicon(self.tab, bytes));
            }
        }
    }
}

wrap_find_handler! {
    struct TabFind {
        tab: TabId,
    }

    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: ::std::os::raw::c_int,
            _final_update: ::std::os::raw::c_int,
        ) {
            // Chromium reports the count and the selected match in separate updates, each
            // leaving the other at -1 or 0; the app keeps what it already knows.
            emit(Event::FindResult { tab: self.tab, count, active: active_match_ordinal });
        }
    }
}

/// What a permission request asks for, in words: "use your camera and microphone".
fn describe(camera: bool, microphone: bool, screen: bool, location: bool, notifications: bool, clipboard: bool) -> Option<String> {
    let mut parts = Vec::new();
    match (camera, microphone) {
        (true, true) => parts.push("use your camera and microphone"),
        (true, false) => parts.push("use your camera"),
        (false, true) => parts.push("use your microphone"),
        (false, false) => {}
    }
    if screen {
        parts.push("record your screen");
    }
    if location {
        parts.push("know your location");
    }
    if notifications {
        parts.push("send you notifications");
    }
    if clipboard {
        parts.push("read your clipboard");
    }
    (!parts.is_empty()).then(|| parts.join(" and "))
}

wrap_permission_handler! {
    struct TabPermissions {
        tab: TabId,
    }

    impl PermissionHandler {
        // Camera, microphone and screen capture, asked for by getUserMedia.
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let asks = |bit: u32| requested_permissions & bit != 0;
            let Some(what) = describe(asks(1 << 1), asks(1 << 0), asks(1 << 2) || asks(1 << 3), false, false, false) else { return 0 };
            super::ask_permission(self.tab, text(requesting_origin), what, super::Pending::Media(callback.clone(), requested_permissions));
            1
        }

        // Everything else a page can ask for. What isn't listed here keeps Chromium's
        // default for an embedded page, which is to ignore the request.
        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            _prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let asks = |bit: u32| requested_permissions & bit != 0;
            let Some(what) = describe(asks(1 << 2), asks(1 << 12), false, asks(1 << 8), asks(1 << 15), asks(1 << 4)) else { return 0 };
            super::ask_permission(self.tab, text(requesting_origin), what, super::Pending::Prompt(callback.clone()));
            1
        }
    }
}

wrap_request_handler! {
    struct TabRequests {
        tab: TabId,
    }

    impl RequestHandler {
        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: ::std::os::raw::c_int,
            _error_string: Option<&CefString>,
        ) {
            emit(Event::Crashed(self.tab));
        }
    }
}

wrap_focus_handler! {
    struct TabFocus;

    impl FocusHandler {
        fn on_got_focus(&self, _browser: Option<&mut Browser>) {
            emit(Event::PageFocused);
        }
    }
}

/// A free path in the downloads folder for `name`: "file.zip", then "file (1).zip", …
fn download_path(name: &str) -> std::path::PathBuf {
    let folder = super::downloads_folder();
    let _ = std::fs::create_dir_all(&folder);
    let name = std::path::Path::new(name).file_name().map(|name| name.to_string_lossy().into_owned()).filter(|name| !name.is_empty()).unwrap_or_else(|| "download".into());
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_owned(), format!(".{extension}")),
        _ => (name.clone(), String::new()),
    };
    (0..)
        .map(|n| if n == 0 { folder.join(&name) } else { folder.join(format!("{stem} ({n}){extension}")) })
        .find(|path| !path.exists())
        .unwrap_or_else(|| folder.join(&name))
}

wrap_download_handler! {
    struct TabDownloads;

    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            1
        }

        // Downloads go straight to the Downloads folder, as Safari's do.
        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let path = download_path(&text(suggested_name));
            callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
            if let Some(item) = download_item {
                emit(Event::Download { id: item.id(), done: false });
            }
            1
        }

        fn on_download_updated(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, _callback: Option<&mut DownloadItemCallback>) {
            let Some(item) = download_item else { return };
            if item.is_complete() != 0 || item.is_canceled() != 0 || item.is_interrupted() != 0 {
                emit(Event::Download { id: item.id(), done: true });
            }
        }
    }
}

wrap_load_handler! {
    struct TabLoad {
        tab: TabId,
    }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            emit(Event::Loading {
                tab: self.tab,
                loading: is_loading != 0,
                can_go_back: can_go_back != 0,
                can_go_forward: can_go_forward != 0,
            });
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            // A load the person stopped, or one replaced by a download, is not a failure.
            let aborted = sys::cef_errorcode_t::from(error_code) == sys::cef_errorcode_t::ERR_ABORTED;
            if !aborted && frame.is_some_and(|frame| frame.is_main() != 0) {
                emit(Event::LoadFailed { tab: self.tab, url: text(failed_url), error: text(error_text) });
            }
        }
    }
}

wrap_life_span_handler! {
    struct TabLife {
        tab: TabId,
    }

    impl LifeSpanHandler {
        // A page asking for a new window (target=_blank, window.open, a middle click) gets
        // a tab instead: the popup is cancelled and the app opens its URL.
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let url = text(target_url);
            if !url.is_empty() {
                emit(Event::OpenTab {
                    opener: self.tab,
                    url,
                    background: target_disposition == WindowOpenDisposition::NEW_BACKGROUND_TAB,
                });
            }
            1
        }

        // Chromium's default close asks the window that holds the page's view to close,
        // which here is the whole browser window. A tab instead closes by taking its view
        // out of the window: with the view gone, Chromium destroys the browser.
        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            if let Some(browser) = browser {
                let mut task = RemoveView::new(browser.clone());
                post_task(ThreadId::UI, Some(&mut task));
            }
            1
        }

        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_add(1, Ordering::SeqCst);
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_sub(1, Ordering::SeqCst);
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

const SHIFT: u32 = 1 << 1;
const CONTROL: u32 = 1 << 2;
const ALT: u32 = 1 << 3;
const COMMAND: u32 = 1 << 7;
/// The modifier the app's shortcuts are held with: ⌘ on macOS, Ctrl elsewhere.
pub const PRIMARY: u32 = if cfg!(target_os = "macos") { COMMAND } else { CONTROL };

/// The app's shortcut for a key pressed while the page has the keyboard.
fn shortcut(modifiers: u32, key: i32) -> Option<Command> {
    let (shift, ctrl, alt) = (modifiers & SHIFT != 0, modifiers & CONTROL != 0, modifiers & ALT != 0);
    let key = u8::try_from(key).ok()?;
    if ctrl && modifiers & COMMAND == 0 && !alt && key == 0x09 {
        return Some(if shift { Command::PreviousTab } else { Command::NextTab });
    }
    // Back and forward: ⌘← and ⌘→ on macOS, Alt+← and Alt+→ elsewhere (Ctrl+← moves by word).
    let mac = cfg!(target_os = "macos");
    if !mac && alt && !ctrl && !shift && (key == 0x25 || key == 0x27) {
        return Some(if key == 0x25 { Command::Back } else { Command::Forward });
    }
    if modifiers & PRIMARY == 0 || (mac && ctrl) {
        return None;
    }
    match (key, shift, alt) {
        (b'T', false, false) => Some(Command::NewTab),
        (b'W', false, false) => Some(Command::CloseTab),
        (b'L', false, false) => Some(Command::FocusAddress),
        (b'R', false, false) => Some(Command::Reload),
        (b'R', true, false) => Some(Command::HardReload),
        (b'S', false, false) => Some(Command::ToggleSidebar),
        (0xDB, false, false) => Some(Command::Back),
        (0xDD, false, false) => Some(Command::Forward),
        (0xDB, true, false) => Some(Command::PreviousTab),
        (0xDD, true, false) => Some(Command::NextTab),
        (0x25, false, false) if mac => Some(Command::Back),
        (0x27, false, false) if mac => Some(Command::Forward),
        (0x25, false, true) if mac => Some(Command::PreviousTab),
        (0x27, false, true) if mac => Some(Command::NextTab),
        (b'1'..=b'8', false, false) => Some(Command::SelectTab(usize::from(key - b'1'))),
        (b'9', false, false) => Some(Command::LastTab),
        (b'I', false, true) => Some(Command::DevTools),
        (b'F', false, false) => Some(Command::Find),
        (b'G', false, false) => Some(Command::FindNext),
        (b'G', true, false) => Some(Command::FindPrevious),
        (b'P', false, false) => Some(Command::Print),
        (b'Y', false, false) => Some(Command::History),
        // The , key.
        (0xBC, false, false) => Some(Command::Settings),
        // The = / + key and the - key.
        (0xBB, _, false) => Some(Command::ZoomIn),
        (0xBD, false, false) => Some(Command::ZoomOut),
        (b'0', false, false) => Some(Command::ZoomReset),
        _ => None,
    }
}

wrap_keyboard_handler! {
    struct TabKeys {
        tab: TabId,
    }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: super::native::OsEvent<'_>,
            _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let Some(event) = event else { return 0 };
            if event.type_ != KeyEventType::RAWKEYDOWN {
                return 0;
            }
            // Escape leaves full screen. Chromium's own interface does this in Chrome; an
            // embedded page has only the app to do it.
            if event.windows_key_code == 0x1B && super::is_fullscreen(self.tab) {
                super::exit_fullscreen(self.tab);
                return 1;
            }
            match shortcut(event.modifiers, event.windows_key_code) {
                Some(command) => {
                    emit(Event::Command(command));
                    1
                }
                None => 0,
            }
        }
    }
}
