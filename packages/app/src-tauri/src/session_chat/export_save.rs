//! Saving the chat page without trusting the webview. The page is made in Rust (`chat_export_html`) and kept here
//! under a one-time token; the save command takes that token and the path the save dialog returned, never the page.
//! So a script in the webview cannot write chosen content, and the path is checked here: absolute, `.html` or `.htm`,
//! not a link, not a device name, and an existing target must be a plain file. The page has a size cap.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;

/// Largest page that is kept or written, in bytes.
pub const MAX_PAGE_BYTES: usize = 20 * 1024 * 1024;
/// Pages kept at once (the oldest goes first); each is bound to the token it was handed out with.
const KEEP_PAGES: usize = 4;

const NEED_HTML: &str = "Choose a file name that ends in .html.";
const GONE: &str = "This page is no longer available. Close this window and open Save as page again.";

struct Held {
    token: String,
    pane_id: String,
    html: String,
}

/// The pages made and not yet saved.
#[derive(Default)]
pub struct Pages {
    held: Mutex<Vec<Held>>,
}

impl Pages {
    /// Keeps `html` for `pane_id` and returns its one-time token. The pane's earlier page (and its token) is dropped.
    pub fn keep(&self, pane_id: &str, html: &str) -> Result<String, String> {
        if html.len() > MAX_PAGE_BYTES {
            return Err("This chat is too big to save as one page. Pick fewer turns.".into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        let mut held = self.held.lock();
        held.retain(|page| page.pane_id != pane_id);
        held.push(Held { token: token.clone(), pane_id: pane_id.to_string(), html: html.to_string() });
        if held.len() > KEEP_PAGES {
            held.remove(0);
        }
        Ok(token)
    }

    /// Writes the page behind `token` to `path`. The token is used up once the page is written; a path that is
    /// refused, or a write that fails, leaves it so the person can choose another place. Returns the path written.
    pub fn save(&self, token: &str, path: &str) -> Result<PathBuf, String> {
        let dest = check_dest(path)?;
        let page = {
            let mut held = self.held.lock();
            let at = held.iter().position(|page| page.token == token).ok_or_else(|| GONE.to_string())?;
            held.remove(at)
        };
        match write_new_or_plain(&dest, page.html.as_bytes()) {
            Ok(()) => Ok(dest),
            Err(error) => {
                self.held.lock().push(page);
                Err(format!("Could not write the file: {error}"))
            }
        }
    }

    #[cfg(test)]
    fn count(&self) -> usize {
        self.held.lock().len()
    }
}

/// The place to write, or why it is refused.
pub fn check_dest(path: &str) -> Result<PathBuf, String> {
    let dest = PathBuf::from(path.trim());
    if dest.as_os_str().is_empty() {
        return Err(NEED_HTML.into());
    }
    if !dest.is_absolute() {
        return Err("Save the page where the save window put it: a full path is needed.".into());
    }
    if dest.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err("The path must not contain . or .. parts.".into());
    }
    let is_html = dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if !is_html {
        return Err(NEED_HTML.into());
    }
    if is_device_name(&dest) {
        return Err("That file name is reserved by the system. Choose another.".into());
    }
    match dest.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => return Err("The folder does not exist.".into()),
    }
    match std::fs::symlink_metadata(&dest) {
        Ok(meta) if meta.file_type().is_symlink() => Err("That file is a link. Choose a plain file name.".into()),
        Ok(meta) if !meta.is_file() => Err("That is not a plain file. Choose another name.".into()),
        Ok(_) => Ok(dest),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(dest),
        Err(error) => Err(format!("Could not check the place: {error}")),
    }
}

/// `CON.html`, `NUL.htm`, `COM1.html`...: names Windows treats as devices (checked on every system, they are harmless
/// elsewhere to refuse).
fn is_device_name(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n.chars().collect::<Vec<_>>().as_slice(), ['1'..='9']));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
}

/// An existing plain file is replaced; a missing one is created, and not followed if something appeared there since
/// the check (`create_new` refuses an existing name).
fn write_new_or_plain(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.len() > MAX_PAGE_BYTES {
        return Err(std::io::Error::other("the page is too big"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if dest.exists() {
        options.truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(dest)?;
    file.write_all(bytes)?;
    file.flush()
}

#[cfg(test)]
#[path = "export_save_tests.rs"]
mod tests;
