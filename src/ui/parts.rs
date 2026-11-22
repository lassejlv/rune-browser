//! What the two layouts share: buttons, favicons, the address field, menus and the page area.

use rune::tok::TextStyle;
use rune::{Cursor, El, Icon, Key, PointerPress, PopupItem, PopupMenu, ScrollSpec, Span, Tx, col, el, icon, rich, row, text, text_input};

use super::theme::{Palette, rgb, rgba};
use super::icons;
use crate::address;
use crate::chromium::{self, Command, Frame};
use crate::extensions::{self, Extension};
use crate::history;
use crate::settings::{ENGINES, Theme};
use crate::state::{EXTENSIONS_URL, HISTORY_URL, Layout, SETTINGS_URL, State, Tab};

pub const BODY: TextStyle = TextStyle::new(13.0, 400.0).lh(16.0);
pub const BODY_MEDIUM: TextStyle = TextStyle::new(13.0, 500.0).lh(16.0);
pub const LABEL: TextStyle = TextStyle::new(12.0, 600.0).lh(16.0);
pub const SMALL: TextStyle = TextStyle::new(11.5, 400.0).lh(15.0);

/// A suggestion row's height, and the list's padding above and below its rows.
const SUGGESTION_HEIGHT: f32 = 44.0;
const SUGGESTION_PAD: f32 = 6.0;

/// A square button holding one icon, which runs `command`. A disabled one is dimmed and inert.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(key: &'static str, label: &'static str, glyph: &'static Icon, side: f32, glyph_size: f32, enabled: bool, p: &Palette, command: Command) -> El {
    turned_icon_button(key, label, glyph, 0.0, side, glyph_size, enabled, p, command)
}

/// [`icon_button`] with its icon turned by `turn` radians.
#[allow(clippy::too_many_arguments)]
fn turned_icon_button(key: &'static str, label: &'static str, glyph: &'static Icon, turn: f32, side: f32, glyph_size: f32, enabled: bool, p: &Palette, command: Command) -> El {
    let button = el()
        .key(key)
        .role("button")
        .label(label)
        .size(side, side)
        .radius(7.0)
        .center()
        .child(icon(glyph, glyph_size).color(p.icon).icon_rotation(turn))
        .opacity(if enabled { 1.0 } else { 0.4 });
    if enabled { button.hover(p.hover).on_press_tx(move |s: &mut State, tx| s.run(command, tx)) } else { button }
}

/// The window's close, minimize and maximize buttons where the app draws its own titlebar:
/// on Linux, whose window has no frame. They sit and look where macOS puts its traffic
/// lights, so both layouts read the same on every system. None on macOS, which draws its own.
pub fn window_buttons() -> Option<El> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let light = |key: &'static str, label: &'static str, fill: u32, glyph: &'static Icon, act: fn(&mut Tx)| {
        el()
            .key(key)
            .role("button")
            .label(label)
            .size(12.0, 12.0)
            .radius(6.0)
            .fill(rgb(fill))
            .border(0.5, rgba(0x000000, 0.12))
            .center()
            .child(icon(glyph, 8.0).color(rgba(0x000000, 0.55)).group_opacity(0.0, 1.0))
            .on_press_tx(move |_: &mut State, tx| act(tx))
    };
    Some(
        row()
            .key("window-buttons")
            .gap(8.0)
            .center_y()
            .hover_group()
            .child(light("window-close", "Close", 0xFF5F57, &icons::CLOSE, Tx::close_window))
            .child(light("window-minimize", "Minimize", 0xFEBC2E, &icons::MINIMIZE, Tx::minimize))
            .child(light("window-maximize", "Maximize", 0x28C840, &icons::MAXIMIZE, Tx::toggle_maximized)),
    )
}

/// A tab's icon: its favicon, or a globe until it has one. Dimmed while the page loads.
pub fn favicon(tab: &Tab, size: f32, p: &Palette) -> El {
    let picture = match &tab.favicon {
        Some(image) => el().size(size, size).radius(size / 4.0).pixels(image.pixels()),
        None => icon(&icons::GLOBE, size).color(p.icon),
    };
    el().key("favicon").size(size, size).center().child(picture).opacity(if tab.loading { 0.45 } else { 1.0 })
}

/// Back, forward and reload (or stop, while the page loads) for the active tab.
pub fn navigation(s: &State, side: f32, p: &Palette) -> [El; 3] {
    let tab = s.active();
    let (back, forward, loading) = tab.map_or((false, false, false), |tab| (tab.can_go_back, tab.can_go_forward, tab.loading));
    let has_page = tab.is_some_and(|tab| !tab.is_blank());
    [
        icon_button("back", "Back", &icons::BACK, side, 16.0, back, p, Command::Back),
        // Forward is the back arrow turned around.
        turned_icon_button("forward", "Forward", &icons::BACK, std::f32::consts::PI, side, 16.0, forward, p, Command::Forward),
        if loading {
            icon_button("stop", "Stop", &icons::CLOSE, side, 12.0, true, p, Command::Stop)
        } else {
            icon_button("reload", "Reload", &icons::RELOAD, side, 15.0, has_page, p, Command::Reload)
        },
    ]
}

/// The address field: the active tab's site and path at rest, an editable URL once pressed.
/// `centered` puts the resting text in the middle, as the top toolbar shows it.
pub fn address_field(s: &State, p: &Palette, fill: rune::Color, height: f32, radius: f32, centered: bool) -> El {
    let field = row().key("address").h(height).pl(12.0).pr(10.0).gap(8.0).center_y().radius(radius).fill(fill).cursor(Cursor::Text);
    if s.address.focused {
        let input = text_input(&s.address, BODY)
            .key("address-input")
            .label("Address")
            .single_line()
            .grow(1.0)
            .min_w(0.0)
            .color(p.text)
            .placeholder("Search or enter address", p.faint)
            .on_edit(|s: &mut State| &mut s.address)
            .on_submit(|s: &mut State, tx| s.submit_address(tx))
            .on_input_key(|s: &mut State, event, tx| match event.key {
                Key::Escape => {
                    s.cancel_address(tx);
                    true
                }
                // The arrows walk the suggestions when there are any.
                Key::Down => s.pick(true),
                Key::Up => s.pick(false),
                _ => false,
            });
        return field.child(input);
    }
    let url = s.active().map(|tab| tab.url.as_str()).unwrap_or_default();
    let resting = if url.is_empty() {
        text("Search or enter address").font(BODY).color(p.faint)
    } else {
        let (site, rest) = address::display(url);
        let split = site.len();
        let whole = format!("{site}{rest}");
        let end = whole.len();
        rich(whole, vec![Span::new(0, split).color(p.text), Span::new(split, end).color(p.faint)]).font(BODY).color(p.text).truncate()
    };
    let resting = if centered { resting } else { resting.grow(1.0).min_w(0.0) };
    let field = if centered { field.center() } else { field };
    let field = if url.starts_with("https://") { field.child(icon(&icons::LOCK, 13.0).color(p.icon)) } else { field };
    field.child(resting).on_press(|s: &mut State| s.focus_address())
}

/// The visited pages matching what is typed in the address field, to show under it. None
/// when there is nothing to suggest.
pub fn suggestions(s: &State, p: &Palette, picked_fill: rune::Color) -> Option<El> {
    let found = s.suggestions();
    if found.is_empty() {
        return None;
    }
    let picked = s.picked();
    let mut list = col().key("suggestions").role("listbox").w_full().py(SUGGESTION_PAD);
    for (index, visit) in found.iter().enumerate() {
        let url = visit.url.clone();
        let (site, rest) = address::display(&visit.url);
        let title = if visit.title.is_empty() { site.clone() } else { visit.title.clone() };
        let item = col()
            .key(format!("suggestion-{index}"))
            .role("option")
            .label(title.clone())
            .h(SUGGESTION_HEIGHT)
            .px(10.0)
            .gap(1.0)
            .justify_center()
            .radius(9.0)
            // A press must not take the keyboard from the field before it lands.
            .preserve_focus_on_press(true)
            .child(text(title).font(BODY).color(p.text).truncate())
            .child(text(format!("{site}{rest}")).font(SMALL).color(p.muted).truncate())
            .on_press_tx(move |s: &mut State, tx| s.go(&url, tx));
        list = list.child(if picked == Some(index) { item.fill(picked_fill).border(0.5, p.tab_ring) } else { item.hover(p.row_hover) });
    }
    Some(list)
}

/// How tall [`suggestions`] draws for the current state.
pub fn suggestions_height(s: &State) -> f32 {
    match s.suggestions().len() {
        0 => 0.0,
        count => count as f32 * SUGGESTION_HEIGHT + 2.0 * SUGGESTION_PAD,
    }
}

/// A small square button for the find bar and prompts.
fn small_button(key: &'static str, label: &'static str, glyph: &'static Icon, turn: f32, p: &Palette) -> El {
    el().key(key).role("button").label(label).size(22.0, 22.0).radius(6.0).center().hover(p.hover).child(icon(glyph, 11.0).color(p.icon).icon_rotation(turn))
}

/// The find bar: what to find, how many matches, previous and next, and close.
pub fn find_bar(s: &State, p: &Palette, fill: rune::Color) -> Option<El> {
    let find = s.find.as_ref()?;
    let input = text_input(&find.edit, BODY)
        .key("find-input")
        .label("Find in page")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(p.text)
        .placeholder("Find in page", p.faint)
        .on_edit(|s: &mut State| match &mut s.find {
            Some(find) => &mut find.edit,
            // The bar closed with a key still on its way: the address field takes it.
            None => &mut s.address,
        })
        .on_input_change(|s: &mut State, _| s.find_changed())
        .on_submit(|s: &mut State, tx| s.find_step(!tx.mods().shift))
        .on_input_key(|s: &mut State, event, tx| {
            if event.key == Key::Escape {
                s.close_find(tx);
                true
            } else {
                false
            }
        });
    let count = if find.edit.text.is_empty() {
        String::new()
    } else if find.count == 0 {
        "No matches".into()
    } else {
        format!("{}/{}", find.active, find.count)
    };
    let quarter = std::f32::consts::FRAC_PI_2;
    Some(
        row()
            .key("find")
            .h(36.0)
            .pl(12.0)
            .pr(6.0)
            .gap(4.0)
            .center_y()
            .radius(10.0)
            .fill(fill)
            .child(input)
            .child(text(count).key("find-count").font(SMALL).color(p.muted))
            // The back arrow turned to point up and down.
            .child(small_button("find-previous", "Previous match", &icons::BACK, quarter, p).on_press(|s: &mut State| s.find_step(false)))
            .child(small_button("find-next", "Next match", &icons::BACK, -quarter, p).on_press(|s: &mut State| s.find_step(true)))
            .child(small_button("find-close", "Close", &icons::CLOSE, 0.0, p).on_press_tx(|s: &mut State, tx| s.close_find(tx))),
    )
}

/// A site's request, with Block and Allow.
pub fn prompt_bar(s: &State, p: &Palette, fill: rune::Color) -> Option<El> {
    let prompt = s.prompt()?;
    let id = prompt.id;
    let (site, _) = address::display(&prompt.origin);
    let choice = |key: &'static str, title: &'static str, allow: bool| {
        let button = row().key(key).role("button").h(26.0).px(12.0).center().radius(8.0).child(text(title).font(LABEL).color(if allow { p.page } else { p.text }));
        let button = if allow { button.fill(p.accent) } else { button.border(1.0, p.divider).hover(p.hover) };
        button.on_press(move |s: &mut State| s.answer_prompt(id, allow))
    };
    Some(
        col()
            .key("prompt")
            .role("alertdialog")
            .padding(12.0)
            .gap(10.0)
            .radius(10.0)
            .fill(fill)
            // A file on disk has no site to name.
            .child(text(format!("{} wants to {}.", if site.is_empty() { "This page" } else { site.as_str() }, prompt.what)).font(BODY).color(p.text).wrap())
            .child(row().gap(8.0).justify_end().child(choice("block", "Block", false)).child(choice("allow", "Allow", true))),
    )
}

/// The History tab: the pages visited, newest first.
fn history_page(s: &State, p: &Palette) -> El {
    let now = history::now();
    let mut list = col().key("history-list").w_full().max_w(760.0).gap(2.0);
    for (index, visit) in s.history.recent(300).into_iter().enumerate() {
        let url = visit.url.clone();
        let (site, rest) = address::display(&visit.url);
        let title = if visit.title.is_empty() { format!("{site}{rest}") } else { visit.title.clone() };
        list = list.child(
            row()
                .key(format!("visit-{index}"))
                .role("link")
                .label(title.clone())
                .h(38.0)
                .px(12.0)
                .gap(14.0)
                .center_y()
                .radius(9.0)
                .hover(p.hover)
                .child(text(title).font(BODY).color(p.text).truncate().grow(1.0).min_w(0.0))
                .child(text(site).font(SMALL).color(p.muted).truncate().max_w(220.0))
                .child(text(history::ago(visit.last, now)).font(SMALL).color(p.faint).w(86.0).align(rune::Align::End))
                .on_press_tx(move |s: &mut State, tx| s.go(&url, tx)),
        );
    }
    let empty = s.history.len() == 0;
    let clear = outline_button("clear-history", "Clear History", p).on_press(|s: &mut State| s.clear_history());
    let header = row()
        .w_full()
        .max_w(760.0)
        .center_y()
        .justify_between()
        .px(12.0)
        .child(text("History").font(TextStyle::new(28.0, 600.0).lh(34.0).tracking(-0.5)).color(p.text))
        .children((!empty).then_some(clear));
    let body = if empty { text("Pages you visit show up here.").font(TextStyle::new(14.0, 400.0).lh(20.0)).color(p.muted).px(12.0).w_full().max_w(760.0) } else { list };
    col().key("history").w_full().h_full().items_center().pt(48.0).px(24.0).gap(20.0).child(header).child(col().w_full().items_center().pb(24.0).child(body).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
}

/// A bordered text button, as on the History and Extensions pages.
fn outline_button(key: impl Into<String>, title: &str, p: &Palette) -> El {
    row().key(key).role("button").h(28.0).px(12.0).center().radius(8.0).border(1.0, p.divider).hover(p.hover).child(text(title.to_owned()).font(LABEL).color(p.text))
}

/// An extension's icon, or a puzzle piece when it has none.
fn extension_icon(s: &State, extension: &Extension, size: f32, p: &Palette) -> El {
    match s.extension_icons.get(&extension.id) {
        Some(image) => el().size(size, size).radius(size / 5.0).pixels(image.pixels()),
        None => icon(&icons::PUZZLE, size).color(p.icon),
    }
}

/// What can be done with one extension, as menu entries named `verb:id`.
fn extension_entries(s: &State, extension: &Extension) -> PopupMenu {
    let id = &extension.id;
    let mut menu = PopupMenu::new();
    if extension.popup.is_some() {
        menu = menu.item(PopupItem::new(format!("open:{id}"), "Open").enabled(extension.enabled));
    }
    if extension.options.is_some() {
        menu = menu.item(PopupItem::new(format!("options:{id}"), "Options").enabled(extension.enabled));
    }
    menu.item(PopupItem::new(format!("pin:{id}"), "Pin to Toolbar").checked(s.is_pinned(id)))
        .item(PopupItem::new(format!("enable:{id}"), "Turned On").checked(extension.enabled))
        .item(PopupItem::new(format!("listing:{id}"), "View in Chrome Web Store").enabled(!extension.unpacked))
        .separator()
        .item(PopupItem::new(format!("remove:{id}"), "Remove…").enabled(extension.removable))
}

fn pick_extension_entry(s: &mut State, picked: Option<&str>, tx: &mut Tx) {
    match picked {
        Some("manage") => s.run(Command::Extensions, tx),
        Some("store") => s.open(extensions::STORE, false, None, tx),
        Some(entry) => s.extension_command(entry, tx),
        None => {}
    }
}

/// The menu behind the extensions button: every extension with what can be done with it.
fn extensions_menu(s: &State, tx: &mut Tx) {
    let mut menu = PopupMenu::new();
    for extension in &s.extensions {
        menu = menu.submenu(extension.name.clone(), extension_entries(s, extension));
    }
    if !s.extensions.is_empty() {
        menu = menu.separator();
    }
    let menu = menu.item(PopupItem::new("manage", "Manage Extensions")).item(PopupItem::new("store", "Chrome Web Store"));
    tx.popup_menu(menu, "extensions-button", pick_extension_entry);
}

/// The puzzle-piece button that lists the extensions.
pub fn extensions_button(side: f32, p: &Palette) -> El {
    el().key("extensions-button")
        .role("button")
        .label("Extensions")
        .help("Extensions")
        .size(side, side)
        .radius(7.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::PUZZLE, 16.0).color(p.icon))
        .on_press_tx(|s: &mut State, tx| extensions_menu(s, tx))
}

/// A button for each pinned extension: a press opens its popup, a secondary press its menu.
pub fn extension_buttons(s: &State, side: f32, p: &Palette) -> Vec<El> {
    s.pinned_extensions()
        .map(|extension| {
            let (id, menu_id) = (extension.id.clone(), extension.id.clone());
            el().key(format!("extension-button-{id}"))
                .role("button")
                .label(extension.name.clone())
                .help(extension.name.clone())
                .size(side, side)
                .radius(7.0)
                .center()
                .hover(p.hover)
                .child(extension_icon(s, extension, 17.0, p))
                .on_press_tx(move |s: &mut State, tx| s.open_extension(&id, tx))
                .on_secondary_press(move |s: &mut State, press, tx| {
                    let Some(extension) = s.extension(&menu_id) else { return };
                    let menu = extension_entries(s, extension).separator().item(PopupItem::new("manage", "Manage Extensions"));
                    tx.popup_menu(menu, press, pick_extension_entry);
                })
        })
        .collect()
}

/// The Extensions tab: each installed extension with its switch, its pin and its removal.
fn extensions_page(s: &State, p: &Palette) -> El {
    let width = 760.0;
    let store = row()
        .key("web-store")
        .role("button")
        .h(36.0)
        .px(16.0)
        .center()
        .radius(10.0)
        .fill(p.accent)
        .child(text("Chrome Web Store").font(LABEL).color(p.page))
        .on_press_tx(|s: &mut State, tx| s.open(extensions::STORE, false, None, tx));
    let title = text("Extensions").font(TextStyle::new(28.0, 600.0).lh(34.0).tracking(-0.5)).color(p.text).grow(1.0);

    let mut list = col().key("extension-list").w_full().max_w(width).gap(2.0);
    for extension in &s.extensions {
        let id = &extension.id;
        let about = match (extension.unpacked, extension.enabled) {
            (true, _) => format!("Version {} · loaded from a folder", extension.version),
            (false, true) => format!("Version {}", extension.version),
            (false, false) => format!("Version {} · turned off", extension.version),
        };
        let words = col().grow(1.0).min_w(0.0).gap(1.0).child(text(extension.name.clone()).font(BODY_MEDIUM).color(p.text).truncate()).child(text(about).font(SMALL).color(p.muted).truncate());
        let mut item = row().key(format!("extension-{id}")).h(52.0).px(12.0).gap(10.0).center_y().radius(9.0).child(el().size(28.0, 28.0).center().child(extension_icon(s, extension, 24.0, p)).opacity(if extension.enabled { 1.0 } else { 0.45 })).child(words);
        if extension.enabled {
            if let Some(options) = extension.options.clone() {
                item = item.child(outline_button(format!("options-{id}"), "Options", p).on_press_tx(move |s: &mut State, tx| s.open(&options, false, None, tx)));
            }
            let pin = id.clone();
            item = item.child(outline_button(format!("pin-{id}"), if s.is_pinned(id) { "Unpin" } else { "Pin" }, p).on_press(move |s: &mut State| s.toggle_extension_pin(&pin)));
        }
        if extension.removable {
            let remove = id.clone();
            item = item.child(outline_button(format!("remove-{id}"), "Remove", p).on_press(move |s: &mut State| s.remove_extension(&remove)));
        }
        let (toggle, enabled) = (id.clone(), extension.enabled);
        list = list.child(item.child(switch(format!("enabled-{id}"), extension.name.clone(), enabled, p).on_press(move |s: &mut State| s.set_extension_enabled(&toggle, !enabled))));
    }

    let body = if s.extensions.is_empty() {
        text("Add an extension from the Chrome Web Store with its own Add button. Pin one to give it a button beside the address field.")
            .font(TextStyle::new(14.0, 400.0).lh(20.0))
            .color(p.muted)
            .px(12.0)
            .w_full()
            .max_w(width)
            .wrap()
    } else {
        list
    };
    col()
        .key("extensions")
        .w_full()
        .h_full()
        .items_center()
        .pt(48.0)
        .px(24.0)
        .gap(16.0)
        .child(row().w_full().max_w(width).px(12.0).gap(8.0).center_y().child(title).child(store))
        .child(col().w_full().items_center().pb(24.0).child(body).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
}

/// One of a segmented control's choices.
fn segment(key: String, title: &str, chosen: bool, p: &Palette) -> El {
    let item = row().key(key).role("button").h(26.0).px(12.0).center().radius(7.0).child(text(title.to_owned()).font(LABEL).color(if chosen { p.text_strong } else { p.muted }));
    if chosen { item.fill(p.tab_active).border(0.5, p.tab_ring) } else { item.hover(p.hover) }
}

/// The track a segmented control's choices sit in.
fn segments(key: &'static str) -> El {
    row().key(key).padding(2.0).gap(2.0).radius(9.0)
}

/// An on/off switch.
fn switch(key: impl Into<String>, label: impl Into<String>, on: bool, p: &Palette) -> El {
    let knob = el().size(16.0, 16.0).radius(8.0).fill(rune::Color::WHITE);
    let track = row().key(key).role("switch").label(label).w(36.0).h(20.0).px(2.0).center_y().radius(10.0).fill(if on { p.accent } else { p.divider });
    if on { track.child(el().grow(1.0)).child(knob) } else { track.child(knob) }
}

/// A row of the Settings page: what it is, a line about it, and its control.
fn setting(title: &str, about: &str, control: El, p: &Palette) -> El {
    let words = col().grow(1.0).min_w(0.0).gap(2.0).child(text(title.to_owned()).font(BODY_MEDIUM).color(p.text));
    let words = if about.is_empty() { words } else { words.child(text(about.to_owned()).font(SMALL).color(p.muted).wrap()) };
    row().w_full().min_h(52.0).py(10.0).px(14.0).gap(16.0).center_y().child(words).child(control)
}

/// A titled group of Settings rows.
fn section(title: &str, rows: Vec<El>, p: &Palette) -> El {
    let mut card = col().w_full().radius(12.0).fill(p.field).border(0.5, p.tab_ring);
    let last = rows.len().saturating_sub(1);
    for (index, item) in rows.into_iter().enumerate() {
        card = card.child(item);
        if index < last {
            card = card.child(el().w_full().h(1.0).fill(p.divider));
        }
    }
    col().w_full().gap(8.0).child(text(title.to_owned()).font(LABEL).color(p.muted).px(14.0)).child(card)
}

/// The Settings tab.
fn settings_page(s: &State, p: &Palette) -> El {
    let width = 640.0;
    let settings = &s.settings;

    let mut layouts = segments("layout-choice").fill(p.window);
    for (layout, title) in [(Layout::Sidebar, "Sidebar"), (Layout::TopTabs, "On top")] {
        layouts = layouts.child(segment(format!("layout-{title}"), title, s.layout == layout, p).on_press_tx(move |s: &mut State, tx| {
            if s.layout != layout {
                s.toggle_layout(tx);
            }
        }));
    }
    let mut themes = segments("theme-choice").fill(p.window);
    for (theme, title) in Theme::ALL {
        themes = themes.child(segment(format!("theme-{title}"), title, settings.theme == theme, p).on_press(move |s: &mut State| s.set(|settings| settings.theme = theme)));
    }
    let engine = outline_button("search-engine", settings.engine().name, p).gap(6.0).child(icon(&icons::CHEVRON_DOWN, 10.0).color(p.icon)).on_press_tx(|_: &mut State, tx| {
        let menu = ENGINES.iter().fold(PopupMenu::new(), |menu, engine| menu.item(PopupItem::new(engine.id, engine.name)));
        tx.popup_menu(menu, "search-engine", |s: &mut State, picked, _| {
            if let Some(id) = picked.map(str::to_owned) {
                s.set(|settings| settings.search = id);
            }
        });
    });
    let folder = settings.downloads_folder();
    let home = std::env::var("HOME").unwrap_or_default();
    let shown = folder.to_string_lossy().strip_prefix(&home).filter(|_| !home.is_empty()).map_or_else(|| folder.to_string_lossy().into_owned(), |rest| format!("~{rest}"));
    let choose = outline_button("downloads-folder", "Change…", p).on_press_tx(|_: &mut State, tx| {
        let options = rune::FileDialogOptions { title: "Where downloads go".into(), ..Default::default() };
        tx.open_folder(options, |s: &mut State, picked, _| {
            if let Ok(Some(folder)) = picked {
                s.set(|settings| settings.downloads = Some(folder));
            }
        });
    });
    let restore = switch("restore-tabs", "Reopen tabs", settings.restore_tabs, p).on_press(|s: &mut State| s.set(|settings| settings.restore_tabs = !settings.restore_tabs));
    let cookies = switch("third-party-cookies", "Block third-party cookies", settings.block_third_party_cookies, p).on_press(|s: &mut State| s.set(|settings| settings.block_third_party_cookies = !settings.block_third_party_cookies));
    let track = switch("do-not-track", "Do Not Track", settings.do_not_track, p).on_press(|s: &mut State| s.set(|settings| settings.do_not_track = !settings.do_not_track));
    let clear = row()
        .gap(8.0)
        .child(outline_button("clear-history", "History", p).on_press(|s: &mut State| s.clear_history()))
        .child(outline_button("clear-cookies", "Cookies", p).on_press(|s: &mut State| s.clear_cookies()))
        .child(outline_button("clear-cache", "Cache", p).on_press(|s: &mut State| s.clear_cache()));

    let body = col()
        .w_full()
        .max_w(width)
        .gap(24.0)
        .pb(32.0)
        .child(section("Appearance", vec![setting("Tabs", "", layouts, p), setting("Theme", "Pages are told to prefer the same one.", themes, p)], p))
        .child(section("Search", vec![setting("Search engine", "What the address field searches with.", engine, p)], p))
        .child(section("Downloads", vec![setting("Save files to", &shown, choose, p)], p))
        .child(section("On startup", vec![setting("Reopen the tabs from last time", "Pinned tabs and spaces always come back.", restore, p)], p))
        .child(section(
            "Privacy",
            vec![
                setting("Block third-party cookies", "Sites can't read cookies set by other sites. Some sign-ins embedded in a page stop working.", cookies, p),
                setting("Ask sites not to track", "Sends the Do Not Track request. Sites may ignore it.", track, p),
                setting("Clear", s.settings_note.unwrap_or("Deleting cookies signs you out everywhere."), clear, p),
            ],
            p,
        ));
    col()
        .key("settings")
        .w_full()
        .h_full()
        .items_center()
        .pt(48.0)
        .px(24.0)
        .gap(20.0)
        .child(row().w_full().max_w(width).px(14.0).child(text("Settings").font(TextStyle::new(28.0, 600.0).lh(34.0).tracking(-0.5)).color(p.text)))
        .child(col().w_full().items_center().child(body).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
}

/// The menu behind the "more" button.
pub fn more_menu(s: &State, anchor: &'static str, tx: &mut Tx) {
    let layout = if s.layout == crate::state::Layout::Sidebar { "Show Tabs on Top" } else { "Show Tabs in a Sidebar" };
    let has_page = s.active().is_some_and(Tab::is_web);
    let menu = PopupMenu::new()
        .item(PopupItem::new("new-tab", "New Tab"))
        .item(PopupItem::new("layout", layout))
        .separator()
        .item(PopupItem::new("history", "History"))
        .item(PopupItem::new("extensions", "Extensions"))
        .item(PopupItem::new("settings", "Settings…"))
        .separator()
        .item(PopupItem::new("find", "Find in Page…").enabled(has_page))
        .item(PopupItem::new("print", "Print…").enabled(has_page))
        .item(PopupItem::new("devtools", "Developer Tools").enabled(has_page));
    tx.popup_menu(menu, anchor, |s: &mut State, picked, tx| {
        let command = match picked {
            Some("new-tab") => Command::NewTab,
            Some("layout") => Command::ToggleSidebar,
            Some("history") => Command::History,
            Some("extensions") => Command::Extensions,
            Some("settings") => Command::Settings,
            Some("find") => Command::Find,
            Some("print") => Command::Print,
            Some("devtools") => Command::DevTools,
            _ => return,
        };
        s.run(command, tx);
    });
}

/// The "more" button, which opens [`more_menu`].
pub fn more_button(side: f32, p: &Palette) -> El {
    el().key("more")
        .role("button")
        .label("More")
        .size(side, side)
        .radius(7.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::MORE, 16.0).color(p.icon))
        .on_press_tx(|s: &mut State, tx| more_menu(s, "more", tx))
}

/// What shows in place of a page that failed to load or stopped.
fn error_page(tab: &Tab, error: &str, p: &Palette) -> El {
    let retry = row()
        .key("retry")
        .role("button")
        .h(30.0)
        .px(14.0)
        .center()
        .radius(9.0)
        .fill(p.accent)
        .child(text("Try Again").font(LABEL).color(p.page))
        .on_press_tx(|s: &mut State, tx| s.run(Command::Reload, tx));
    col()
        .key("error")
        .center()
        .gap(10.0)
        .max_w(520.0)
        .child(text("This page didn't load").font(TextStyle::new(28.0, 600.0).lh(34.0).tracking(-0.5)).color(p.text))
        .child(text(address::display(&tab.url).0).font(TextStyle::new(14.0, 500.0).lh(20.0)).color(p.text))
        .child(text(error.to_owned()).font(TextStyle::new(14.0, 400.0).lh(20.0)).color(p.muted).wrap())
        .child(el().h(6.0))
        .child(retry)
}

/// Whether the app draws the page area itself for the tab on screen, over any page it has.
pub fn draws_page(s: &State) -> bool {
    s.active().is_some_and(|tab| tab.error.is_some() || tab.is_internal())
}

/// The menu a secondary press on a tab opens.
pub fn tab_menu(s: &mut State, id: chromium::TabId, press: PointerPress, tx: &mut Tx) {
    let Some(tab) = s.tab(id) else { return };
    let has_page = !tab.is_blank();
    let menu = PopupMenu::new()
        .item(PopupItem::new("pin", if tab.pinned { "Unpin Tab" } else { "Pin Tab" }).enabled(has_page))
        .item(PopupItem::new("duplicate", "Duplicate Tab").enabled(has_page))
        .item(PopupItem::new("copy", "Copy Link").enabled(has_page))
        .separator()
        .item(PopupItem::new("close", "Close Tab"));
    tx.popup_menu(menu, press, move |s: &mut State, picked, tx| match picked {
        Some("pin") => s.toggle_pin(id),
        Some("duplicate") => s.duplicate(id, tx),
        Some("copy") => {
            if let Some(url) = s.tab(id).map(|tab| tab.url.clone()) {
                tx.copy(url);
            }
        }
        Some("close") => s.close(id, tx),
        _ => {}
    });
}

/// The menu for the spaces: each one to switch to, then adding and removing.
pub fn space_menu(s: &mut State, anchor: impl Into<rune::PopupAnchor>, tx: &mut Tx) {
    let mut menu = PopupMenu::new();
    for (index, space) in s.spaces.iter().enumerate() {
        menu = menu.item(PopupItem::new(format!("space-{index}"), space.name.clone()).checked(index == s.space));
    }
    let menu = menu.separator().item(PopupItem::new("new", "New Space")).item(PopupItem::new("remove", "Remove This Space").enabled(s.spaces.len() > 1));
    tx.popup_menu(menu, anchor, |s: &mut State, picked, tx| match picked {
        Some("new") => s.add_space(tx),
        Some("remove") => s.remove_space(tx),
        Some(id) => {
            if let Some(index) = id.strip_prefix("space-").and_then(|index| index.parse().ok()) {
                s.switch_space(index, tx);
            }
        }
        None => {}
    });
}

/// Opens the downloads folder in Finder.
pub fn show_downloads(s: &mut State) {
    let _ = std::process::Command::new("open").arg(s.settings.downloads_folder()).spawn();
}

/// The downloads button, with a dot in the space's colour while something downloads.
pub fn downloads_button(s: &State, side: f32, p: &Palette) -> El {
    let button = el()
        .key("downloads")
        .role("button")
        .label("Downloads")
        .size(side, side)
        .radius(7.0)
        .center()
        .hover(p.hover)
        .child(icon(&icons::DOWNLOAD, 15.0).color(p.icon))
        .on_press(show_downloads);
    if s.downloads.is_empty() { button } else { button.child(el().key("downloading").size(6.0, 6.0).radius(3.0).fill(p.accent).absolute(side - 9.0, 3.0)) }
}

/// Where the page goes. Chromium's view is laid over this element; what's drawn here shows
/// while the tab has no page (an empty tab, or no Chromium) or the app draws in its place
/// (History, a failed load). `over` is drawn across the top of the area, over the page: the
/// top toolbar's suggestions.
pub fn page_area(s: &State, p: &Palette, radius: f32, square_top: bool, over: Option<El>) -> El {
    let tab = s.active();
    let content = match tab {
        Some(tab) if tab.url == HISTORY_URL => Some(history_page(s, p)),
        Some(tab) if tab.url == EXTENSIONS_URL => Some(extensions_page(s, p)),
        Some(tab) if tab.url == SETTINGS_URL => Some(settings_page(s, p)),
        Some(tab) => match &tab.error {
            Some(error) => Some(error_page(tab, error, p)),
            // With a page on screen, Chromium's view covers this area: draw nothing under it.
            None if s.chromium && tab.is_web() => None,
            None => Some(empty_page(s, tab, p)),
        },
        None => None,
    };
    col()
        .key("page")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .center()
        .children(content)
        .children(over.map(|over| over.absolute(0.0, 0.0)))
        .on_drawn_layout_tx(move |_: &mut State, rect, tx| {
            chromium::set_frame(Frame { rect, radius, square_top });
            tx.live_only();
        })
}

/// An empty tab, or a tab whose page can't show because Chromium isn't running (a headless
/// render, or the binary run outside its app bundle).
fn empty_page(s: &State, tab: &Tab, p: &Palette) -> El {
    let waiting = tab.is_web();
    let headline = if waiting { tab.label() } else { "New Tab".to_owned() };
    let hint = match (&s.notice, waiting) {
        (Some(notice), _) => notice.clone(),
        (None, true) => tab.url.clone(),
        (None, false) => "Type a search or an address to begin.".into(),
    };
    col()
        .key("empty")
        .center()
        .gap(10.0)
        .child(text(headline).font(TextStyle::new(28.0, 600.0).lh(34.0).tracking(-0.5)).color(p.text))
        .child(text(hint).font(TextStyle::new(14.0, 400.0).lh(20.0)).color(p.muted))
}
