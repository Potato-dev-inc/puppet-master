//! What a standby worker is keyed by: everything fixed when it is spawned. A run may claim a
//! standby only when its own key is equal, so a standby never runs with options it was not
//! started with.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PoolKey {
    pub agent_type: String,
    /// Canonical project path (see `canonical_project`).
    pub project: String,
    /// Model requested at spawn. Standbys are started with the CLI's default model, so a run that
    /// names a model never matches one.
    pub model: Option<String>,
    pub read_only: bool,
    pub allow_shell: bool,
    pub headless: bool,
}

impl PoolKey {
    pub fn new(
        agent_type: &str,
        project: &str,
        model: Option<&str>,
        read_only: bool,
        allow_shell: bool,
        headless: bool,
    ) -> Self {
        Self {
            agent_type: agent_type.to_ascii_lowercase(),
            project: canonical_project(project),
            model: model
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            read_only,
            allow_shell,
            headless,
        }
    }

    /// Short label for logs and the status view.
    pub fn label(&self) -> String {
        format!(
            "{}{} {}",
            self.agent_type,
            if self.headless { " (headless)" } else { "" },
            self.project
        )
    }
}

/// The project path in one spelling, so `C:\Work\App`, `c:/work/app/` and `\\?\C:\Work\App` match.
/// Case is folded on Windows only, where paths are case-insensitive.
pub fn canonical_project(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/");
    if let Some(stripped) = text.strip_prefix("//?/") {
        text = stripped.to_string();
    }
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    if cfg!(windows) {
        text = text.to_lowercase();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_project_are_one_key() {
        let a = PoolKey::new("cursor_agent", "C:\\Work\\App", None, false, false, false);
        let b = PoolKey::new("CURSOR_AGENT", "c:/work/app/", None, false, false, false);
        if cfg!(windows) {
            assert_eq!(a, b);
            assert_eq!(
                a,
                PoolKey::new("cursor_agent", "\\\\?\\C:\\Work\\App", None, false, false, false)
            );
        }
        assert_eq!(canonical_project("/"), "/");
    }

    #[test]
    fn every_spawn_option_is_part_of_the_key() {
        let base = PoolKey::new("claude", "/p", None, false, false, true);
        assert_ne!(base, PoolKey::new("claude", "/p", Some("opus"), false, false, true));
        assert_ne!(base, PoolKey::new("claude", "/p", None, true, false, true));
        assert_ne!(base, PoolKey::new("claude", "/p", None, false, true, true));
        assert_ne!(base, PoolKey::new("claude", "/p", None, false, false, false));
        assert_ne!(base, PoolKey::new("claude", "/q", None, false, false, true));
        assert_eq!(
            base,
            PoolKey::new("claude", "/p", Some("  "), false, false, true),
            "a blank model is no model"
        );
    }
}
