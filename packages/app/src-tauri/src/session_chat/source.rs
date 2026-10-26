//! How a pane is tied to the session file its agent writes. Each agent says where to look
//! (`SessionSource`, implemented under `src/tui/<agent>/`); the watcher only follows the answer.

use std::path::PathBuf;
use std::time::SystemTime;

use super::model::SessionFormat;

/// What Puppet Master knows about a pane that helps to find its session file.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneFacts {
    /// The folder the agent was started in.
    pub cwd: PathBuf,
    /// When the pane was spawned.
    pub started_at: SystemTime,
    /// The agent's own session id, once a hook has reported it.
    pub provider_session_id: Option<String>,
    /// The agent's config folder when the pane runs under a Puppet Master account; `None` = the CLI's own.
    pub home: Option<PathBuf>,
}

/// The answer to "which file is this pane writing?".
#[derive(Debug, Clone, PartialEq)]
pub enum Located {
    Found(PathBuf),
    /// No file yet (the agent has not written one). Ask again later.
    NotYet,
    /// Several files could be this pane's and nothing tells them apart yet. Ask again later.
    Ambiguous(Vec<PathBuf>),
}

/// One agent's session files: how to read them and how to find the one a pane writes.
pub trait SessionSource: Send + Sync {
    fn format(&self) -> &dyn SessionFormat;
    /// The session file this pane is writing, if it can be told yet. Called about once a second.
    fn locate(&self, pane: &PaneFacts) -> Located;
}
