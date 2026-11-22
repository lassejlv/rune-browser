//! Tabs down the side: window controls and navigation, the address field, pinned tabs as
//! tiles, the space's tabs, and the spaces along the bottom. The page fills the rest.

use rune::{El, Phase, ScrollSpec, col, el, icon, row, text};

use super::parts::{self, BODY, BODY_MEDIUM, LABEL};
use super::theme::{self, Palette};
use super::{PAGE_RADIUS, icons};
use crate::chromium::Command;
use crate::state::{State, Tab};

const WIDTH: f32 = 256.0;

pub fn view(s: &State, p: &Palette) -> El {
    let page = col()
        .key("page-panel")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .radius(PAGE_RADIUS)
        .fill(p.page)
        .border(0.5, p.page_ring)
        .shadow(1.0, 3.0, p.page_shadow)
        .child(parts::page_area(s, p, PAGE_RADIUS, false, None));
    row().w_full().h_full().fill(p.window).py(8.0).pr(8.0).child(sidebar(s, p)).child(page)
}

fn sidebar(s: &State, p: &Palette) -> El {
    let mut bar = col().key("sidebar").w(WIDTH).h_full().pt(10.0).pr(10.0).pb(8.0).pl(14.0).gap(14.0).window_drag().child(controls(s, p)).child(parts::address_field(s, p, p.field, 36.0, 10.0, false));
    // While an address is being typed, its suggestions take the place of the tabs.
    if let Some(suggestions) = parts::suggestions(s, p, p.tab_active) {
        return bar.child(suggestions.grow(1.0).min_h(0.0)).child(bottom(s, p));
    }
    bar = bar.children(parts::find_bar(s, p, p.field)).children(parts::prompt_bar(s, p, p.field));
    let extensions = parts::extension_buttons(s, 28.0, p);
    if !extensions.is_empty() {
        bar = bar.child(row().key("extension-buttons").gap(2.0).flex_wrap().children(extensions));
    }
    if s.pinned().next().is_some() {
        bar = bar.child(pinned(s, p));
    }
    bar.child(tabs(s, p)).child(bottom(s, p))
}

/// The row the window buttons sit in, with the sidebar toggle and navigation at its end.
fn controls(s: &State, p: &Palette) -> El {
    let [back, forward, reload] = parts::navigation(s, 28.0, p);
    let buttons = row()
        .gap(2.0)
        .center_y()
        .child(parts::extensions_button(28.0, p))
        .child(parts::icon_button("layout", "Tabs on top", &icons::SIDEBAR, 28.0, 16.0, true, p, Command::ToggleSidebar))
        .child(back)
        .child(forward)
        .child(reload);
    let controls = row().key("controls").h(28.0).center_y();
    match parts::window_buttons() {
        Some(window) => controls.pl(4.0).child(window).child(el().grow(1.0)).child(buttons),
        None => controls.justify_end().child(buttons),
    }
}

/// Pinned tabs, as a row of tiles showing only their icons.
fn pinned(s: &State, p: &Palette) -> El {
    let active = s.active_id();
    // Four tiles to a row, as designed; a fifth starts another row.
    let width = (WIDTH - 24.0 - 3.0 * 6.0) / 4.0;
    let mut tiles = row().key("pinned").gap(6.0).flex_wrap();
    for tab in s.pinned() {
        let id = tab.id;
        let tile = el()
            .key(format!("pin-{id}"))
            .role("tab")
            .label(tab.label())
            .help(tab.label())
            .size(width, 40.0)
            .radius(10.0)
            .center()
            .child(parts::favicon(tab, 18.0, p))
            .on_press_tx(move |s: &mut State, tx| s.select(id, tx))
            .on_secondary_press(move |s: &mut State, press, tx| parts::tab_menu(s, id, press, tx));
        tiles = tiles.child(if active == Some(id) { selected(tile, p) } else { tile.fill(p.field) });
    }
    tiles
}

/// The selected tab's look: a raised pill.
fn selected(tab: El, p: &Palette) -> El {
    tab.fill(p.tab_active).border(0.5, p.tab_ring).shadow(1.0, 2.0, p.tab_shadow)
}

fn tabs(s: &State, p: &Palette) -> El {
    let active = s.active_id();
    let header = row()
        .key("space")
        .h(28.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .child(el().size(6.0, 6.0).radius(3.0).fill(p.accent))
        .child(text(s.spaces[s.space].name.clone()).font(LABEL).color(p.text))
        .child(el().grow(1.0).h(1.0).fill(p.divider))
        .on_secondary_press(|s: &mut State, press, tx| parts::space_menu(s, press, tx));
    let new_tab = row()
        .key("new-tab")
        .role("button")
        .h(34.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(9.0)
        .hover(p.hover)
        .child(el().size(16.0, 16.0).center().child(icon(&icons::PLUS, 14.0).color(p.icon)))
        .child(text("New Tab").font(BODY).color(p.muted))
        .on_press_tx(|s: &mut State, tx| s.new_tab(tx));
    let list = col().key("tabs").gap(2.0).children(s.listed().map(|tab| tab_row(tab, active == Some(tab.id), s.is_dragging(tab.id), p))).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0);
    col().key("tab-section").grow(1.0).min_h(0.0).gap(2.0).pt(4.0).child(header).child(new_tab).child(list)
}

/// A tab row's height and the gap under it: one place in the list.
const ROW_STEP: f32 = 36.0;

fn tab_row(tab: &Tab, active: bool, dragging: bool, p: &Palette) -> El {
    let id = tab.id;
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
    // The selected tab always shows its close button. On the others it appears when they're
    // pointed at, over the end of the title, so a resting title has the row's whole width.
    let close = if active { close } else { close.fill(p.row_hover).absolute(WIDTH - 24.0 - 28.0, 6.0).group_opacity(0.0, 1.0) };
    let title = text(tab.label()).font(if active { BODY_MEDIUM } else { BODY }).color(if active { p.text_strong } else { p.text_soft }).truncate().grow(1.0).min_w(0.0);
    let item = row()
        .key(format!("tab-{id}"))
        .role("tab")
        .h(34.0)
        .pl(10.0)
        .pr(if active { 6.0 } else { 10.0 })
        .gap(10.0)
        .center_y()
        .radius(9.0)
        .hover_group()
        .child(parts::favicon(tab, 16.0, p))
        .child(title)
        .child(close)
        .on_press_tx(move |s: &mut State, tx| s.select(id, tx))
        // Dragging a row up or down moves the tab a place for each row's height travelled.
        .on_drag(move |s: &mut State, drag, tx| {
            if drag.phase != Phase::Down {
                tx.animated();
            }
            let steps = if drag.cancelled() { 0 } else { (drag.dy / ROW_STEP).round() as i32 };
            s.drag_tab(id, steps, drag.phase == Phase::Down, drag.phase == Phase::Up);
        })
        .on_secondary_press(move |s: &mut State, press, tx| parts::tab_menu(s, id, press, tx));
    if active || dragging { selected(item, p) } else { item.hover(p.row_hover) }
}

/// Downloads and the "more" menu, a dot for each space, and a button that adds a space.
fn bottom(s: &State, p: &Palette) -> El {
    let mut dots = row().key("spaces").center_y();
    for index in 0..s.spaces.len() {
        let color = if index == s.space { p.accent } else if p.dark { theme::accent(index, true).alpha(0.45) } else { p.dot };
        let dot = el()
            .key(format!("space-dot-{index}"))
            .role("button")
            .label(s.spaces[index].name.clone())
            .help(s.spaces[index].name.clone())
            .size(16.0, 16.0)
            .center()
            .child(el().size(6.0, 6.0).radius(3.0).fill(color))
            .on_press_tx(move |s: &mut State, tx| s.switch_space(index, tx));
        dots = dots.child(dot);
    }
    let add = el()
        .key("new-space")
        .role("button")
        .label("New space")
        .size(28.0, 28.0)
        .radius(7.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::PLUS, 15.0).color(p.icon))
        .on_press_tx(|s: &mut State, tx| s.add_space(tx));
    // Equal-width ends keep the dots in the middle.
    let leading = row().w(58.0).gap(2.0).center_y().child(parts::downloads_button(s, 28.0, p)).child(parts::more_button(28.0, p));
    row().key("bottom").h(32.0).center_y().justify_between().child(leading).child(dots).child(row().w(58.0).justify_end().child(add))
}
