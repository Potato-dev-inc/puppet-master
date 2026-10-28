//! A companion hires helpers (`run_agent` from its own pane). The pieces that belong to the companion:
//!
//! - the feed line `delegated` ("Mochi started a Codex helper in demo-a"),
//! - the rule that a helper never works in the companion's own home folder (it is the companion's notes,
//!   not a project), and
//! - the default budget of `HELPER_TURNS` turns (applied by `budgets::with_companion_default`).
//!
//! Lineage is not here: the helper's pane is nested under the companion's pane by the same hint check as any
//! other helper (`pty::pane_parent`). The hard rule that the companion never answers its helpers' cards is in
//! `guard.rs`.

use super::model::FeedKind;
use super::service::{pane_id_of, Service};

/// A helper started by a companion is held to this many turns unless the person set a limit of their own.
pub const HELPER_TURNS: u32 = 20;

/// The folder as one comparable string: forward slashes, lower case, no trailing slash.
fn plain(path: &str) -> String {
    path.trim().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Whether `path` is `home` or inside it.
pub fn inside_home(home: &str, path: &str) -> bool {
    let (home, path) = (plain(home), plain(path));
    !home.is_empty() && (path == home || path.starts_with(&format!("{home}/")))
}

impl Service {
    /// The companion whose pane is `pane_id` started a helper: one line in its feed. `agent` is the helper's
    /// agent name and `folder_name` the name (not the path) of the folder it works in.
    pub fn note_delegated(&self, pane_id: &str, agent: &str, folder_name: &str) {
        let now = self.host.now_ms();
        let profile = {
            let inner = self.inner.lock();
            inner.profiles.iter().find(|p| !p.archived && pane_id_of(&p.id) == pane_id).cloned()
        };
        let Some(profile) = profile else { return };
        let folder = folder_name.trim();
        let text = if folder.is_empty() {
            format!("{} started a {agent} helper.", profile.name)
        } else {
            format!("{} started a {agent} helper in {folder}.", profile.name)
        };
        self.feed(&profile, FeedKind::Delegated, now, &text);
    }

    /// Whether `path` is the home folder of the companion in `pane_id`, or inside it.
    pub fn is_home_path(&self, pane_id: &str, path: &str) -> bool {
        let inner = self.inner.lock();
        inner.profiles.iter().find(|p| pane_id_of(&p.id) == pane_id).is_some_and(|p| inside_home(&p.folder, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_and_what_is_inside_it_are_home() {
        let home = "C:\\Users\\you\\PuppetMaster\\companions\\mochi";
        assert!(inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi"));
        assert!(inside_home(home, "c:/users/you/puppetmaster/companions/mochi/"));
        assert!(inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\memory"));
        assert!(!inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi2"));
        assert!(!inside_home(home, "C:\\Users\\you\\projects\\demo-a"));
        assert!(!inside_home("", "C:\\anything"));
    }
}
