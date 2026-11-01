//! Where the app's bundled Potato Bot folders (`potato-brain`, `potato-sandbox`) are.
//!
//! The Tauri bundle lists them as `resources/potato-brain/...` and `resources/potato-sandbox/...`, and an installed app puts
//! them under `<resource dir>/resources/<name>`; a dev run may have them directly under `<resource dir>/<name>`. Both are
//! tried, and a folder counts only if its marker file or folder is inside (a half-copied folder is not used). When neither
//! is there, the callers fall back to the source tree of a dev build.

use std::path::PathBuf;

/// The places a bundled folder can be, relative to the resource dir, best first.
pub fn candidates(name: &str) -> [String; 2] {
    [format!("resources/{name}"), name.to_string()]
}

/// The first candidate that exists and has `marker` inside. `resolve` turns a relative name into a path.
pub fn find(name: &str, marker: &str, resolve: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    candidates(name).iter().filter_map(|rel| resolve(rel)).find(|dir| dir.join(marker).exists())
}

/// The installed copy of `name` (looked up through the app's resource dir).
#[cfg(not(test))]
pub fn installed(app: &tauri::AppHandle, name: &str, marker: &str) -> Option<PathBuf> {
    use tauri::{path::BaseDirectory, Manager};
    find(name, marker, |rel| app.path().resolve(rel, BaseDirectory::Resource).ok())
}
