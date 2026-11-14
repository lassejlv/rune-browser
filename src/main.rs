//! Rune Browser: a web browser whose interface is drawn with Rune and whose pages are drawn
//! by Chromium.

mod address;
mod chromium;
mod extensions;
mod history;
mod session;
mod settings;
mod state;
mod store;
mod ui;

use rune::{AppSpec, Key, KeyEvent, Tx, Waker};

use chromium::Command;
use state::State;

/// The app's shortcut for a key Rune's own controls didn't take.
fn shortcut(event: &KeyEvent) -> Option<Command> {
    let mods = event.mods;
    match &event.key {
        Key::Tab if mods.ctrl => Some(if mods.shift { Command::PreviousTab } else { Command::NextTab }),
        Key::Command(key) => match (*key, mods.shift, mods.alt) {
            ('t', false, false) => Some(Command::NewTab),
            ('w', false, false) => Some(Command::CloseTab),
            ('l', false, false) => Some(Command::FocusAddress),
            ('r', false, false) => Some(Command::Reload),
            ('r', true, false) => Some(Command::HardReload),
            ('s', false, false) => Some(Command::ToggleSidebar),
            ('[', false, false) => Some(Command::Back),
            (']', false, false) => Some(Command::Forward),
            ('[' | '{', true, false) => Some(Command::PreviousTab),
            (']' | '}', true, false) => Some(Command::NextTab),
            ('1'..='8', false, false) => Some(Command::SelectTab(*key as usize - '1' as usize)),
            ('9', false, false) => Some(Command::LastTab),
            ('i', false, true) => Some(Command::DevTools),
            ('f', false, false) => Some(Command::Find),
            ('g', false, false) => Some(Command::FindNext),
            ('g', true, false) => Some(Command::FindPrevious),
            ('p', false, false) => Some(Command::Print),
            ('y', false, false) => Some(Command::History),
            (',', false, false) => Some(Command::Settings),
            ('=' | '+', _, false) => Some(Command::ZoomIn),
            ('-', false, false) => Some(Command::ZoomOut),
            ('0', false, false) => Some(Command::ZoomReset),
            _ => None,
        },
        _ => None,
    }
}

fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    match shortcut(event) {
        Some(command) => {
            s.run(command, tx);
            true
        }
        None => false,
    }
}

fn setup(s: &mut State, waker: Waker) {
    s.set_waker(waker);
    // Before the first frame, so it is drawn in the chosen theme. A headless scene that set
    // its own keeps them.
    if s.settings == settings::Settings::default() {
        s.settings = settings::load();
    }
}

/// Tabs like the ones in the Paper design, for headless renders.
fn demo(s: &mut State) {
    s.restore_demo();
}

fn demo_top(s: &mut State) {
    s.restore_demo();
    s.layout = state::Layout::TopTabs;
}

fn typing(s: &mut State) {
    s.restore_demo_typing();
}

fn typing_top(s: &mut State) {
    s.restore_demo_typing();
    s.layout = state::Layout::TopTabs;
}

fn bars(s: &mut State) {
    s.restore_demo_bars();
}

fn bars_top(s: &mut State) {
    s.restore_demo_bars();
    s.layout = state::Layout::TopTabs;
}

fn history_tab(s: &mut State) {
    s.restore_demo();
    let id = s.listed().next().map(|tab| tab.id).unwrap_or_default();
    s.demo_history(id);
}

fn settings_tab(s: &mut State) {
    s.restore_demo();
    s.settings = settings::Settings::default();
    let id = s.listed().next().map(|tab| tab.id).unwrap_or_default();
    s.demo_page(id, state::SETTINGS_URL);
}

fn extensions_tab(s: &mut State) {
    s.restore_demo();
    s.settings = settings::Settings::default();
    let id = s.listed().next().map(|tab| tab.id).unwrap_or_default();
    s.demo_page(id, state::EXTENSIONS_URL);
    let extension = |id: &str, name: &str, enabled: bool, options: bool| extensions::Extension {
        id: id.into(),
        name: name.into(),
        version: "2026.11.1".into(),
        enabled,
        removable: true,
        options: options.then(|| format!("chrome-extension://{id}/options.html")),
        popup: Some(format!("chrome-extension://{id}/popup.html")),
        ..Default::default()
    };
    s.extensions = vec![extension("nngceckbapebfimnlniiiahkandclblb", "Bitwarden Password Manager", true, false), extension("lidfahaahiogmnlccifabccgplofocck", "Paper Snapshot", false, false), extension("ddkjiahejlhfcafbddmgiahcphecmpfh", "uBlock Origin Lite", true, true)];
    s.settings.pinned_extensions = vec!["nngceckbapebfimnlniiiahkandclblb".into(), "ddkjiahejlhfcafbddmgiahcphecmpfh".into()];
}

fn extensions_top(s: &mut State) {
    extensions_tab(s);
    s.layout = state::Layout::TopTabs;
}

fn main() {
    let spec = AppSpec::<State> {
        min_size: Some((640.0, 420.0)),
        scenes: &[("demo", demo), ("demo-top", demo_top), ("typing", typing), ("typing-top", typing_top), ("bars", bars), ("bars-top", bars_top), ("history", history_tab), ("settings", settings_tab), ("extensions", extensions_tab), ("extensions-top", extensions_top)],
        setup: Some(setup),
        on_key: Some(on_key),
        ..AppSpec::new("Rune Browser", (1280.0, 820.0), State::new, ui::view)
    };
    let app = rune::App::new(spec)
        .titlebar(ui::SIDEBAR_TITLEBAR)
        .standard_menus()
        .on_open_url(|s: &mut State, url, tx| s.open(url, false, None, tx))
        .on_close(|s: &mut State, _| s.save_now())
        .on_quit(|s: &mut State, _| {
            s.save_now();
            chromium::shutdown();
        });
    let handle = app.handle();
    app.on_ready(move |window| {
        // The first frame is on screen: only now load Chromium, which takes about a second.
        let events = handle.clone();
        let started = chromium::start(window, move |event| events.update(move |s: &mut State, tx| s.on_event(event, tx)));
        let saved = started.is_ok().then(session::load).flatten();
        let visited = started.is_ok().then(history::History::load);
        let window = window.clone();
        handle.update(move |s: &mut State, tx| {
            s.set_window(window);
            if let Some(visited) = visited {
                s.history = visited;
            }
            if let Err(reason) = started {
                s.notice = Some(format!("Pages need Chromium: {reason}."));
            }
            if let Some(mut saved) = saved {
                if !s.settings.restore_tabs {
                    saved.forget_tabs();
                }
                s.restore(saved, tx);
            }
        });
    })
    .run();
}
