//! Windows Chromium opens on its own.
//!
//! An extension that opens a tab (its welcome page after installing, `chrome.tabs.create`)
//! has no Chrome window to put it in, so Chromium makes one, with Chrome's own tab strip and
//! toolbar. Such a window is kept invisible, its address becomes a tab in the browser window,
//! and it is closed.

use std::sync::atomic::Ordering;

use cef::*;

use super::{Command, Event, LIVE_BROWSERS, emit, native};

/// Sets the opacity of the window `browser`'s page is in.
fn set_alpha(browser: &Browser, alpha: f64) {
    if let Some(view) = native::view_of(browser) {
        native::set_window_alpha(&view, alpha);
    }
}

wrap_client! {
    pub struct StrayClient;

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(StrayLife::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(StrayDisplay::new())
        }
    }
}

wrap_life_span_handler! {
    struct StrayLife;

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_add(1, Ordering::SeqCst);
            if let Some(browser) = browser {
                set_alpha(browser, 0.0);
            }
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            LIVE_BROWSERS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

wrap_display_handler! {
    struct StrayDisplay;

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            let url = url.map(CefString::to_string).unwrap_or_default();
            if frame.is_main() != 1 || url.is_empty() || url == "about:blank" {
                return;
            }
            if crate::state::logging() {
                eprintln!("rune-browser: Chromium opened a window of its own for {url}");
            }
            // The developer tools are a window of Chromium's that is meant to be one.
            if url.starts_with("devtools://") {
                return set_alpha(browser, 1.0);
            }
            // Chrome's settings and extensions pages have the app's own in their place.
            if url.starts_with("chrome://settings") {
                emit(Event::Command(Command::Settings));
            } else if url.starts_with("chrome://extensions") {
                emit(Event::Command(Command::Extensions));
            } else {
                emit(Event::OpenTab { opener: 0, url, background: false });
            }
            if let Some(host) = browser.host() {
                host.close_browser(1);
            }
        }
    }
}
