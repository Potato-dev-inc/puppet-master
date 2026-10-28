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

/// The folder as one comparable string: absolute, `.` and `..` resolved, links resolved where the folder (or its
/// nearest existing parent) exists, forward slashes, no trailing slash, and lower case on Windows (its file names
/// do not care about case). A path that does not exist yet is resolved as far as it exists, then lexically.
fn plain(path: &str) -> String {
    use std::path::{Component, Path, PathBuf};
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return String::new();
    }
    let given = Path::new(&path);
    let absolute = if given.is_absolute() { given.to_path_buf() } else { std::env::current_dir().map(|cwd| cwd.join(given)).unwrap_or_else(|_| given.to_path_buf()) };
    // `..` and `.` by text, so it works for a folder that is not there (yet).
    let mut lexical = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(lexical.components().next_back(), Some(Component::Normal(_))) {
                    lexical.pop();
                }
            }
            other => lexical.push(other.as_os_str()),
        }
    }
    // Links and 8.3 names: resolve the longest part that exists, then put the rest back.
    let mut base = lexical.clone();
    let mut rest = Vec::new();
    while !base.exists() {
        let Some(name) = base.file_name().map(|n| n.to_os_string()) else { break };
        rest.push(name);
        base.pop();
    }
    let mut resolved = base.canonicalize().unwrap_or(base);
    for name in rest.into_iter().rev() {
        resolved.push(name);
    }
    let text = resolved.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).trim_end_matches('/').to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
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
        assert!(inside_home(home, "C:/Users/you/PuppetMaster/companions/mochi/"));
        assert!(inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\memory"));
        assert!(!inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi2"));
        assert!(!inside_home(home, "C:\\Users\\you\\projects\\demo-a"));
        assert!(!inside_home("", "C:\\anything"));
        assert!(!inside_home("   ", "C:\\anything"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_names_do_not_care_about_case() {
        assert!(inside_home("C:\\Users\\you\\PuppetMaster\\companions\\mochi", "c:/users/YOU/puppetmaster/companions/MOCHI/notes"));
    }

    #[test]
    fn dot_dot_and_dot_segments_cannot_hide_a_home_path() {
        let home = "C:\\Users\\you\\PuppetMaster\\companions\\mochi";
        // Into home by way of `..`.
        assert!(inside_home(home, "C:\\Users\\you\\projects\\..\\PuppetMaster\\companions\\mochi\\memory"));
        assert!(inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\.\\notes"));
        assert!(inside_home(home, "C:/Users/you/PuppetMaster/companions/bit/../mochi"));
        // Out of home by way of `..`.
        assert!(!inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\..\\bit"));
        assert!(!inside_home(home, "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\..\\..\\..\\projects"));
        // A home written with `..` itself.
        assert!(inside_home("C:\\Users\\you\\x\\..\\PuppetMaster\\companions\\mochi", "C:\\Users\\you\\PuppetMaster\\companions\\mochi\\a"));
        // `..` at the root does not climb out.
        assert!(!inside_home(home, "C:\\..\\..\\projects"));
    }

    #[test]
    fn a_relative_path_is_taken_from_the_working_folder() {
        let cwd = std::env::current_dir().unwrap();
        let home = cwd.join("mochi-home-test").to_string_lossy().to_string();
        assert!(inside_home(&home, "mochi-home-test/memory"));
        assert!(inside_home(&home, "other/../mochi-home-test"));
        assert!(!inside_home(&home, "other"));
    }

    #[test]
    fn real_folders_are_compared_by_where_they_are() {
        let dir = std::env::temp_dir().join(format!("pm-delegate-home-{}", std::process::id()));
        let home = dir.join("Mochi");
        std::fs::create_dir_all(home.join("memory")).unwrap();
        let (home_text, inside) = (home.to_string_lossy().to_string(), home.join("memory").to_string_lossy().to_string());
        assert!(inside_home(&home_text, &inside));
        // A path to the same folder that goes through `..` out of a folder that exists.
        let detour = dir.join("Mochi").join("memory").join("..").join("memory").to_string_lossy().to_string();
        assert!(inside_home(&home_text, &detour));
        // A folder next door, and a folder in the home that is not there yet.
        let next_door = dir.join("Mochi2").to_string_lossy().to_string();
        assert!(!inside_home(&home_text, &next_door));
        assert!(inside_home(&home_text, &home.join("not").join("yet").to_string_lossy()));
        // A link into the home is the home (where the OS lets a test make one).
        let link = dir.join("link-to-home");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&home, &link).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&home, &link).is_ok();
        if made {
            assert!(inside_home(&home_text, &link.join("memory").to_string_lossy()), "a link into the home");
        }
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir(&link);
    }
}
