//! The Preview tab of Simple mode: the page the agent made, read from the agent's COPY of the
//! project. v0 templates make ONE html file, so the file is shown as it is (no dev server).

use std::path::{Path, PathBuf};

/// Larger pages are not shown here (the UI says so and offers "Open it").
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

fn is_html(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
}

/// The page to show: `tool.html`, else `index.html`, else the most recently changed html file at
/// the top of `dir`. Never looks inside `.git` or `.puppet-master`.
pub fn find_preview(dir: &Path) -> Option<PathBuf> {
    for name in ["tool.html", "index.html"] {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false) && is_html(&e.path()))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PreviewFile {
    pub name: String,
    /// `None` when the page is larger than [`MAX_BYTES`].
    pub html: Option<String>,
}

pub fn read_preview(dir: &Path) -> Option<PreviewFile> {
    let path = find_preview(dir)?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(true);
    let html = if too_big { None } else { std::fs::read_to_string(&path).ok() };
    Some(PreviewFile { name, html })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pmprev-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tool_html_wins_then_index_then_any_html() {
        let dir = temp("order");
        assert!(find_preview(&dir).is_none());
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(find_preview(&dir).is_none());
        std::fs::write(dir.join("other.html"), "<p>o</p>").unwrap();
        assert_eq!(find_preview(&dir).unwrap().file_name().unwrap(), "other.html");
        std::fs::write(dir.join("index.html"), "<p>i</p>").unwrap();
        assert_eq!(find_preview(&dir).unwrap().file_name().unwrap(), "index.html");
        std::fs::write(dir.join("tool.html"), "<p>t</p>").unwrap();
        assert_eq!(read_preview(&dir).unwrap(), PreviewFile { name: "tool.html".into(), html: Some("<p>t</p>".into()) });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_over_the_cap_is_named_but_not_read() {
        let dir = temp("big");
        std::fs::write(dir.join("tool.html"), vec![b'a'; (MAX_BYTES + 1) as usize]).unwrap();
        let file = read_preview(&dir).unwrap();
        assert_eq!(file.name, "tool.html");
        assert!(file.html.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
