//! Where Focus state lives: `<app data>/focus/state.json`, private, written atomically (`secure_file::write_private`).
//!
//! Fails open. A missing file means Focus is off. A damaged file, or one with another `v`, is renamed to
//! `state.json.bad` and read as empty, so Focus starts off and pings flow. A read error is also read as empty.

use super::model::{clamp_end, State, FILE_VERSION};
use crate::secure_file::write_private;
use std::fs;
use std::path::{Path, PathBuf};

pub const STATE_FILE: &str = "state.json";
pub const BAD_FILE: &str = "state.json.bad";

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// A store in `root` (tests use a temp folder).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store in the app data folder.
    #[cfg(not(test))]
    pub fn live() -> Self {
        Self::at(crate::app_paths::app_data_dir().join("focus"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }

    pub fn bad_path(&self) -> PathBuf {
        self.root.join(BAD_FILE)
    }

    /// The saved state, or the default (Focus off) when there is no usable file.
    pub fn load(&self) -> State {
        let path = self.state_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return State::default(),
            Err(err) => {
                tracing::warn!(error = %err, "could not read focus state; Focus starts off");
                return State::default();
            }
        };
        match serde_json::from_str::<State>(&text) {
            Ok(state) if state.v == FILE_VERSION => sanitize(state),
            _ => {
                self.set_aside();
                State::default()
            }
        }
    }

    /// Write the state atomically. The folder is created private.
    pub fn save(&self, state: &State) -> Result<(), String> {
        let text = serde_json::to_string_pretty(state).map_err(|err| format!("encode focus state: {err}"))?;
        write_private(&self.state_path(), &text)
    }

    /// Move a damaged or future-version file out of the way. A file that cannot be moved stays; the next save
    /// replaces it.
    fn set_aside(&self) {
        if let Err(err) = fs::rename(self.state_path(), self.bad_path()) {
            tracing::warn!(error = %err, "could not set aside a damaged focus state file");
        }
    }
}

/// Keeps the end time inside the 240 minute cap even when the file was edited by hand.
fn sanitize(mut state: State) -> State {
    if let Some(session) = state.session.as_mut() {
        session.ends_at_ms = clamp_end(session.started_at_ms, session.ends_at_ms);
    }
    state
}

/// A new session id: `fc-` plus 12 lowercase hex characters.
pub fn new_id() -> String {
    format!("fc-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}
