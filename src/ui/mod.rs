//! The browser's own interface, drawn with Rune around the page Chromium draws.

mod icons;
mod parts;
mod sidebar;
mod theme;
mod top_tabs;

use rune::{Cx, El, Titlebar, row};

use crate::chromium::Overlay;
use crate::state::{Layout, State};

/// The page panel's corner radius, which the Chromium view over it shares.
pub const PAGE_RADIUS: f32 = 12.0;

/// Where the macOS window buttons sit in each layout: beside the sidebar's controls, and in
/// the middle of the tab strip.
pub const SIDEBAR_TITLEBAR: Titlebar = Titlebar::new(46.0).traffic_lights(18.0, 24.0);
const TOP_TITLEBAR: Titlebar = Titlebar::new(top_tabs::STRIP_HEIGHT).traffic_lights(18.0, 15.0);

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = s.settings.theme.dark(cx.system_dark());
    let p = theme::palette(dark, s.space);
    crate::chromium::set_background(theme::page_argb(dark));
    // A page in full screen (a video) is all there is: no tabs, no toolbar, no rounding.
    if s.fullscreen.is_some() && s.fullscreen == s.active_id() {
        crate::chromium::set_overlay(Overlay::default());
        return row().w_full().h_full().fill(rune::Color::BLACK).child(parts::page_area(s, &p, 0.0, true, None));
    }
    // In the top layout the address field's suggestions drop over the page.
    let cover_top = if s.layout == Layout::TopTabs { parts::suggestions_height(s) } else { 0.0 };
    crate::chromium::set_overlay(Overlay { cover_top, hidden: parts::draws_page(s) });
    match s.layout {
        Layout::Sidebar => {
            cx.set_titlebar(SIDEBAR_TITLEBAR);
            sidebar::view(s, &p)
        }
        Layout::TopTabs => {
            cx.set_titlebar(TOP_TITLEBAR);
            top_tabs::view(s, &p)
        }
    }
}
