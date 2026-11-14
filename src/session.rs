//! The open tabs and spaces, kept in a JSON file so the next launch starts where this one ended.

use serde::{Deserialize, Serialize};

use crate::store;

pub const SIDEBAR: &str = "sidebar";
pub const TOP_TABS: &str = "top-tabs";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub layout: String,
    pub space: usize,
    pub spaces: Vec<Space>,
    pub tabs: Vec<Tab>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Space {
    pub name: String,
    /// The space's active tab, as an index into [`Session::tabs`].
    pub active: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Tab {
    pub space: usize,
    pub pinned: bool,
    pub url: String,
    pub title: String,
    /// The favicon's PNG bytes in hex, so a restored tab has its icon before its page loads.
    #[serde(default)]
    pub favicon: String,
}

impl Session {
    /// Drops the tabs that aren't pinned, for a launch that starts fresh.
    pub fn forget_tabs(&mut self) {
        self.tabs.retain(|tab| tab.pinned);
        for space in &mut self.spaces {
            space.active = None;
        }
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn from_hex(hex: &str) -> Vec<u8> {
    let digits = hex.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return Vec::new();
    }
    digits.chunks(2).map(|pair| std::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect::<Option<Vec<u8>>>().unwrap_or_default()
}

const FILE: &str = "session.json";

/// The saved session, if there is one that reads.
pub fn load() -> Option<Session> {
    serde_json::from_slice(&std::fs::read(store::path(FILE)).ok()?).ok()
}

/// Writes `session` before returning: for quitting.
pub fn write(session: &Session) {
    if let Ok(json) = serde_json::to_vec_pretty(session) {
        store::write(&store::path(FILE), &json);
    }
}

/// Writes `session` off the UI thread.
pub fn save(session: Session) {
    if let Ok(json) = serde_json::to_vec_pretty(&session) {
        store::write_later(store::path(FILE), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_bad_hex_is_empty() {
        let bytes = [0u8, 1, 0x7f, 0xff, 0x89];
        assert_eq!(from_hex(&to_hex(&bytes)), bytes);
        assert!(from_hex("abc").is_empty());
        assert!(from_hex("zz").is_empty());
    }
}
