//! Which pane follows which session file, so two panes of one agent in one folder do not read the same chat.
//!
//! An agent writes its session files into a folder shared by every pane that runs in the project, and the source
//! only knows "the newest file written since this pane started". Without more, a second pane that has not said
//! anything yet would show the first pane's conversation. A `Claims` table is kept per `Watchers`: each pane's
//! follower notes the file it follows, and a pane that has no id from its agent's hooks does not take a file a
//! sibling already follows (`Follower::vet`). Chat text is never in here, only pane ids and paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

/// What one pane follows.
struct Row {
    /// Which agent's source (its address): panes of other agents in the same folder write other files.
    source: usize,
    folder: String,
    file: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct Claims {
    rows: Mutex<HashMap<String, Row>>,
}

/// A folder or file as a key: no `\\?\` prefix, one kind of slash, no trailing slash, lower case (Windows).
fn key(path: &Path) -> String {
    let text = super::source::plain_path(&path.to_string_lossy()).replace('\\', "/");
    text.trim_end_matches('/').to_lowercase()
}

impl Claims {
    /// The pane follows `file` now (`None`: it still looks for one).
    pub fn note(&self, pane: &str, source: usize, folder: &Path, file: Option<&Path>) {
        self.rows.lock().insert(pane.to_string(), Row { source, folder: key(folder), file: file.map(Path::to_path_buf) });
    }

    pub fn forget(&self, pane: &str) {
        self.rows.lock().remove(pane);
    }

    /// Another pane of the same agent in the same folder follows `file`.
    pub fn held_by_other(&self, pane: &str, source: usize, folder: &Path, file: &Path) -> bool {
        let (folder, file) = (key(folder), key(file));
        self.rows.lock().iter().any(|(other, row)| {
            other != pane && row.source == source && row.folder == folder && row.file.as_deref().is_some_and(|held| key(held) == file)
        })
    }

    /// Another pane of the same agent runs in the same folder (whether or not it found its file yet).
    pub fn has_sibling(&self, pane: &str, source: usize, folder: &Path) -> bool {
        let folder = key(folder);
        self.rows.lock().iter().any(|(other, row)| other != pane && row.source == source && row.folder == folder)
    }

    /// Another pane of the same agent in the same folder still looks for its file: a new file could be its.
    pub fn rival_searching(&self, pane: &str, source: usize, folder: &Path) -> bool {
        let folder = key(folder);
        self.rows.lock().iter().any(|(other, row)| other != pane && row.source == source && row.folder == folder && row.file.is_none())
    }
}

#[cfg(test)]
#[path = "siblings_tests.rs"]
mod tests;
