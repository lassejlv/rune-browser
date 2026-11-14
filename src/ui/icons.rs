//! The browser's icons. The first seven were generated with Quiver in the Paper file's
//! "Icons — Quiver" board and normalised there to one weight; they draw in their element's
//! colour. The last three are drawn by hand to match.

use rune::Icon;

pub static SIDEBAR: Icon = Icon {
    name: "sidebar",
    view: 512.0,
    body: r#"<path d="m131.2 66.05v379.9m-87.81-331.5c0-26.75 20.67-48.45 45.950-48.45h332.5c25.41 0 47.23 20.29 47.23 48.45v282.6c0 28.61-22.53 48.83-48.58 48.83h-330.3c-26.5 0-46.78-21.7-46.78-48.830v-282.6z" fill="none" stroke="currentColor" stroke-width="40" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub static BACK: Icon = Icon {
    name: "back",
    view: 512.0,
    body: r#"<path d="m440.6 245h-340.6l119.5-120.5c4-4.32 4.64-12.32 0.48-16.64-4.16-4.48-11.68-4.64-15.84-0.48l-138.7 138.7c-5.28 5.28-4.8 13.92 0 18.72l138.4 138.9c4.32 4.48 11.84 4.16 16-0.16 4.16-4.480 4.16-12.16-0.32-16.32l-119.2-120h340.3c5.92 0 10.08-5.44 10.08-12 0-5.76-4.8-10.24-10.080-10.24z" fill="currentColor" stroke="currentColor" stroke-width="22" stroke-linejoin="round"/>"#,
};

pub static RELOAD: Icon = Icon {
    name: "reload",
    view: 512.0,
    body: r#"<path d="m452.7 235.3c0.853 7.253 1.28 14.72 1.28 22.4 0 114.7-91.52 200.4-198 200.4-109.2 0-198.1-88.96-198.1-200.4 0-113.7 91.52-203.5 198.1-203.5 62.72 0 124.8 32.96 159.6 83.52" fill="none" stroke="currentColor" stroke-width="42" stroke-linecap="round"/><path d="m421.2 74.88v66.77h-60.59" fill="none" stroke="currentColor" stroke-width="42" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub static LOCK: Icon = Icon {
    name: "lock",
    view: 512.0,
    body: r#"<path d="m366.2 226.2h-220.9c-28.48 0-49.92 21.65-49.92 50.45v127.7c0 29.76 24 50.99 50.88 50.99h219.5c26.88 0 50.88-21.23 50.88-50.99v-127.7c0-28.8-23.04-50.45-50.45-50.45z" fill="none" stroke="currentColor" stroke-width="44" stroke-linejoin="round"/><path d="m152.9 224.7v-64.53c0-57.81 46.29-103.8 102.7-103.8 56.53 0 104.1 45.65 104.1 104.7v64.21" fill="none" stroke="currentColor" stroke-width="44" stroke-linecap="round"/>"#,
};

pub static PLUS: Icon = Icon {
    name: "plus",
    view: 512.0,
    body: r#"<path d="m435 240.7h-163v-164.1c0-8.838-7.131-15.66-15.79-15.66-8.716 0-15.6 7.01-15.6 15.66v164.1h-163.8c-8.838 0-15.3 6.827-15.3 15.18 0 8.777 7.192 15.91 15.91 15.91h163.3v163.1c0 9.143 7.314 15.85 15.42 15.85 8.472 0 15.91-7.01 15.91-16.03v-162.9h162.7c8.716 0 15.66-7.131 15.66-16.03 0-8.472-6.644-15.12-15.42-15.12z" fill="currentColor" stroke="currentColor" stroke-width="14" stroke-linejoin="round"/>"#,
};

pub static DOWNLOAD: Icon = Icon {
    name: "download",
    view: 512.0,
    body: r#"<path d="m411.3 438.8h-310.6c-7.893 0-12.37 5.76-12.37 12.16 0 7.36 5.76 12.69 13.23 12.69h308.9c7.467 0.427 13.55-5.333 13.55-12.69 0-7.253-5.227-12.16-12.69-12.16z" fill="currentColor" stroke="currentColor" stroke-width="20" stroke-linejoin="round"/><path d="m247.1 392.4c2.56 2.453 6.187 3.84 9.067 3.84 3.2 0 6.827-1.387 9.387-3.84l131-135.6c4.907-4.267 5.12-13.76 0-18.24-5.12-4.693-13.76-4.587-18.24 0.32l-108.8 113.1v-290.7c0-7.36-5.653-13.23-13.12-13.23-7.253 0.213-12.48 5.44-12.48 12.8v291.3l-109.5-113.8c-4.8-4.8-13.33-4.8-18.35 0.213-5.227 5.013-5.013 13.87 0.213 18.13l130.9 135.7z" fill="currentColor" stroke="currentColor" stroke-width="20" stroke-linejoin="round"/>"#,
};

pub static CLOSE: Icon = Icon {
    name: "close",
    view: 512.0,
    body: r#"<path d="m95.36 95.17 319.9 319.7M415.4 95.17l-320.1 319.7" fill="none" stroke="currentColor" stroke-width="48" stroke-linecap="round"/>"#,
};

/// A tab with no favicon.
pub static GLOBE: Icon = Icon {
    name: "globe",
    view: 16.0,
    body: r#"<path d="M8 1.75a6.25 6.25 0 1 0 0 12.5a6.25 6.25 0 0 0 0-12.5ZM1.75 8h12.5M8 1.75c-1.9 1.7-2.75 3.8-2.75 6.25s0.85 4.55 2.75 6.25c1.9-1.7 2.75-3.8 2.75-6.25S9.9 3.45 8 1.75Z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub static CHEVRON_DOWN: Icon = Icon {
    name: "chevron-down",
    view: 10.0,
    body: r#"<path d="M2.5 4 5 6.5 7.5 4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub static MORE: Icon = Icon {
    name: "more",
    view: 16.0,
    body: r#"<circle cx="3.5" cy="8" r="1.3" fill="currentColor"/><circle cx="8" cy="8" r="1.3" fill="currentColor"/><circle cx="12.5" cy="8" r="1.3" fill="currentColor"/>"#,
};

pub static PUZZLE: Icon = Icon {
    name: "puzzle",
    view: 16.0,
    body: r#"<path d="M6.5 2.5a1.25 1.25 0 0 1 2.5 0v1h2.75a.75.75 0 0 1 .75.75V7h1a1.25 1.25 0 0 1 0 2.5h-1v2.75a.75.75 0 0 1-.75.75H9v-1a1.25 1.25 0 0 0-2.5 0v1H3.75a.75.75 0 0 1-.75-.75V9.5h1a1.25 1.25 0 0 0 0-2.5H3V4.25a.75.75 0 0 1 .75-.75H6.5Z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
