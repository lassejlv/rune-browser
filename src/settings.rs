//! What the person chose on the Settings page, kept in `settings.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::store;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Light or dark as macOS is.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [(Theme, &'static str); 3] = [(Theme::System, "System"), (Theme::Light, "Light"), (Theme::Dark, "Dark")];

    /// Whether the app is dark, given whether macOS is.
    pub fn dark(self, system: bool) -> bool {
        match self {
            Theme::System => system,
            Theme::Light => false,
            Theme::Dark => true,
        }
    }

    /// The scheme pages are told to prefer: `None` leaves it to macOS.
    pub fn forced(self) -> Option<bool> {
        (self != Theme::System).then_some(self == Theme::Dark)
    }
}

/// A search engine the address field can send a query to.
pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    /// `{}` takes the encoded query.
    pub url: &'static str,
}

pub const ENGINES: [Engine; 7] = [
    Engine { id: "duckduckgo", name: "DuckDuckGo", url: "https://duckduckgo.com/?q={}" },
    Engine { id: "google", name: "Google", url: "https://www.google.com/search?q={}" },
    Engine { id: "bing", name: "Bing", url: "https://www.bing.com/search?q={}" },
    Engine { id: "brave", name: "Brave Search", url: "https://search.brave.com/search?q={}" },
    Engine { id: "ecosia", name: "Ecosia", url: "https://www.ecosia.org/search?q={}" },
    Engine { id: "startpage", name: "Startpage", url: "https://www.startpage.com/do/search?q={}" },
    Engine { id: "kagi", name: "Kagi", url: "https://kagi.com/search?q={}" },
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    /// The id of one of [`ENGINES`].
    pub search: String,
    /// Where downloads go; `None` is `~/Downloads`.
    pub downloads: Option<PathBuf>,
    /// Open the tabs of the last session at launch. Pinned tabs and spaces come back
    /// either way.
    pub restore_tabs: bool,
    pub block_third_party_cookies: bool,
    pub do_not_track: bool,
    /// The ids of the extensions with a button in the toolbar, in the order they show.
    pub pinned_extensions: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: Theme::System, search: ENGINES[0].id.into(), downloads: None, restore_tabs: true, block_third_party_cookies: false, do_not_track: false, pinned_extensions: Vec::new() }
    }
}

impl Settings {
    /// The chosen search engine; the first one when the file names one this build lacks.
    pub fn engine(&self) -> &'static Engine {
        ENGINES.iter().find(|engine| engine.id == self.search).unwrap_or(&ENGINES[0])
    }

    pub fn downloads_folder(&self) -> PathBuf {
        self.downloads.clone().unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Downloads"))
    }
}

const FILE: &str = "settings.json";

/// The saved settings; the defaults for anything the file lacks.
pub fn load() -> Settings {
    std::fs::read(store::path(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn save(settings: &Settings) {
    if let Ok(json) = serde_json::to_vec_pretty(settings) {
        store::write_later(store::path(FILE), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_an_older_build_fills_in_with_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"dark","search":"gone"}"#).unwrap();
        assert_eq!(settings.theme, Theme::Dark);
        assert!(settings.restore_tabs);
        assert_eq!(settings.engine().id, "duckduckgo");
        assert!(settings.downloads_folder().ends_with("Downloads"));
    }

    #[test]
    fn theme_follows_the_system_unless_set() {
        assert!(Theme::System.dark(true));
        assert!(!Theme::Light.dark(true));
        assert!(Theme::Dark.dark(false));
        assert_eq!(Theme::System.forced(), None);
        assert_eq!(Theme::Light.forced(), Some(false));
    }
}
