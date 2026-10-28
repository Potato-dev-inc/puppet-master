//! The folders and variables an agent store is read from. Every store reads through `HistoryEnv`, never the
//! process, so a test can inject fake folders and a live check can stay on its fixtures.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// When set, that folder is the only home and no other environment variable is read (live checks use it).
pub const HOME_OVERRIDE_VAR: &str = "PUPPET_MASTER_HISTORY_HOME";

/// Reads one environment variable by name.
type Lookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The home folder and the variable lookup for one read of the agent stores.
#[derive(Clone)]
pub struct HistoryEnv {
    home: Option<PathBuf>,
    lookup: Lookup,
}

impl fmt::Debug for HistoryEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Folders are never printed: a debug line must not carry the account name.
        f.write_str("HistoryEnv")
    }
}

impl HistoryEnv {
    /// A fixed environment for tests and fixtures: this home (None = no home) and these variables only.
    pub fn fixed(home: Option<PathBuf>, vars: &[(&str, &str)]) -> Self {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let lookup: Lookup = Arc::new(move |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()));
        Self { home, lookup }
    }

    /// No home and no variables: a store finds nothing.
    pub fn isolated() -> Self {
        Self::fixed(None, &[])
    }

    /// The app's own environment. Under test it is empty (no home), so a test that forgets to inject finds nothing.
    #[cfg(test)]
    pub fn from_process() -> Self {
        Self::isolated()
    }

    /// The app's own environment: the process variables and the OS home folder (`HOME`, then `USERPROFILE`).
    #[cfg(not(test))]
    pub fn from_process() -> Self {
        Self::from_lookup(Arc::new(|name: &str| std::env::var(name).ok()), crate::project_path::home_dir())
    }

    /// Builds the environment from a lookup of the process variables and the process home folder. With
    /// `PUPPET_MASTER_HISTORY_HOME` set, that folder is the home and every other variable answers `None`.
    pub(crate) fn from_lookup(process: Lookup, process_home: Option<PathBuf>) -> Self {
        match (process)(HOME_OVERRIDE_VAR).filter(|folder| !folder.trim().is_empty()) {
            Some(folder) => {
                let none: Lookup = Arc::new(|_: &str| None::<String>);
                Self { home: Some(PathBuf::from(folder)), lookup: none }
            }
            None => Self { home: process_home, lookup: process },
        }
    }

    /// The home folder the stores live under, when there is one.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// One environment variable, when it is set and not empty.
    pub fn var(&self, name: &str) -> Option<String> {
        (self.lookup)(name).filter(|value| !value.is_empty())
    }
}
