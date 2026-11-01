//! Where the app's bundled Potato Bot folders (`potato-brain`, `potato-sandbox`) are.
//!
//! The Tauri bundle lists them as `resources/potato-brain/...` and `resources/potato-sandbox/...`, and an installed app puts
//! them under `<resource dir>/resources/<name>`; a dev run may have them directly under `<resource dir>/<name>`. Both are
//! tried, and a folder counts only if its marker file or folder is inside (a half-copied folder is not used). When neither
//! is there, the callers fall back to the source tree of a dev build.

use std::path::{Path, PathBuf};

/// `path` without the Windows extended-length prefix: `\\?\C:\x` becomes `C:\x` and `\\?\UNC\host\share` becomes
/// `\\host\share`. Anything else comes back unchanged.
///
/// Tauri builds its resource folder from a canonicalised exe path, which on Windows is the `\\?\C:\...` form.
/// Node cannot run a script named like that (`EISDIR: lstat 'C:'`, exit 1 at once), so the brain would crash on
/// every start. A folder handed to a child process goes through here first.
pub fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        // Drive-letter paths only: `\\?\Volume{...}` has no plain spelling.
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The places a bundled folder can be, relative to the resource dir, best first.
pub fn candidates(name: &str) -> [String; 2] {
    [format!("resources/{name}"), name.to_string()]
}

/// The first candidate that exists and has `marker` inside. `resolve` turns a relative name into a path.
pub fn find(name: &str, marker: &str, resolve: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    candidates(name).iter().filter_map(|rel| resolve(rel)).find(|dir| dir.join(marker).exists()).map(|dir| without_verbatim_prefix(&dir))
}

/// The installed copy of `name` (looked up through the app's resource dir).
#[cfg(not(test))]
pub fn installed(app: &tauri::AppHandle, name: &str, marker: &str) -> Option<PathBuf> {
    use tauri::{path::BaseDirectory, Manager};
    find(name, marker, |rel| app.path().resolve(rel, BaseDirectory::Resource).ok())
}
