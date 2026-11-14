//! Tabs along the top: one strip holding the window buttons, pinned tabs as icons, the
//! space's tabs and the space switcher, over a panel with a toolbar and the page.

use rune::{El, Phase, col, el, icon, row, text};

use super::parts::{self, BODY, BODY_MEDIUM, LABEL};
use super::theme::Palette;
use super::{PAGE_RADIUS, icons};
use crate::chromium::Command;
use crate::state::{State, Tab};

pub const STRIP_HEIGHT: f32 = 46.0;

pub fn view(s: &State, p: &Palette) -> El {
    let panel = col()
        .key("page-panel")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .radius(PAGE_RADIUS)
        .fill(p.page)
        .border(0.5, p.page_ring)
        .shadow(1.0, 3.0, p.page_shadow)
        .child(toolbar(s, p))
        .child(el().w_full().h(1.0).fill(p.toolbar_border))
        .children(bands(s, p))
        .child(parts::page_area(s, p, PAGE_RADIUS, true, suggestions(s, p)));
    col().w_full().h_full().fill(p.window).px(8.0).pb(8.0).child(strip(s, p)).child(panel)
}

/// The find bar and a site's request, each on a band of its own between toolbar and page.
fn bands(s: &State, p: &Palette) -> Vec<El> {
    let band = |bar: El| col().w_full().py(6.0).items_center().child(bar.w(520.0).shrink().min_w(200.0)).child(el());
    [parts::find_bar(s, p, p.toolbar_field), parts::prompt_bar(s, p, p.toolbar_field)]
        .into_iter()
        .flatten()
        .flat_map(|bar| [band(bar), el().w_full().h(1.0).fill(p.toolbar_border)])
        .collect()
}

/// The address field's suggestions, in a sheet across the top of the page, under the field.
fn suggestions(s: &State, p: &Palette) -> Option<El> {
    let list = parts::suggestions(s, p, p.toolbar_field)?;
    Some(row().key("suggestion-sheet").w_full().justify_center().fill(p.page).child(list.w(520.0).shrink().min_w(200.0)))
}

fn strip(s: &State, p: &Palette) -> El {
    let active = s.active_id();
    // The macOS window buttons take the strip's first 52 points, and 14 of air after them.
    let mut strip = row().key("strip").h(STRIP_HEIGHT).pl(10.0).gap(6.0).center_y().window_drag().child(el().w(66.0));
    if s.pinned().next().is_some() {
        let mut pinned = row().key("pinned").gap(4.0).center_y();
        for tab in s.pinned() {
            let id = tab.id;
            let tile = el()
                .key(format!("pin-{id}"))
                .role("tab")
                .label(tab.label())
                .help(tab.label())
                .size(34.0, 30.0)
                .radius(8.0)
                .center()
                .child(parts::favicon(tab, 16.0, p))
                .on_press_tx(move |s: &mut State, tx| s.select(id, tx))
                .on_secondary_press(move |s: &mut State, press, tx| parts::tab_menu(s, id, press, tx));
            pinned = pinned.child(if active == Some(id) { selected(tile, p) } else { tile.hover(p.hover) });
        }
        strip = strip.child(pinned.child(el().size(1.0, 16.0).fill(p.divider)));
    }
    let new_tab = el()
        .key("new-tab")
        .role("button")
        .label("New tab")
        .size(32.0, 32.0)
        .radius(9.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::PLUS, 14.0).color(p.icon))
        .on_press_tx(|s: &mut State, tx| s.new_tab(tx));
    let tabs = row().key("tabs").grow(1.0).min_w(0.0).gap(2.0).pl(4.0).center_y().children(s.listed().map(|tab| tab_item(tab, active == Some(tab.id), s.is_dragging(tab.id), p))).child(new_tab);
    let space = row()
        .key("space")
        .role("button")
        .h(28.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(14.0)
        .hover(p.hover)
        .child(el().size(6.0, 6.0).radius(3.0).fill(p.accent))
        .child(text(s.spaces[s.space].name.clone()).font(LABEL).color(p.text))
        .child(icon(&icons::CHEVRON_DOWN, 10.0).color(p.icon))
        .on_press_tx(|s: &mut State, tx| parts::space_menu(s, "space", tx));
    strip.child(tabs).child(space)
}

fn selected(tab: El, p: &Palette) -> El {
    tab.fill(p.tab_active).border(0.5, p.tab_ring).shadow(1.0, 2.0, p.tab_shadow)
}

/// A tab in the strip: 196 points wide (220 selected), narrowing when they no longer fit.
fn tab_item(tab: &Tab, active: bool, dragging: bool, p: &Palette) -> El {
    let id = tab.id;
    let title = text(tab.label()).font(if active { BODY_MEDIUM } else { BODY }).color(if active { p.text_strong } else { p.text_strip }).truncate().grow(1.0).min_w(0.0);
    let item = row()
        .key(format!("tab-{id}"))
        .role("tab")
        .w(if active { 220.0 } else { 196.0 })
        .min_w(if active { 120.0 } else { 44.0 })
        .shrink()
        .h(32.0)
        .pl(10.0)
        .pr(if active { 6.0 } else { 10.0 })
        .gap(8.0)
        .center_y()
        .radius(9.0)
        .child(parts::favicon(tab, 16.0, p))
        .child(title)
        .on_press_tx(move |s: &mut State, tx| s.select(id, tx))
        // Dragging a tab sideways moves it a place for each tab's width travelled.
        .on_drag(move |s: &mut State, drag, tx| {
            if drag.phase != Phase::Down {
                tx.animated();
            }
            let steps = if drag.cancelled() { 0 } else { (drag.dx / (drag.w + 2.0)).round() as i32 };
            s.drag_tab(id, steps, drag.phase == Phase::Down, drag.phase == Phase::Up);
        })
        .on_secondary_press(move |s: &mut State, press, tx| parts::tab_menu(s, id, press, tx));
    if !active {
        return if dragging { selected(item, p) } else { item.hover(p.hover) };
    }
    let close = el()
        .key("close")
        .role("button")
        .label("Close tab")
        .size(22.0, 22.0)
        .radius(6.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::CLOSE, 11.0).color(p.icon))
        .on_press_tx(move |s: &mut State, tx| s.close(id, tx));
    selected(item.child(close), p)
}

/// Layout toggle and navigation, the address in the middle, downloads and more at the end.
fn toolbar(s: &State, p: &Palette) -> El {
    let [back, forward, reload] = parts::navigation(s, 30.0, p);
    let leading = row()
        .gap(2.0)
        .center_y()
        .child(parts::icon_button("layout", "Tabs in a sidebar", &icons::SIDEBAR, 30.0, 16.0, true, p, Command::ToggleSidebar))
        .child(back)
        .child(forward)
        .child(reload);
    let pinned = parts::extension_buttons(s, 30.0, p);
    // Both ends are as wide as the wider one, which keeps the address field in the middle.
    let ends = (126.0f32).max((pinned.len() + 3) as f32 * 32.0);
    let trailing = row().gap(2.0).center_y().children(pinned).child(parts::extensions_button(30.0, p)).child(parts::downloads_button(s, 30.0, p)).child(parts::more_button(30.0, p));
    let address = el().grow(1.0).min_w(0.0).center().child(parts::address_field(s, p, p.toolbar_field, 32.0, 9.0, true).w(520.0).shrink().min_w(120.0));
    row().key("toolbar").h(48.0).px(10.0).gap(2.0).center_y().child(leading.w(ends)).child(address).child(trailing.w(ends).justify_end())
}
