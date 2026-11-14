//! The browser's model: spaces and tabs, and what commands and Chromium's reports do to them.


use std::collections::HashMap;

use rune::{Image, ImageCache, TextEdit, Tx, Waker, WindowHandle};

use crate::address;
use crate::chromium::{self, Command, Event, TabId};
use crate::extensions::{self, Extension};
use crate::settings::{self, Settings};
use crate::history::{self, History, Visit};
use crate::session::{self, Session};

/// The address of the History tab, which the app draws itself.
pub const HISTORY_URL: &str = "rune://history";
/// The address of the Extensions tab.
pub const EXTENSIONS_URL: &str = "rune://extensions";
/// The address of the Settings tab.
pub const SETTINGS_URL: &str = "rune://settings";
/// How many pages the address field suggests.
const SUGGESTIONS: usize = 6;

/// Where the tabs are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Tabs down the side, the page filling the rest.
    #[default]
    Sidebar,
    /// Tabs along the top, a toolbar over the page.
    TopTabs,
}

/// Whether Chromium's reports are printed (the RUNE_BROWSER_LOG environment variable).
pub fn logging() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUNE_BROWSER_LOG").is_some())
}

pub struct Tab {
    pub id: TabId,
    /// The space it lists in. A pinned tab shows in every space.
    pub space: usize,
    pub pinned: bool,
    /// Empty for a new tab nothing was loaded in yet.
    pub url: String,
    pub title: String,
    pub favicon: Option<Image>,
    /// The favicon's PNG, kept to save with the session.
    pub favicon_png: Vec<u8>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Why the page isn't showing, when its load failed or its process died.
    pub error: Option<String>,
}

/// The find bar, searching one tab's page.
pub struct Find {
    pub tab: TabId,
    pub edit: TextEdit,
    /// How many matches the page has, and which is selected (from 1).
    pub count: i32,
    pub active: i32,
}

/// A site's request the person has yet to answer.
pub struct Prompt {
    pub id: u64,
    pub tab: TabId,
    pub origin: String,
    /// What it asks for, to follow "wants to".
    pub what: String,
}

impl Tab {
    fn new(id: TabId, space: usize, url: &str) -> Tab {
        Tab {
            id,
            space,
            pinned: false,
            url: url.to_owned(),
            title: String::new(),
            favicon: None,
            favicon_png: Vec::new(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            error: None,
        }
    }

    /// A page the app draws itself, with no Chromium page behind it.
    pub fn is_internal(&self) -> bool {
        self.url.starts_with("rune://")
    }

    /// Whether Chromium has (or will have) a page for this tab.
    pub fn is_web(&self) -> bool {
        !self.is_blank() && !self.is_internal()
    }

    pub fn is_blank(&self) -> bool {
        self.url.is_empty()
    }

    /// What the tab is called in the list: its title, else its site, else "New Tab".
    pub fn label(&self) -> String {
        if self.url == HISTORY_URL {
            "History".into()
        } else if self.url == EXTENSIONS_URL {
            "Extensions".into()
        } else if self.url == SETTINGS_URL {
            "Settings".into()
        } else if !self.title.is_empty() {
            self.title.clone()
        } else if self.is_blank() {
            "New Tab".into()
        } else {
            // A file has no site: its path names it.
            let (site, rest) = address::display(&self.url);
            if site.is_empty() { rest } else { site }
        }
    }
}

pub struct Space {
    pub name: String,
    /// The tab on screen while this space is.
    pub active: Option<TabId>,
}

pub struct State {
    pub tabs: Vec<Tab>,
    pub spaces: Vec<Space>,
    /// The space on screen.
    pub space: usize,
    pub layout: Layout,
    pub address: TextEdit,
    /// Downloads in progress.
    pub downloads: Vec<u32>,
    /// Chromium is up and can open pages.
    pub chromium: bool,
    /// Why there is no Chromium, when it failed to start.
    pub notice: Option<String>,
    pub history: History,
    pub find: Option<Find>,
    /// Requests from sites, oldest first.
    pub prompts: Vec<Prompt>,
    /// The tab whose page fills the screen (a video's full-screen mode).
    pub fullscreen: Option<TabId>,
    /// The suggestion the arrow keys chose, and the text it was chosen for: typing on
    /// makes it stale.
    picked: Option<(usize, String)>,
    /// The tab being dragged, and where in its list the drag began.
    drag: Option<(TabId, usize)>,
    /// The window went full screen for a page and returns when the page is done.
    restore_window: bool,
    window: Option<WindowHandle>,
    pub settings: Settings,
    /// What the last clearing on the Settings page did, to say so there.
    pub settings_note: Option<&'static str>,
    /// The installed extensions, as Chromium's manager last reported them, and their icons.
    pub extensions: Vec<Extension>,
    pub extension_icons: HashMap<String, Image>,
    images: Option<ImageCache>,
    next_id: TabId,
}

impl State {
    pub fn new() -> State {
        let mut state = State {
            tabs: Vec::new(),
            spaces: vec![Space { name: "Personal".into(), active: None }],
            space: 0,
            layout: Layout::default(),
            address: TextEdit::new(""),
            downloads: Vec::new(),
            chromium: false,
            notice: None,
            history: History::default(),
            find: None,
            prompts: Vec::new(),
            fullscreen: None,
            picked: None,
            drag: None,
            restore_window: false,
            window: None,
            settings: Settings::default(),
            settings_note: None,
            extensions: Vec::new(),
            extension_icons: HashMap::new(),
            images: None,
            next_id: 1,
        };
        let first = state.push_tab(0, "", None);
        state.spaces[0].active = Some(first);
        state.address.focused = true;
        state
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.images = Some(ImageCache::new(waker));
    }

    pub fn set_window(&mut self, window: WindowHandle) {
        self.window = Some(window);
    }

    // Reading

    pub fn active_id(&self) -> Option<TabId> {
        self.spaces.get(self.space).and_then(|space| space.active)
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active_id().and_then(|id| self.tab(id))
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn pinned(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().filter(|tab| tab.pinned)
    }

    /// The current space's own tabs, in order.
    pub fn listed(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().filter(|tab| !tab.pinned && tab.space == self.space)
    }

    /// Every tab reachable in the current space: pinned first, as they are drawn.
    fn reachable(&self) -> Vec<TabId> {
        self.pinned().chain(self.listed()).map(|tab| tab.id).collect()
    }

    // Tabs

    fn push_tab(&mut self, space: usize, url: &str, after: Option<TabId>) -> TabId {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new(id, space, url);
        match after.and_then(|after| self.tabs.iter().position(|tab| tab.id == after)) {
            Some(index) => self.tabs.insert(index + 1, tab),
            None => self.tabs.push(tab),
        }
        id
    }

    pub fn new_tab(&mut self, tx: &mut Tx) {
        // One empty tab is enough: go back to it instead of opening another.
        let blank = self.listed().find(|tab| tab.is_blank()).map(|tab| tab.id);
        let id = blank.unwrap_or_else(|| self.push_tab(self.space, "", None));
        self.select(id, tx);
    }

    /// Opens `url` in a new tab of the current space, after `after` when given.
    pub fn open(&mut self, url: &str, background: bool, after: Option<TabId>, tx: &mut Tx) {
        let space = after.and_then(|id| self.tab(id)).filter(|tab| !tab.pinned).map_or(self.space, |tab| tab.space);
        let id = self.push_tab(space, url, after);
        if background {
            // Its page loads once it's looked at; until then the list shows its site.
            self.save();
        } else {
            self.select(id, tx);
        }
    }

    /// Puts `id` on screen, opening its page if it has none yet.
    pub fn select(&mut self, id: TabId, tx: &mut Tx) {
        if self.tab(id).is_some_and(|tab| tab.url == EXTENSIONS_URL) {
            self.refresh_extensions();
        }
        let Some(tab) = self.tab(id) else { return };
        let (blank, web, url, title) = (tab.is_blank(), tab.is_web(), tab.url.clone(), tab.label());
        if !tab.pinned && tab.space != self.space {
            self.space = tab.space;
        }
        // What belonged to the tab left behind goes with it.
        if self.find.as_ref().is_some_and(|find| find.tab != id) {
            self.close_find(tx);
        }
        if let Some(full) = self.fullscreen.filter(|full| *full != id) {
            chromium::exit_fullscreen(full);
        }
        self.spaces[self.space].active = Some(id);
        if self.chromium && web && !chromium::has_page(id) {
            chromium::create_tab(id, &url);
        }
        chromium::activate(id);
        tx.set_title(title);
        if blank {
            self.focus_address();
        } else {
            self.leave_address(tx);
            if web {
                chromium::focus_page(id);
            } else {
                chromium::focus_app();
            }
        }
        self.save();
    }

    pub fn close(&mut self, id: TabId, tx: &mut Tx) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else { return };
        // The neighbour that takes its place: the next tab in its list, else the one before.
        let order = self.reachable();
        let neighbour = order.iter().position(|other| *other == id).and_then(|at| order.get(at + 1).or_else(|| at.checked_sub(1).and_then(|before| order.get(before))).copied());
        self.tabs.remove(index);
        if self.find.as_ref().is_some_and(|find| find.tab == id) {
            self.find = None;
        }
        // A closed tab's requests are refused.
        for prompt in self.prompts.iter().filter(|prompt| prompt.tab == id) {
            chromium::answer_permission(prompt.id, false);
        }
        self.prompts.retain(|prompt| prompt.tab != id);
        chromium::close_tab(id);
        for space in &mut self.spaces {
            if space.active == Some(id) {
                space.active = None;
            }
        }
        if self.active_id().is_none() {
            match neighbour {
                Some(next) => self.select(next, tx),
                None => self.new_tab(tx),
            }
        }
        self.save();
    }

    pub fn toggle_pin(&mut self, id: TabId) {
        let space = self.space;
        if let Some(tab) = self.tab_mut(id) {
            tab.pinned = !tab.pinned;
            // An unpinned tab lands in the space it was unpinned in.
            tab.space = space;
        }
        self.save();
    }

    pub fn duplicate(&mut self, id: TabId, tx: &mut Tx) {
        if let Some(url) = self.tab(id).map(|tab| tab.url.clone()).filter(|url| !url.is_empty()) {
            self.open(&url, false, Some(id), tx);
        }
    }

    /// Moves a dragged tab within its list. `steps` is how many places the pointer has
    /// travelled from where the drag began; `begin` and `end` mark the gesture's ends.
    pub fn drag_tab(&mut self, id: TabId, steps: i32, begin: bool, end: bool) {
        let order: Vec<TabId> = self.listed().map(|tab| tab.id).collect();
        let Some(at) = order.iter().position(|other| *other == id) else { return };
        if begin {
            self.drag = Some((id, at));
        }
        let Some((_, from)) = self.drag.filter(|(dragged, _)| *dragged == id) else { return };
        let to = (from as i32 + steps).clamp(0, order.len() as i32 - 1) as usize;
        if to != at {
            // The list's tabs keep their slots among all tabs; only who is in which changes.
            let mut order = order;
            order.remove(at);
            order.insert(to, id);
            let slots: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, tab)| !tab.pinned && tab.space == self.space).map(|(slot, _)| slot).collect();
            let mut moved: Vec<Option<Tab>> = self.tabs.drain(..).map(Some).collect();
            let mut picked: Vec<Tab> = Vec::with_capacity(order.len());
            for wanted in &order {
                if let Some(tab) = moved.iter_mut().find(|tab| tab.as_ref().is_some_and(|tab| tab.id == *wanted)).and_then(Option::take) {
                    picked.push(tab);
                }
            }
            let mut picked = picked.into_iter();
            self.tabs = moved
                .into_iter()
                .enumerate()
                .filter_map(|(slot, tab)| if slots.contains(&slot) { picked.next() } else { tab })
                .collect();
        }
        if end {
            self.drag = None;
            self.save();
        }
    }

    pub fn is_dragging(&self, id: TabId) -> bool {
        self.drag.is_some_and(|(dragged, _)| dragged == id)
    }

    // The address field

    /// Gives the address field the keyboard, holding the active tab's URL selected.
    pub fn focus_address(&mut self) {
        chromium::focus_app();
        let url = self.active().map(|tab| tab.url.clone()).unwrap_or_default();
        let end = url.len();
        self.address.replace_text(url);
        self.address.set_selection(0, end);
        self.address.focused = true;
    }

    fn leave_address(&mut self, tx: &mut Tx) {
        if self.address.focused {
            self.address.focused = false;
            tx.blur();
        }
    }

    /// The visited pages matching what is being typed in the address field. None until
    /// something is typed: a field just focused holds the page's own address.
    pub fn suggestions(&self) -> Vec<&Visit> {
        let typed = self.address.text.trim();
        if !self.address.focused || typed.is_empty() || self.active().is_some_and(|tab| tab.url == typed) {
            return Vec::new();
        }
        self.history.suggest(typed, SUGGESTIONS)
    }

    /// The suggestion the arrow keys chose, while it still belongs to what is typed.
    pub fn picked(&self) -> Option<usize> {
        let (index, typed) = self.picked.as_ref()?;
        (*typed == self.address.text && *index < self.suggestions().len()).then_some(*index)
    }

    /// Moves the chosen suggestion down or up. Past either end is no choice: Return then
    /// loads what was typed.
    pub fn pick(&mut self, down: bool) -> bool {
        let count = self.suggestions().len();
        if count == 0 {
            return false;
        }
        let next = match (self.picked(), down) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(index), true) => (index + 1 < count).then_some(index + 1),
            (Some(index), false) => index.checked_sub(1),
        };
        self.picked = next.map(|index| (index, self.address.text.clone()));
        true
    }

    /// Loads what the address field holds in the active tab: the chosen suggestion, else
    /// the address or search typed.
    pub fn submit_address(&mut self, tx: &mut Tx) {
        let chosen = self.picked().and_then(|index| self.suggestions().get(index).map(|visit| visit.url.clone()));
        self.picked = None;
        self.go(&chosen.unwrap_or_else(|| address::resolve(&self.address.text, self.settings.engine().url)), tx);
    }

    /// Loads `url` in the active tab.
    pub fn go(&mut self, url: &str, tx: &mut Tx) {
        let url = url.to_owned();
        if url.is_empty() {
            return;
        }
        // Chrome's own pages for these are not part of an embedded Chromium.
        for (chrome, command) in [("chrome://settings", Command::Settings), ("chrome://extensions", Command::Extensions), ("chrome://history", Command::History)] {
            if url.starts_with(chrome) {
                self.cancel_address(tx);
                return self.run(command, tx);
            }
        }
        match self.active_id() {
            Some(id) => self.load(id, &url, tx),
            None => self.open(&url, false, None, tx),
        }
    }

    fn load(&mut self, id: TabId, url: &str, tx: &mut Tx) {
        let chromium_up = self.chromium;
        let Some(tab) = self.tab_mut(id) else { return };
        tab.url = url.to_owned();
        tab.title.clear();
        tab.error = None;
        tab.favicon = None;
        tab.favicon_png.clear();
        let web = tab.is_web();
        tab.loading = chromium_up && web;
        if chromium_up && web {
            if chromium::has_page(id) {
                chromium::navigate(id, url);
            } else {
                chromium::create_tab(id, url);
            }
        }
        self.select(id, tx);
    }

    /// Escape in the address field: back to the page, the field showing its URL again.
    pub fn cancel_address(&mut self, tx: &mut Tx) {
        self.leave_address(tx);
        if let Some(id) = self.active_id().filter(|id| chromium::has_page(*id)) {
            chromium::focus_page(id);
        }
    }

    // Find in page

    /// Opens the find bar for the active tab's page, or returns the keyboard to it.
    pub fn open_find(&mut self) {
        let Some(id) = self.active_id().filter(|id| chromium::has_page(*id)) else { return };
        chromium::focus_app();
        self.address.focused = false;
        let find = self.find.get_or_insert_with(|| Find { tab: id, edit: TextEdit::new(""), count: 0, active: 0 });
        let end = find.edit.text.len();
        find.edit.set_selection(0, end);
        find.edit.focused = true;
    }

    /// Searches for what the find bar holds, from the top.
    pub fn find_changed(&mut self) {
        let Some(find) = &mut self.find else { return };
        if find.edit.text.is_empty() {
            find.count = 0;
            find.active = 0;
            chromium::stop_find(find.tab);
        } else {
            // A new search starts at its first match.
            find.active = 0;
            chromium::find(find.tab, &find.edit.text, true, false);
        }
    }

    /// Moves to the next or previous match.
    pub fn find_step(&mut self, forward: bool) {
        match &self.find {
            Some(find) if !find.edit.text.is_empty() => chromium::find(find.tab, &find.edit.text, forward, true),
            Some(_) => {}
            None => self.open_find(),
        }
    }

    pub fn close_find(&mut self, tx: &mut Tx) {
        let Some(find) = self.find.take() else { return };
        chromium::stop_find(find.tab);
        if find.edit.focused {
            tx.blur();
        }
        if self.active_id() == Some(find.tab) {
            chromium::focus_page(find.tab);
        }
    }

    // Requests from sites

    /// The request to answer now: the oldest from the tab on screen.
    pub fn prompt(&self) -> Option<&Prompt> {
        let active = self.active_id()?;
        self.prompts.iter().find(|prompt| prompt.tab == active)
    }

    pub fn answer_prompt(&mut self, id: u64, allow: bool) {
        chromium::answer_permission(id, allow);
        self.prompts.retain(|prompt| prompt.id != id);
    }

    // History

    /// Shows the History tab, opening it if there is none.
    pub fn show_history(&mut self, tx: &mut Tx) {
        let open = self.listed().find(|tab| tab.url == HISTORY_URL).map(|tab| tab.id);
        let id = open.unwrap_or_else(|| self.push_tab(self.space, HISTORY_URL, self.active_id()));
        self.select(id, tx);
    }

    pub fn clear_history(&mut self) {
        self.settings_note = Some("History was cleared.");
        self.history.clear();
        if self.chromium {
            self.history.save();
        }
    }

    // Settings

    pub fn show_settings(&mut self, tx: &mut Tx) {
        let open = self.listed().find(|tab| tab.url == SETTINGS_URL).map(|tab| tab.id);
        let id = open.unwrap_or_else(|| self.push_tab(self.space, SETTINGS_URL, self.active_id()));
        self.settings_note = None;
        self.select(id, tx);
    }

    /// Changes the settings with `change`, keeps them and puts them into effect.
    pub fn set(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.settings_note = None;
        if self.chromium {
            settings::save(&self.settings);
        }
        self.apply_settings();
    }

    /// Tells Chromium what the settings say.
    fn apply_settings(&self) {
        if !self.chromium {
            return;
        }
        chromium::set_downloads_folder(self.settings.downloads_folder());
        chromium::set_color_scheme(self.settings.theme.forced());
        chromium::block_third_party_cookies(self.settings.block_third_party_cookies);
        chromium::send_do_not_track(self.settings.do_not_track);
    }

    pub fn clear_cookies(&mut self) {
        chromium::clear_cookies();
        self.settings_note = Some("Cookies were deleted. Sites will ask you to sign in again.");
    }

    pub fn clear_cache(&mut self) {
        self.settings_note = Some(if chromium::clear_cache() { "The cache was emptied." } else { "Open a page first: the cache is emptied through one." });
    }

    // Extensions

    pub fn show_extensions(&mut self, tx: &mut Tx) {
        let open = self.listed().find(|tab| tab.url == EXTENSIONS_URL).map(|tab| tab.id);
        let id = open.unwrap_or_else(|| self.push_tab(self.space, EXTENSIONS_URL, self.active_id()));
        self.select(id, tx);
    }

    /// Asks Chromium for the installed extensions again. It also says when they change.
    fn refresh_extensions(&mut self) {
        if self.chromium {
            chromium::manager::list();
        }
    }

    pub fn extension(&self, id: &str) -> Option<&Extension> {
        self.extensions.iter().find(|extension| extension.id == id)
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.settings.pinned_extensions.iter().any(|pinned| pinned == id)
    }

    /// The extensions with a button in the toolbar: pinned, installed and turned on.
    pub fn pinned_extensions(&self) -> impl Iterator<Item = &Extension> {
        self.settings.pinned_extensions.iter().filter_map(|id| self.extension(id)).filter(|extension| extension.enabled)
    }

    /// What pressing an extension's button does: opens its popup under the toolbar, or its
    /// options in a tab when it has no popup.
    pub fn open_extension(&mut self, id: &str, tx: &mut Tx) {
        let Some(extension) = self.extension(id).filter(|extension| extension.enabled) else { return };
        match (extension.popup.clone(), extension.options.clone()) {
            // The popup hangs where the buttons are: the right end of the top toolbar, or
            // beside the sidebar.
            (Some(popup), _) => {
                let over = self.active().filter(|tab| tab.is_web()).map(|tab| chromium::popup::Over { tab: tab.id, url: tab.url.clone(), title: tab.label() });
                chromium::popup::open(&popup, self.layout == Layout::TopTabs, over);
            }
            (None, Some(options)) => self.open(&options, false, None, tx),
            (None, None) => {}
        }
    }

    pub fn toggle_extension_pin(&mut self, id: &str) {
        let (id, pinned) = (id.to_owned(), self.is_pinned(id));
        self.set(|settings| if pinned { settings.pinned_extensions.retain(|other| *other != id) } else { settings.pinned_extensions.push(id) });
    }

    pub fn set_extension_enabled(&mut self, id: &str, enabled: bool) {
        chromium::manager::set_enabled(id, enabled);
    }

    /// Asks Chromium to remove an extension. It confirms with a dialog of its own first.
    pub fn remove_extension(&mut self, id: &str) {
        chromium::popup::close();
        chromium::manager::remove(id);
    }

    /// Runs an entry of an extension's menu: `verb:id`.
    pub fn extension_command(&mut self, picked: &str, tx: &mut Tx) {
        let Some((verb, id)) = picked.split_once(':') else { return };
        match verb {
            "open" => self.open_extension(id, tx),
            "options" => {
                if let Some(options) = self.extension(id).and_then(|extension| extension.options.clone()) {
                    self.open(&options, false, None, tx);
                }
            }
            "listing" => {
                if let Some(listing) = self.extension(id).map(Extension::listing) {
                    self.open(&listing, false, None, tx);
                }
            }
            "pin" => self.toggle_extension_pin(id),
            "enable" => {
                if let Some(enabled) = self.extension(id).map(|extension| extension.enabled) {
                    self.set_extension_enabled(id, !enabled);
                }
            }
            "remove" => self.remove_extension(id),
            _ => {}
        }
    }

    /// Whether the window is in macOS full screen.
    fn window_is_full_screen(&self) -> bool {
        self.window.as_ref().and_then(WindowHandle::placement).is_some_and(|placement| placement.full_screen)
    }

    // Spaces

    pub fn switch_space(&mut self, space: usize, tx: &mut Tx) {
        if space >= self.spaces.len() || space == self.space {
            return;
        }
        self.space = space;
        match self.spaces[space].active.filter(|id| self.tab(*id).is_some()).or_else(|| self.listed().next().map(|tab| tab.id)) {
            Some(id) => self.select(id, tx),
            None => self.new_tab(tx),
        }
    }

    pub fn add_space(&mut self, tx: &mut Tx) {
        let name = format!("Space {}", self.spaces.len() + 1);
        self.spaces.push(Space { name, active: None });
        self.switch_space(self.spaces.len() - 1, tx);
    }

    /// Removes the current space and its tabs. The last space stays.
    pub fn remove_space(&mut self, tx: &mut Tx) {
        if self.spaces.len() < 2 {
            return;
        }
        let gone = self.space;
        for id in self.tabs.iter().filter(|tab| !tab.pinned && tab.space == gone).map(|tab| tab.id).collect::<Vec<_>>() {
            chromium::close_tab(id);
        }
        self.tabs.retain(|tab| tab.pinned || tab.space != gone);
        for tab in &mut self.tabs {
            if tab.space > gone {
                tab.space -= 1;
            }
        }
        self.spaces.remove(gone);
        // Land on the space before it, forcing the switch although the index may match.
        let target = gone.saturating_sub(1);
        self.space = usize::MAX;
        self.switch_space(target, tx);
    }

    // Commands

    pub fn toggle_layout(&mut self, tx: &mut Tx) {
        // The page jumps to its new place: easing it there would have Chromium lay the page
        // out again on every frame of the way.
        tx.instant();
        self.layout = match self.layout {
            Layout::Sidebar => Layout::TopTabs,
            Layout::TopTabs => Layout::Sidebar,
        };
        self.save();
    }

    pub fn run(&mut self, command: Command, tx: &mut Tx) {
        let active = self.active_id();
        match command {
            Command::NewTab => self.new_tab(tx),
            Command::CloseTab => match active {
                Some(id) => self.close(id, tx),
                None => tx.close_window(),
            },
            Command::FocusAddress => self.focus_address(),
            Command::Reload | Command::HardReload => {
                if let Some(id) = active {
                    chromium::reload(id, command == Command::HardReload);
                }
            }
            Command::Stop => {
                if let Some(id) = active {
                    chromium::stop(id);
                }
            }
            Command::Back => {
                if let Some(id) = active {
                    chromium::back(id);
                }
            }
            Command::Forward => {
                if let Some(id) = active {
                    chromium::forward(id);
                }
            }
            Command::NextTab | Command::PreviousTab => {
                let order = self.reachable();
                let Some(at) = active.and_then(|id| order.iter().position(|other| *other == id)) else { return };
                let step = if command == Command::NextTab { 1 } else { order.len() - 1 };
                self.select(order[(at + step) % order.len()], tx);
            }
            Command::SelectTab(index) => {
                if let Some(id) = self.reachable().get(index).copied() {
                    self.select(id, tx);
                }
            }
            Command::LastTab => {
                if let Some(id) = self.reachable().last().copied() {
                    self.select(id, tx);
                }
            }
            Command::ToggleSidebar => self.toggle_layout(tx),
            Command::DevTools => {
                if let Some(id) = active {
                    chromium::show_dev_tools(id);
                }
            }
            Command::Find => self.open_find(),
            Command::FindNext => self.find_step(true),
            Command::FindPrevious => self.find_step(false),
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => {
                if let Some(id) = active {
                    chromium::zoom(id, match command {
                        Command::ZoomIn => 1,
                        Command::ZoomOut => -1,
                        _ => 0,
                    });
                }
            }
            Command::Print => {
                if let Some(id) = active {
                    chromium::print(id);
                }
            }
            Command::History => self.show_history(tx),
            Command::Extensions => self.show_extensions(tx),
            Command::Settings => self.show_settings(tx),
        }
    }

    // Chromium's reports

    pub fn on_event(&mut self, event: Event, tx: &mut Tx) {
        // RUNE_BROWSER_LOG=1 prints what Chromium reports, to see why a page misbehaves.
        if logging() && !matches!(event, Event::Favicon(..)) {
            eprintln!("rune-browser: {event:?}");
        }
        match event {
            Event::Ready => {
                self.chromium = true;
                self.notice = None;
                self.apply_settings();
                chromium::manager::start();
                // The tab on screen gets its page now; the others when they're looked at.
                if let Some(id) = self.active_id() {
                    self.select(id, tx);
                }
            }
            Event::Title(id, title) => {
                let active = self.active_id() == Some(id);
                let mut titled = None;
                if let Some(tab) = self.tab_mut(id) {
                    tab.title = title;
                    titled = Some((tab.url.clone(), tab.title.clone()));
                    if active {
                        tx.set_title(tab.label());
                    }
                }
                if let Some((url, title)) = titled {
                    self.history.set_title(&url, &title);
                }
                self.save();
            }
            Event::Address(id, url) => {
                if let Some(tab) = self.tab_mut(id)
                    && tab.url != url
                {
                    // Another page: its icon is another one too.
                    tab.url = url.clone();
                    tab.favicon = None;
                    tab.favicon_png.clear();
                    self.history.record(&url, history::now());
                }
                self.save();
            }
            Event::Loading { tab: id, loading, can_go_back, can_go_forward } => {
                if let Some(tab) = self.tab_mut(id) {
                    tab.loading = loading;
                    tab.can_go_back = can_go_back;
                    tab.can_go_forward = can_go_forward;
                    if loading {
                        // A new attempt: whatever failed before is behind it.
                        tab.error = None;
                    }
                }
            }
            Event::Favicon(id, png) => {
                let image = self.images.as_ref().map(|images| images.load(png.clone()));
                if let Some(tab) = self.tab_mut(id) {
                    tab.favicon = image;
                    tab.favicon_png = png;
                }
                self.save();
            }
            Event::Extensions(json) => {
                self.extensions = extensions::parse(&json, &crate::store::folder().join("Default").join("Extensions"));
                let images = self.images.as_ref();
                self.extension_icons = self.extensions.iter().filter(|extension| !extension.icon.is_empty()).filter_map(|extension| Some((extension.id.clone(), images?.load(extension.icon.clone())))).collect();
            }
            Event::Navigate(url) => self.go(&url, tx),
            Event::OpenTab { opener, url, background } => self.open(&url, background, Some(opener), tx),
            Event::Command(command) => self.run(command, tx),
            Event::PageFocused => self.leave_address(tx),
            Event::FindResult { tab, count, active } => {
                if let Some(find) = self.find.as_mut().filter(|find| find.tab == tab) {
                    // Each report may know only one of the two (the other is -1 or 0).
                    if count >= 0 {
                        find.count = count;
                    }
                    if active > 0 {
                        find.active = active;
                    }
                    if find.count == 0 {
                        find.active = 0;
                    }
                }
            }
            Event::Fullscreen(id, on) => {
                // The page jumps to the whole window, and the window to the whole screen
                // unless it's there already.
                tx.instant();
                if on {
                    self.fullscreen = Some(id);
                    if !self.window_is_full_screen() {
                        self.restore_window = true;
                        tx.toggle_full_screen();
                    }
                } else if self.fullscreen.take().is_some() && std::mem::take(&mut self.restore_window) {
                    tx.toggle_full_screen();
                }
            }
            Event::LoadFailed { tab: id, url, error } => {
                if let Some(tab) = self.tab_mut(id) {
                    tab.loading = false;
                    tab.error = Some(if error.is_empty() { format!("{url} could not be loaded.") } else { error });
                }
            }
            Event::Crashed(id) => {
                if let Some(tab) = self.tab_mut(id) {
                    tab.loading = false;
                    tab.error = Some("The page stopped unexpectedly.".into());
                }
            }
            Event::Permission { id, tab, origin, what } => self.prompts.push(Prompt { id, tab, origin, what }),
            Event::Download { id, done } => {
                self.downloads.retain(|other| *other != id);
                if !done {
                    self.downloads.push(id);
                }
            }
        }
    }

    // The session

    pub fn session(&self) -> Session {
        // Empty tabs aren't worth keeping, so tabs are numbered among the ones that are.
        let kept: Vec<&Tab> = self.tabs.iter().filter(|tab| tab.is_web()).collect();
        let index_of = |id: TabId| kept.iter().position(|tab| tab.id == id);
        Session {
            layout: match self.layout {
                Layout::Sidebar => session::SIDEBAR.into(),
                Layout::TopTabs => session::TOP_TABS.into(),
            },
            space: self.space,
            spaces: self.spaces.iter().map(|space| session::Space { name: space.name.clone(), active: space.active.and_then(index_of) }).collect(),
            tabs: kept
                .iter()
                .map(|tab| session::Tab {
                    space: tab.space,
                    pinned: tab.pinned,
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    favicon: session::to_hex(&tab.favicon_png),
                })
                .collect(),
        }
    }

    /// Replaces the tabs and spaces with a saved session's.
    pub fn restore(&mut self, saved: Session, tx: &mut Tx) {
        if saved.spaces.is_empty() {
            return;
        }
        self.layout = if saved.layout == session::TOP_TABS { Layout::TopTabs } else { Layout::Sidebar };
        self.tabs.clear();
        let mut ids = Vec::with_capacity(saved.tabs.len());
        for tab in &saved.tabs {
            let id = self.push_tab(tab.space.min(saved.spaces.len() - 1), &tab.url, None);
            ids.push(id);
            let png = session::from_hex(&tab.favicon);
            let image = (!png.is_empty()).then(|| self.images.as_ref().map(|images| images.load(png.clone()))).flatten();
            if let Some(restored) = self.tab_mut(id) {
                restored.pinned = tab.pinned;
                restored.title = tab.title.clone();
                restored.favicon = image;
                restored.favicon_png = png;
            }
        }
        self.spaces = saved.spaces.iter().map(|space| Space { name: space.name.clone(), active: space.active.and_then(|index| ids.get(index).copied()) }).collect();
        // Force the switch: the index may be the one already current.
        self.space = usize::MAX;
        self.switch_space(saved.space.min(self.spaces.len() - 1), tx);
    }

    /// Fills the browser with tabs like the Paper design's, for headless renders.
    pub fn restore_demo(&mut self) {
        self.tabs.clear();
        self.spaces = vec![Space { name: "Studio".into(), active: None }, Space { name: "Home".into(), active: None }, Space { name: "Travel".into(), active: None }];
        self.space = 0;
        for (url, title) in [
            ("https://linear.example/roadmap", "Roadmap — Q1 planning"),
            ("https://git.example/rune-browser/pulls", "rune-browser · Pull requests"),
            ("https://trails.example/jotunheimen", "Trail conditions — Jotunheimen"),
            ("https://fieldnotes.blog/making-tools", "The quiet craft of making tools"),
            ("https://cameras.example/film", "Film cameras under 300 €"),
        ] {
            let id = self.push_tab(0, url, None);
            if let Some(tab) = self.tab_mut(id) {
                tab.title = title.into();
            }
            if title.starts_with("The quiet") {
                self.spaces[0].active = Some(id);
            }
        }
        self.address.focused = false;
        let now = history::now();
        for (url, title, minutes) in [
            ("https://fieldnotes.blog/", "Fieldnotes", 3),
            ("https://fieldnotes.blog/making-tools", "The quiet craft of making tools", 9),
            ("https://fieldnotes.blog/archive", "Archive — Fieldnotes", 95),
            ("https://cameras.example/film", "Film cameras under 300 €", 200),
            ("https://trails.example/jotunheimen", "Trail conditions — Jotunheimen", 60 * 26),
            ("https://git.example/rune-browser/pulls", "rune-browser · Pull requests", 60 * 72),
        ] {
            self.history.record(url, now - minutes * 60);
            self.history.set_title(url, title);
        }
    }

    /// Turns `id` into the History tab and shows it, without a `Tx` (for a headless scene).
    pub fn demo_history(&mut self, id: TabId) {
        self.demo_page(id, HISTORY_URL);
    }

    /// Turns `id` into one of the app's own pages and shows it, for a headless scene.
    pub fn demo_page(&mut self, id: TabId, url: &str) {
        if let Some(tab) = self.tab_mut(id) {
            tab.url = url.into();
            tab.title.clear();
        }
        self.spaces[self.space].active = Some(id);
    }

    /// [`Self::restore_demo`] with an address half typed, so its suggestions show.
    pub fn restore_demo_typing(&mut self) {
        self.restore_demo();
        self.address.replace_text("field");
        self.address.focused = true;
        self.pick(true);
    }

    /// [`Self::restore_demo`] with the find bar open and a site's request waiting.
    pub fn restore_demo_bars(&mut self) {
        self.restore_demo();
        let Some(tab) = self.active_id() else { return };
        self.find = Some(Find { tab, edit: TextEdit::new("craft"), count: 7, active: 2 });
        self.prompts.push(Prompt { id: 0, tab, origin: "https://fieldnotes.blog".into(), what: "know your location".into() });
    }

    /// Writes the session to disk, off the UI thread. Only a running browser saves: a headless
    /// render or a test must not replace the session.
    pub fn save(&self) {
        if self.chromium {
            session::save(self.session());
            self.history.save();
        }
    }

    /// The session, written before this returns: for quitting.
    pub fn save_now(&self) {
        if self.chromium {
            session::write(&self.session());
            self.history.save();
        }
    }
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, El, Env, Runtime, el};

    fn nothing(_: &State, _: &mut Cx<State>) -> El {
        el()
    }

    /// A state and a way to run handlers on it.
    fn runtime() -> Runtime<State> {
        Runtime::new(State::new(), nothing, (400.0, 300.0), Env { system_dark: false, scale: 2.0 })
    }

    fn urls(state: &State) -> Vec<&str> {
        state.listed().map(|tab| tab.url.as_str()).collect()
    }

    #[test]
    fn typing_an_address_loads_it_in_the_empty_tab() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.address.replace_text("example.com");
            s.submit_address(tx);
        });
        assert_eq!(urls(&rt.state), ["https://example.com"]);
        assert_eq!(rt.state.active().map(Tab::label).as_deref(), Some("example.com"));
    }

    #[test]
    fn a_new_tab_reuses_the_empty_one() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.new_tab(tx);
            s.new_tab(tx);
        });
        assert_eq!(rt.state.tabs.len(), 1);
    }

    #[test]
    fn closing_the_active_tab_moves_to_its_neighbour_and_the_last_leaves_an_empty_one() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://a.test", false, None, tx);
            s.open("https://b.test", false, None, tx);
            s.open("https://c.test", false, None, tx);
            let b = s.listed().find(|tab| tab.url == "https://b.test").map(|tab| tab.id).unwrap_or_default();
            s.select(b, tx);
            s.close(b, tx);
        });
        assert_eq!(rt.state.active().map(|tab| tab.url.as_str()), Some("https://c.test"));
        rt.update(|s, tx| {
            loop {
                let Some(id) = s.listed().find(|tab| !tab.is_blank()).map(|tab| tab.id) else { break };
                s.close(id, tx);
            }
            let last = s.active_id().unwrap_or_default();
            s.close(last, tx);
        });
        assert_eq!(rt.state.tabs.len(), 1);
        assert!(rt.state.active().is_some_and(Tab::is_blank));
    }

    #[test]
    fn a_page_opened_from_a_tab_lands_right_after_it() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://a.test", false, None, tx);
            s.open("https://z.test", false, None, tx);
            let a = s.listed().find(|tab| tab.url == "https://a.test").map(|tab| tab.id).unwrap_or_default();
            s.on_event(Event::OpenTab { opener: a, url: "https://popup.test".into(), background: true }, tx);
        });
        let listed: Vec<&str> = urls(&rt.state).into_iter().filter(|url| !url.is_empty()).collect();
        assert_eq!(listed, ["https://a.test", "https://popup.test", "https://z.test"]);
        assert_eq!(rt.state.active().map(|tab| tab.url.as_str()), Some("https://z.test"));
    }

    #[test]
    fn each_space_keeps_its_own_tabs_and_pinned_tabs_show_in_all() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://work.test", false, None, tx);
            s.open("https://pin.test", false, None, tx);
            let pin = s.active_id().unwrap_or_default();
            s.toggle_pin(pin);
            s.add_space(tx);
            s.open("https://play.test", false, None, tx);
        });
        assert_eq!(rt.state.space, 1);
        assert!(urls(&rt.state).contains(&"https://play.test"));
        assert!(!urls(&rt.state).contains(&"https://work.test"));
        assert_eq!(rt.state.pinned().count(), 1);
        rt.update(|s, tx| s.remove_space(tx));
        assert_eq!(rt.state.spaces.len(), 1);
        assert!(urls(&rt.state).contains(&"https://work.test"));
    }

    #[test]
    fn dragging_a_tab_moves_it_within_its_list_and_stops_at_the_ends() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            for url in ["https://a.test", "https://b.test", "https://c.test"] {
                s.open(url, false, None, tx);
            }
            let a = s.listed().find(|tab| tab.url == "https://a.test").map(|tab| tab.id).unwrap_or_default();
            s.toggle_pin(a);
            s.open("https://d.test", false, None, tx);
            let b = s.listed().find(|tab| tab.url == "https://b.test").map(|tab| tab.id).unwrap_or_default();
            s.drag_tab(b, 0, true, false);
            s.drag_tab(b, 1, false, false);
        });
        let listed = |s: &State| urls(s).into_iter().filter(|url| !url.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(listed(&rt.state), ["https://c.test", "https://b.test", "https://d.test"]);
        rt.update(|s, _| {
            let b = s.listed().find(|tab| tab.url == "https://b.test").map(|tab| tab.id).unwrap_or_default();
            // Far past the end is the end; the drag's steps count from where it began.
            s.drag_tab(b, 9, false, false);
            s.drag_tab(b, -9, false, true);
        });
        assert_eq!(listed(&rt.state)[0], "https://b.test");
        assert!(rt.state.pinned().any(|tab| tab.url == "https://a.test"));
    }

    #[test]
    fn the_arrows_choose_a_suggestion_and_return_loads_it() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.history.record("https://example.org/docs", 10);
            s.history.record("https://example.org/", 20);
            s.address.replace_text("exam");
            assert_eq!(s.suggestions().len(), 2);
            assert_eq!(s.picked(), None);
            assert!(s.pick(true));
            assert!(s.pick(true));
            assert_eq!(s.picked(), Some(1));
            s.submit_address(tx);
        });
        assert_eq!(rt.state.active().map(|tab| tab.url.as_str()), Some("https://example.org/docs"));
        rt.update(|s, tx| {
            // Typing on forgets the choice: Return loads what was typed.
            s.focus_address();
            s.address.replace_text("exam");
            s.pick(true);
            s.address.replace_text("examp");
            assert_eq!(s.picked(), None);
            s.submit_address(tx);
        });
        assert_eq!(rt.state.active().map(|tab| tab.url.as_str()), Some("https://duckduckgo.com/?q=examp"));
    }

    #[test]
    fn the_history_tab_opens_once_and_is_not_saved() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://a.test", false, None, tx);
            s.show_history(tx);
            s.show_history(tx);
        });
        assert_eq!(rt.state.tabs.iter().filter(|tab| tab.url == HISTORY_URL).count(), 1);
        assert_eq!(rt.state.active().map(Tab::label).as_deref(), Some("History"));
        assert_eq!(rt.state.session().tabs.len(), 1);
    }

    #[test]
    fn a_failed_load_shows_until_the_next_attempt() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://down.test", false, None, tx);
            let id = s.active_id().unwrap_or_default();
            s.on_event(Event::LoadFailed { tab: id, url: "https://down.test".into(), error: "ERR_NAME_NOT_RESOLVED".into() }, tx);
            assert_eq!(s.active().and_then(|tab| tab.error.as_deref()), Some("ERR_NAME_NOT_RESOLVED"));
            s.on_event(Event::Loading { tab: id, loading: true, can_go_back: false, can_go_forward: false }, tx);
        });
        assert!(rt.state.active().is_some_and(|tab| tab.error.is_none()));
    }

    #[test]
    fn a_session_round_trips() {
        let mut rt = runtime();
        rt.update(|s, tx| {
            s.open("https://a.test", false, None, tx);
            s.add_space(tx);
            s.open("https://b.test", false, None, tx);
            s.toggle_layout(tx);
        });
        let saved = rt.state.session();
        let mut restored = runtime();
        restored.update(move |s, tx| s.restore(saved, tx));
        assert_eq!(restored.state.layout, Layout::TopTabs);
        assert_eq!(restored.state.spaces.len(), 2);
        assert_eq!(restored.state.space, 1);
        assert_eq!(restored.state.active().map(|tab| tab.url.as_str()), Some("https://b.test"));
    }
}
