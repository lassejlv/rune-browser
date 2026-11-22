# Rune Browser

A web browser for macOS and Linux. Its interface is drawn with [Rune](https://github.com/anomalyco/rune);
its pages are drawn by Chromium, embedded through [CEF](https://github.com/chromiumembedded/cef)
and the [`cef`](https://crates.io/crates/cef) crate. The design is in the Paper file "Rune Browser".

## Run

```sh
scripts/run.sh            # debug build
scripts/run.sh --release
```

The first run downloads Chromium (about 350 MB) into `.cef/`. On macOS Chromium only runs from an
app bundle, so the script builds the two binaries, assembles `target/<profile>/Rune Browser.app`
(the framework plus five helper apps) and starts it; `cargo run` alone opens the interface
without pages. On Linux the script puts `libcef.so` and Chromium's data files beside the two
binaries in `target/<profile>/` and starts the browser from there.

On Linux pages are X11 windows inside the Rune window, so the app runs on X11, or through
XWayland in a Wayland session. Building needs `cmake`, `ninja` and `libxkbcommon-dev`; running
needs the libraries Chromium does (NSS, GTK 3, `libxkbcommon-x11`) and a Vulkan or GL driver
for Rune (Mesa's software ones will do). Chromium runs without its sandbox there, which needs a
setuid helper installed as root.

```sh
cargo test
cargo run -- --snap out.png --scene demo [--theme light|dark]     # sidebar layout, headless
cargo run -- --snap out.png --scene demo-top                      # tabs on top
```

## Use

Two layouts, switched with the sidebar button or ⌘S: tabs in a sidebar, or tabs along the top.
Light and dark follow the system.

| Shortcut | |
| --- | --- |
| ⌘T, ⌘W | New tab, close tab |
| ⌘L | Address field (a search or an address; Esc returns to the page) |
| ⌘R, ⇧⌘R | Reload, reload without the cache |
| ⌘[, ⌘] | Back, forward |
| ⌃Tab, ⌃⇧Tab | Next, previous tab |
| ⌘1 … ⌘8, ⌘9 | Tab by position, last tab |
| ⌘S | Switch layout |
| ⌘F, ⌘G, ⇧⌘G | Find in page, next match, previous match |
| ⌘Y | History |
| ⌘, | Settings |
| ⌘=, ⌘-, ⌘0 | Zoom in, zoom out, actual size |
| ⌘P | Print |
| ⌥⌘I | Developer tools |

On Linux the same shortcuts take Ctrl in place of ⌘, and Alt+← and Alt+→ go back and forward.

Typing in the address field lists matching pages from history; ↑ and ↓ pick one, Return opens
it. Drag a tab to reorder it. A secondary click on a tab pins, duplicates or closes it, or
copies its link. Pinned tabs show as icon tiles in every space. Spaces are separate tab lists:
the dots at the bottom of the sidebar (or the menu at the end of the tab strip) switch between
them, `+` adds one, and a secondary click on a space's name removes it. The download button
opens the downloads folder. The `···` button opens History, Extensions and Settings.

A video or page that asks for full screen takes the whole window; Esc leaves it. A page that
asks for the camera, microphone, location or notifications shows a bar under the address field
with Allow and Block.

Tabs, spaces and the layout are saved to
`~/Library/Application Support/Rune Browser/session.json` (`~/.local/share/Rune Browser/` on
Linux) and restored at launch, and history
to `history.json` beside it. The Chromium profile is in the same folder, so cookies, local
storage, IndexedDB, service workers and the cache all outlive a restart. A restored tab loads
its page when first shown.

## Settings

`···` → Settings (⌘,): where the tabs sit, the theme (pages are told to prefer the same one), the
search engine, the downloads folder, whether the last session's tabs reopen, blocking
third-party cookies, Do Not Track, and clearing history, cookies or the cache. Kept in
`settings.json` beside the session.

Chrome's own settings page isn't used: an embedded Chromium only shows it in a window Chromium
draws itself, with Chrome's look.

## Extensions

Chromium installs and runs extensions itself. Open one in the Chrome Web Store and use its
own Add button.

The puzzle-piece button lists the installed extensions. Each has a menu: open its popup, its
options, pin it to the toolbar, turn it off, remove it. A pinned extension gets a button of its
own beside the address field (under it, in the sidebar layout); pressing it opens the
extension's popup in a small window under the toolbar, which closes when you click away or
press Esc. The Extensions page (`···` → Extensions) has the same controls in a list.

A tab an extension opens itself becomes a tab here. Removing asks first, with Chromium's own
dialog.

## Passkeys

A page that asks for a passkey gets Chromium's own dialog over the page. It offers:

- **A phone or tablet** (the QR code). macOS asks once to let the app use Bluetooth, and only
  when the app was started as an app (`scripts/run.sh`, Finder), not as a bare executable.
- **A USB security key.**
- **This Mac, with Touch ID**, when the bundle is signed (below). These passkeys are kept by
  this browser on this Mac. They don't sync.

Passkeys in iCloud Keychain aren't offered: Apple grants that to browsers on request.

### Signing

Chromium keeps Touch ID passkeys in a keychain access group, which macOS grants only to an app
signed by a developer team with a provisioning profile.

```sh
scripts/provision.sh <team id>     # once a year; Xcode must be signed in to the team
```

leaves a profile in `.signing/`, and `scripts/bundle.sh` signs with it from then on (`SIGN=0`
skips it). The first signing may ask for the login keychain password to use the certificate.
Chromium's name for the group is compiled into its framework and is one no team can be
granted, so the bundle script rewrites it, in the bundle's copy, to the team's group.

## Develop

`RUNE_BROWSER_LOG=1` prints what Chromium reports (addresses, loads, permissions, find results).
`RUNE_BROWSER_PROFILE=<folder>` keeps everything in another folder. A bundle under another name
has a profile of its own, for testing beside the copy in use:

```sh
NAME="Rune Browser Dev" scripts/bundle.sh debug
```

## How it fits together

- `src/state.rs`: tabs, spaces and commands. No drawing, no Chromium types; tested headless.
- `src/history.rs`, `src/session.rs`, `src/store.rs`: visited pages and their ranking for
  suggestions, the saved tabs, and the files both are kept in.
- `src/settings.rs`: what the Settings page keeps.
- `src/extensions.rs`: the installed extensions, as Chromium's extension manager reports them.
- `src/ui/`: the Rune view. `sidebar.rs` and `top_tabs.rs` are the two layouts, `parts.rs` what
  they share, `theme.rs` the palette, `icons.rs` the icon set.
- `src/chromium/`: the CEF host. Each tab is a Chromium browser with a native view of its own,
  inside one rounded container view laid over the page area the Rune view reports
  (`parts::page_area`). Chromium handles its own input, scrolling, menus and video there.
  `pump.rs` runs Chromium's work on Rune's main loop, `app_protocol.rs` teaches winit's
  `NSApplication` the protocol Chromium expects, `client.rs` turns Chromium's callbacks into
  `Event`s that `State::on_event` applies. `stray.rs` turns a window Chromium opens on its
  own into a tab, and `views::tend_dialogs` puts Chromium's dialogs over the page.
  `manager.rs` keeps Chrome's extensions page in a window that is never shown and asks it to
  list, turn off and remove extensions (only that page may); `popup.rs` is an extension's
  popup window.
- `src/bin/rune-browser-helper.rs`: Chromium's renderer, GPU and utility processes.
- `scripts/fetch-cef.sh`: downloads CEF. The `cef` crate's own downloader reads CEF's build index
  through a client capped at 10 MB, which the index has outgrown.

## Not there yet

- macOS only (the CEF host is AppKit code).
- Rune can't draw over a page: the page is a native view above Rune's layer. The suggestion
  list in the tabs-on-top layout clips the page from above instead.
- No iCloud Keychain passkeys (see above), no sign-in prompt for HTTP authentication, no
  handing links like `mailto:` to other apps.
- An extension under development can't be loaded from a folder.
- An extension's button shows its icon only: no badge (a blocker's count), and an extension
  that changes its icon per page shows the one from the store.
- Untested: whether a popup that acts on "the current tab" (a password manager filling the
  page) finds the tab it was opened over. Chrome tells its popups; this window is not one of
  Chrome's.
- Pinned tabs can't be reordered by dragging, and history rows have no icons.
- Debug builds keep Chromium's cookie-encryption key in the profile (`--use-mock-keychain`)
  because an unsigned build would otherwise prompt for the login keychain on every rebuild.
  Release builds use the keychain.
