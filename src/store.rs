//! Small files in the app's support folder, written off the UI thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};

/// The folder everything the app keeps is in: `~/Library/Application Support/<app name>` on
/// macOS, `$XDG_DATA_HOME/<app name>` (`~/.local/share/<app name>`) on Linux. The name is the
/// bundle executable's, so a copy of the app under another name (a build to test with) keeps
/// a profile of its own.
pub fn folder() -> PathBuf {
    // RUNE_BROWSER_PROFILE puts everything somewhere else: a throwaway profile, or a test's.
    if let Some(folder) = std::env::var_os("RUNE_BROWSER_PROFILE").filter(|folder| !folder.is_empty()) {
        return PathBuf::from(folder);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let name = std::env::current_exe().ok().and_then(|exe| exe.file_name().map(|name| name.to_string_lossy().into_owned()));
    let name = name.filter(|name| name.starts_with("Rune Browser")).unwrap_or_else(|| "Rune Browser".into());
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support").join(name);
    }
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|data| data.is_absolute()).unwrap_or_else(|| home.join(".local/share"));
    data.join(name)
}

/// A file in [`folder`].
pub fn path(name: &str) -> PathBuf {
    folder().join(name)
}

/// Writes `bytes` to `path` now, through a temporary file so a crash can't leave half of one.
pub fn write(path: &Path, bytes: &[u8]) {
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// Writes `bytes` to `path` on a worker thread, so the UI thread never waits for the disk.
/// Writes to one file queued while the worker is busy collapse into the newest.
pub fn write_later(path: PathBuf, bytes: Vec<u8>) {
    /// A file to write: where, and what.
    type Job = (PathBuf, Vec<u8>);
    static WRITER: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (sender, receiver) = channel::<Job>();
        std::thread::spawn(move || {
            while let Ok(first) = receiver.recv() {
                let mut latest = vec![first];
                while let Ok((path, bytes)) = receiver.try_recv() {
                    latest.retain(|(other, _)| *other != path);
                    latest.push((path, bytes));
                }
                for (path, bytes) in latest {
                    write(&path, &bytes);
                }
            }
        });
        Mutex::new(sender)
    });
    if let Ok(sender) = writer.lock() {
        let _ = sender.send((path, bytes));
    }
}
