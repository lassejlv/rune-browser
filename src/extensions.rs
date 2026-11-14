//! Chrome extensions. Chromium installs and runs them itself: the Chrome Web Store's own
//! button adds one. Its extension manager (`chromium::manager`) says which are installed and
//! turns them off or removes them.
//!
//! Pages are embedded without Chrome's toolbar, so the app draws the extensions' buttons
//! itself: one for each pinned extension, which opens its popup (`chromium::popup`).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Deserialize;

/// Where extensions are found.
pub const STORE: &str = "https://chromewebstore.google.com/category/extensions";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extension {
    /// The id Chromium gives it: 32 letters a–p.
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    /// Loaded from a folder, not installed from the Web Store.
    pub unpacked: bool,
    /// Whether the person may remove it (one installed by a policy can't be).
    pub removable: bool,
    /// Its icon, as a PNG; empty when it has none.
    pub icon: Vec<u8>,
    /// The addresses of its options page and of its toolbar popup.
    pub options: Option<String>,
    pub popup: Option<String>,
}

impl Extension {
    /// Its page in the Chrome Web Store.
    pub fn listing(&self) -> String {
        format!("https://chromewebstore.google.com/detail/{}", self.id)
    }
}

/// An extension as Chromium's manager reports it.
#[derive(Deserialize)]
struct Reported {
    id: String,
    name: String,
    version: String,
    enabled: bool,
    icon: Option<String>,
    options: Option<String>,
    unpacked: bool,
    removable: bool,
    /// Where an unpacked extension is.
    path: Option<String>,
}

/// The newest installed version of the extension `id` in a profile's `Extensions` folder,
/// which holds one folder per id and in it one per version.
fn installed_folder(extensions: &Path, id: &str) -> Option<PathBuf> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(extensions.join(id)).ok()?.flatten().map(|entry| entry.path()).filter(|path| path.join("manifest.json").is_file()).collect();
    // An update leaves the old version beside the new one until the next launch.
    versions.sort_by_key(|version| std::fs::metadata(version).and_then(|metadata| metadata.modified()).ok());
    versions.pop()
}

/// The toolbar popup an extension's manifest names, relative to its folder.
fn popup_in(folder: &Path) -> Option<String> {
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).ok()?).ok()?;
    let popup = ["/action/default_popup", "/browser_action/default_popup"].iter().find_map(|at| manifest.pointer(at))?.as_str()?;
    (!popup.is_empty()).then(|| popup.trim_start_matches('/').to_owned())
}

/// The PNG in a `data:image/png;base64,…` address.
fn png_of(data_url: &str) -> Vec<u8> {
    data_url.strip_prefix("data:image/png;base64,").and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok()).unwrap_or_default()
}

/// The extensions in the manager's report `json`, by name. `extensions` is the profile's
/// `Extensions` folder, where the manifests of Web Store extensions are: the report doesn't
/// say what an extension's popup is.
pub fn parse(json: &str, extensions: &Path) -> Vec<Extension> {
    let reported: Vec<Reported> = serde_json::from_str(json).unwrap_or_default();
    let mut found: Vec<Extension> = reported
        .into_iter()
        .map(|reported| {
            let folder = reported.path.as_deref().filter(|path| !path.is_empty()).map(PathBuf::from).or_else(|| installed_folder(extensions, &reported.id));
            let popup = folder.as_deref().and_then(popup_in).map(|popup| format!("chrome-extension://{}/{popup}", reported.id));
            Extension {
                popup,
                icon: reported.icon.as_deref().map(png_of).unwrap_or_default(),
                options: reported.options.filter(|options| !options.is_empty()),
                id: reported.id,
                name: reported.name,
                version: reported.version,
                enabled: reported.enabled,
                unpacked: reported.unpacked,
                removable: reported.removable,
            }
        })
        .collect();
    found.sort_by_key(|extension| extension.name.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_managers_report_and_finds_popups_in_manifests() {
        let folder = std::env::temp_dir().join(format!("rune-browser-extensions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let store = folder.join("aaaabbbbccccddddeeeeffffgggghhhh").join("1.2_0");
        let unpacked = folder.join("work");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&unpacked).unwrap();
        std::fs::write(store.join("manifest.json"), r#"{"name":"Zebra","version":"1.2","action":{"default_popup":"/popup.html"}}"#).unwrap();
        std::fs::write(unpacked.join("manifest.json"), r#"{"name":"Apfel","version":"3.0","browser_action":{"default_popup":"ui/index.html"}}"#).unwrap();
        let report = serde_json::json!([
            {"id": "aaaabbbbccccddddeeeeffffgggghhhh", "name": "Zebra", "version": "1.2", "enabled": true, "icon": "data:image/png;base64,iVBORw==", "options": "chrome-extension://aaaabbbbccccddddeeeeffffgggghhhh/options.html", "unpacked": false, "removable": true, "path": null},
            {"id": "ppppoooonnnnmmmmllllkkkkjjjjiiii", "name": "apfel", "version": "3.0", "enabled": false, "icon": "chrome://extension-icon/x", "options": null, "unpacked": true, "removable": true, "path": unpacked},
            {"id": "ccccccccccccccccccccccccccccccca", "name": "No popup", "version": "1", "enabled": true, "icon": null, "options": "", "unpacked": false, "removable": false, "path": null},
        ])
        .to_string();

        let found = parse(&report, &folder);
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(found.iter().map(|extension| extension.name.as_str()).collect::<Vec<_>>(), ["apfel", "No popup", "Zebra"]);
        assert_eq!(found[0].popup.as_deref(), Some("chrome-extension://ppppoooonnnnmmmmllllkkkkjjjjiiii/ui/index.html"));
        assert!(found[0].icon.is_empty() && !found[0].enabled && found[0].unpacked);
        assert_eq!((found[1].popup.as_deref(), found[1].options.as_deref(), found[1].removable), (None, None, false));
        assert_eq!(found[2].popup.as_deref(), Some("chrome-extension://aaaabbbbccccddddeeeeffffgggghhhh/popup.html"));
        assert_eq!(found[2].icon, [0x89, b'P', b'N', b'G']);
        assert_eq!(found[2].listing(), "https://chromewebstore.google.com/detail/aaaabbbbccccddddeeeeffffgggghhhh");
        assert!(parse("not json", &folder).is_empty());
    }
}
