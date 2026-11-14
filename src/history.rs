//! The pages visited, kept across launches: what the address field suggests from, and what
//! the History tab lists.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{address, store};

const FILE: &str = "history.json";
/// The most pages remembered; the least recently visited are dropped past it.
const LIMIT: usize = 5000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    /// How many times the page was visited.
    pub visits: u32,
    /// When it was last visited, in seconds since 1970.
    pub last: u64,
}

#[derive(Default)]
pub struct History {
    visits: Vec<Visit>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Only web pages are remembered: not empty tabs, data: URLs or the browser's own pages.
fn remembered(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

impl History {
    /// The saved history, or an empty one.
    pub fn load() -> History {
        let visits = std::fs::read(store::path(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        History { visits }
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_vec(&self.visits) {
            store::write_later(store::path(FILE), json);
        }
    }

    pub fn len(&self) -> usize {
        self.visits.len()
    }

    /// Notes a visit to `url` at `when`.
    pub fn record(&mut self, url: &str, when: u64) {
        if !remembered(url) {
            return;
        }
        match self.visits.iter_mut().find(|visit| visit.url == url) {
            Some(visit) => {
                visit.visits += 1;
                visit.last = when;
            }
            None => self.visits.push(Visit { url: url.to_owned(), title: String::new(), visits: 1, last: when }),
        }
        if self.visits.len() > LIMIT {
            self.visits.sort_by_key(|visit| std::cmp::Reverse(visit.last));
            self.visits.truncate(LIMIT);
        }
    }

    pub fn set_title(&mut self, url: &str, title: &str) {
        if let Some(visit) = self.visits.iter_mut().find(|visit| visit.url == url) {
            visit.title = title.to_owned();
        }
    }

    pub fn clear(&mut self) {
        self.visits.clear();
    }

    /// The pages visited most recently, newest first.
    pub fn recent(&self, limit: usize) -> Vec<&Visit> {
        let mut visits: Vec<&Visit> = self.visits.iter().collect();
        visits.sort_by_key(|visit| std::cmp::Reverse(visit.last));
        visits.truncate(limit);
        visits
    }

    /// The pages that best match what is typed, best first. Every word typed must appear in
    /// the page's address or title; a site whose name starts with what is typed ranks first,
    /// then pages visited often and lately.
    pub fn suggest(&self, typed: &str, limit: usize) -> Vec<&Visit> {
        let typed = typed.trim().to_lowercase();
        let words: Vec<&str> = typed.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let newest = self.visits.iter().map(|visit| visit.last).max().unwrap_or(0);
        let mut scored: Vec<(f32, &Visit)> = self
            .visits
            .iter()
            .filter_map(|visit| {
                let url = visit.url.to_lowercase();
                let title = visit.title.to_lowercase();
                if !words.iter().all(|word| url.contains(word) || title.contains(word)) {
                    return None;
                }
                let (site, rest) = address::display(&url);
                let place = if site.starts_with(&typed) || format!("{site}{rest}").starts_with(&typed) {
                    // The site itself has an edge over a page inside it, small enough for a
                    // page visited often to overcome.
                    if rest.is_empty() { 3.5 } else { 3.0 }
                } else if site.contains(words[0]) {
                    2.0
                } else if title.contains(words[0]) {
                    1.5
                } else {
                    1.0
                };
                // Up to one more point each for how often and how lately it was visited.
                let often = (visit.visits as f32).ln_1p() / 5.0;
                let days = newest.saturating_sub(visit.last) as f32 / 86_400.0;
                let lately = 1.0 / (1.0 + days / 7.0);
                Some((place + often.min(1.0) + lately, visit))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(limit).map(|(_, visit)| visit).collect()
    }
}

/// How long ago `then` was, in words: "now", "12 min ago", "3 h ago", "2 days ago".
pub fn ago(then: u64, now: u64) -> String {
    let seconds = now.saturating_sub(then);
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> History {
        let mut history = History::default();
        for (url, title, visits, last) in [
            ("https://github.com/", "GitHub", 40, 1_000_000),
            ("https://github.com/anomalyco/rune", "anomalyco/rune: Native UI framework", 12, 1_000_500),
            ("https://news.example/rust-2026", "What changed in Rust this year", 1, 999_000),
            ("https://gitlab.example/", "GitLab", 2, 400_000),
        ] {
            history.record(url, last);
            history.set_title(url, title);
            if let Some(visit) = history.visits.iter_mut().find(|visit| visit.url == url) {
                visit.visits = visits;
            }
        }
        history
    }

    fn urls<'a>(found: &[&'a Visit]) -> Vec<&'a str> {
        found.iter().map(|visit| visit.url.as_str()).collect()
    }

    #[test]
    fn a_site_whose_name_starts_with_what_is_typed_comes_first() {
        let history = history();
        assert_eq!(urls(&history.suggest("git", 3)), ["https://github.com/", "https://github.com/anomalyco/rune", "https://gitlab.example/"]);
    }

    #[test]
    fn every_word_must_match_in_the_address_or_the_title() {
        let history = history();
        assert_eq!(urls(&history.suggest("rune framework", 5)), ["https://github.com/anomalyco/rune"]);
        assert_eq!(urls(&history.suggest("RUST year", 5)), ["https://news.example/rust-2026"]);
        assert!(history.suggest("nothing like it", 5).is_empty());
        assert!(history.suggest("   ", 5).is_empty());
    }

    #[test]
    fn only_web_pages_are_remembered_and_a_revisit_counts() {
        let mut history = History::default();
        history.record("about:blank", 1);
        history.record("rune://history", 2);
        history.record("https://a.test/", 3);
        history.record("https://a.test/", 9);
        assert_eq!(history.len(), 1);
        assert_eq!(history.recent(5)[0].visits, 2);
        assert_eq!(history.recent(5)[0].last, 9);
    }

    #[test]
    fn times_read_as_how_long_ago() {
        assert_eq!(ago(100, 130), "now");
        assert_eq!(ago(0, 60 * 12), "12 min ago");
        assert_eq!(ago(0, 3600 * 3 + 5), "3 h ago");
        assert_eq!(ago(0, 86_400 + 5), "yesterday");
        assert_eq!(ago(0, 86_400 * 4), "4 days ago");
    }
}
