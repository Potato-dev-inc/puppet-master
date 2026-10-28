//! Saving the chat without trusting the webview. The page (HTML) or the Markdown file is made in Rust
//! (`chat_export_html`, `chat_export_markdown`) and kept here under a one-time token; the save command takes that
//! token and the path the save dialog returned, never the text. So a script in the webview cannot write chosen
//! content. The token remembers its kind: an HTML token saves `.html` or `.htm` only, a Markdown token `.md` only.
//! The path is checked here too: absolute, not a link, not a device name, and an existing target must be a plain
//! file. Both kinds have a size cap.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;

/// Largest page or file that is kept or written, in bytes.
pub const MAX_PAGE_BYTES: usize = 20 * 1024 * 1024;
/// Pages kept at once (the oldest goes first). A pane keeps at most one page of each kind.
const KEEP_PAGES: usize = 4;

const NEED_HTML: &str = "Choose a file name that ends in .html.";
const NEED_MD: &str = "Choose a file name that ends in .md.";
const GONE: &str = "This page is no longer available. Close this window and open Save as page again.";

/// What a kept text is. The token it was handed out with only saves that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Html,
    Markdown,
}

impl PageKind {
    fn extensions(self) -> &'static [&'static str] {
        match self {
            PageKind::Html => &["html", "htm"],
            PageKind::Markdown => &["md"],
        }
    }

    fn need_name(self) -> &'static str {
        match self {
            PageKind::Html => NEED_HTML,
            PageKind::Markdown => NEED_MD,
        }
    }
}

struct Held {
    token: String,
    pane_id: String,
    kind: PageKind,
    text: String,
}

/// The pages made and not yet saved.
#[derive(Default)]
pub struct Pages {
    held: Mutex<Vec<Held>>,
}

impl Pages {
    /// Keeps `text` (of `kind`) for `pane_id` and returns its one-time token. The pane's earlier text of the same
    /// kind (and its token) is dropped.
    pub fn keep(&self, pane_id: &str, kind: PageKind, text: &str) -> Result<String, String> {
        if text.len() > MAX_PAGE_BYTES {
            return Err("This chat is too big to save as one file. Pick fewer turns.".into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        let mut held = self.held.lock();
        held.retain(|page| !(page.pane_id == pane_id && page.kind == kind));
        held.push(Held { token: token.clone(), pane_id: pane_id.to_string(), kind, text: text.to_string() });
        if held.len() > KEEP_PAGES {
            held.remove(0);
        }
        Ok(token)
    }

    /// Writes the text behind `token` to `path`, which must have the token's kind's extension. The token is used up
    /// once the file is written; a path that is refused, or a write that fails, leaves it so the person can choose
    /// another place. Returns the path written.
    pub fn save(&self, token: &str, path: &str) -> Result<PathBuf, String> {
        let kind = self.held.lock().iter().find(|page| page.token == token).map(|page| page.kind).ok_or_else(|| GONE.to_string())?;
        let dest = check_dest(path, kind)?;
        let page = {
            let mut held = self.held.lock();
            let at = held.iter().position(|page| page.token == token).ok_or_else(|| GONE.to_string())?;
            held.remove(at)
        };
        match write_new_or_plain(&dest, page.text.as_bytes()) {
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

/// The place to write, or why it is refused. The name must end in the extension of `kind`.
pub fn check_dest(path: &str, kind: PageKind) -> Result<PathBuf, String> {
    check_dest_with(path, kind.extensions(), kind.need_name())
}

/// The place to write for a file whose extension must be one of `exts` (no dot, any case); `need_msg` is the
/// sentence shown when the extension is wrong. The other checks are the same for every file kind.
pub fn check_dest_with(path: &str, exts: &[&str], need_msg: &str) -> Result<PathBuf, String> {
    let dest = PathBuf::from(path.trim());
    if dest.as_os_str().is_empty() {
        return Err(need_msg.into());
    }
    if !dest.is_absolute() {
        return Err("Save the file where the save window put it: a full path is needed.".into());
    }
    if dest.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err("The path must not contain . or .. parts.".into());
    }
    let ext_ok = dest.extension().is_some_and(|e| exts.iter().any(|want| e.eq_ignore_ascii_case(want)));
    if !ext_ok {
        return Err(need_msg.into());
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
