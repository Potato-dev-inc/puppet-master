//! Which of a declared agent's transcript files a pane is writing: the newest file of the table's folder that was
//! written since the pane started (or the one named after the agent's own session id).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use super::format::DeclaredSessionFile;
use super::path::{allowed_roots, checked_dir, checked_file, expand, glob_matches, Names};
use super::table::{validate_session_chat, SessionChatTable};
use crate::session_chat::{Located, PaneFacts, SessionFormat, SessionSource};
use crate::tui::declarative::schema::DefError;

/// A file can be a few seconds older than the pane when the clocks or the filesystem round.
const SLACK: Duration = Duration::from_secs(5);

/// A transcript larger than this is not followed.
pub const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// Most bytes read from the transcript in one poll; a bigger backlog is read over the next polls.
pub const MAX_READ_PER_POLL: u64 = 8 * 1024 * 1024;

/// Most folder entries looked at in one `locate`.
const MAX_ENTRIES: usize = 5000;

/// One declared agent's transcript files: how to read them and how to find the one a pane writes.
pub struct DeclaredSource {
    table: SessionChatTable,
    format: DeclaredSessionFile,
}

static SOURCES: Lazy<Mutex<HashMap<String, &'static DeclaredSource>>> = Lazy::new(|| Mutex::new(HashMap::new()));

impl DeclaredSource {
    /// A source for a table that passed validation; `Err` with the problems otherwise.
    pub fn new(table: SessionChatTable) -> Result<Self, Vec<DefError>> {
        let errors = validate_session_chat(&table);
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(DeclaredSource { format: DeclaredSessionFile::new(&table), table })
    }

    /// The source for `table`, built once: `SessionSource` is handed out as `'static`, and a definition that is
    /// loaded again with the same table must not leak another one.
    pub fn shared(table: &SessionChatTable) -> Result<&'static DeclaredSource, Vec<DefError>> {
        let key = serde_json::to_string(table).unwrap_or_default();
        let mut sources = SOURCES.lock();
        if let Some(source) = sources.get(&key) {
            return Ok(source);
        }
        let source: &'static DeclaredSource = Box::leak(Box::new(DeclaredSource::new(table.clone())?));
        sources.insert(key, source);
        Ok(source)
    }

    /// `locate` with the home folder and environment given, for tests.
    pub fn locate_with(&self, pane: &PaneFacts, names: &Names<'_>) -> Located {
        let Some(dir) = expand(&self.table.dir, names) else {
            return Located::NotYet;
        };
        let roots = allowed_roots(names);
        let Some(dir) = checked_dir(&dir, &roots) else {
            return Located::NotYet;
        };
        let since = pane.started_at.checked_sub(SLACK).unwrap_or(pane.started_at);
        let id = pane.provider_session_id.as_deref().filter(|id| !id.is_empty());
        let mut best: Option<(bool, SystemTime, PathBuf)> = None;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Located::NotYet;
        };
        for entry in entries.filter_map(Result::ok).take(MAX_ENTRIES) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !glob_matches(&self.table.file_glob, &name) {
                continue;
            }
            // A symlink is followed only to a file that is still under the roots.
            let Some(real) = checked_file(&entry.path(), &roots) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&real) else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if meta.len() > MAX_FILE_BYTES || modified < since {
                continue;
            }
            let named = id.is_some_and(|id| name.contains(id));
            if best.as_ref().is_none_or(|(was_named, was_modified, _)| (named, modified) > (*was_named, *was_modified)) {
                best = Some((named, modified, real));
            }
        }
        match best {
            Some((_, _, real)) => Located::Found(PathBuf::from(crate::session_chat::plain_path(&real.to_string_lossy()))),
            None => Located::NotYet,
        }
    }
}

/// The environment variable `name`, for `{env:NAME}`.
fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

impl SessionSource for DeclaredSource {
    fn format(&self) -> &dyn SessionFormat {
        &self.format
    }

    fn locate(&self, pane: &PaneFacts) -> Located {
        let home = crate::project_path::home_dir();
        self.locate_with(pane, &Names { home: home.as_deref(), cwd: &pane.cwd, env: &env_var })
    }

    fn looking_in(&self, pane: &PaneFacts) -> Option<PathBuf> {
        let home = crate::project_path::home_dir();
        let names = Names { home: home.as_deref(), cwd: &pane.cwd, env: &env_var };
        let dir = expand(&self.table.dir, &names)?;
        // Say only a folder that is allowed: the person is told where the view looks, never somewhere it will not.
        checked_dir(&dir, &allowed_roots(&names)).map(|real| PathBuf::from(crate::session_chat::plain_path(&real.to_string_lossy())))
    }

    fn max_read_per_poll(&self) -> Option<u64> {
        Some(MAX_READ_PER_POLL)
    }
}
