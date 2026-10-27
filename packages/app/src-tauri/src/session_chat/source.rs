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
    /// The folder `locate` looks in for this pane, to tell the person what the view is waiting for.
    fn looking_in(&self, _pane: &PaneFacts) -> Option<PathBuf> {
        None
    }
    /// Whether a pane of this agent is read off its screen until its file has something to show. A source that
    /// someone else wrote (a definition) may point at a folder the agent never writes to; the chat must not stay
    /// empty then. Default `false`: the file is the chat, and the view waits for it.
    fn screen_until_found(&self) -> bool {
        false
    }
    /// Most bytes of the file to read in one poll (`None`: all that was added). A format whose files come from a
    /// definition someone wrote caps it, so a huge or runaway file costs a few polls, not one huge read.
    fn max_read_per_poll(&self) -> Option<u64> {
        None
    }
}

/// A folder path as a program sees it: no `\\?\` / `//?/` extended-length prefix (`\\?\UNC\h\s` is `\\h\s`).
/// Trailing separators stay; separators are not changed.
pub fn plain_path(path: &str) -> String {
    for prefix in [r"\\?\", "//?/", r"\\.\", "//./"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let unc = rest.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("UNC\\") || head.eq_ignore_ascii_case("UNC/"));
            return match (unc, rest.get(4..)) {
                (true, Some(share)) => format!(r"\\{share}"),
                _ => rest.to_string(),
            };
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::plain_path;

    #[test]
    fn the_extended_length_prefix_is_dropped() {
        assert_eq!(plain_path(r"\\?\C:\Users\you\proj"), r"C:\Users\you\proj");
        assert_eq!(plain_path("//?/C:/Users/you/proj"), "C:/Users/you/proj");
        assert_eq!(plain_path(r"\\.\C:\x"), r"C:\x");
    }

    #[test]
    fn a_unc_share_keeps_two_leading_slashes() {
        assert_eq!(plain_path(r"\\?\UNC\host\share\x"), r"\\host\share\x");
        assert_eq!(plain_path(r"\\?\unc\host\share"), r"\\host\share");
    }

    #[test]
    fn an_ordinary_path_is_unchanged() {
        assert_eq!(plain_path(r"C:\Users\you\proj\"), r"C:\Users\you\proj\");
        assert_eq!(plain_path(r"\\host\share\x"), r"\\host\share\x");
        assert_eq!(plain_path("/home/you/proj"), "/home/you/proj");
        assert_eq!(plain_path(""), "");
    }
}
