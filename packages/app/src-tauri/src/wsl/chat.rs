//! What the chat view needs from a Linux pane (SPEC-n33 section 3.6 D): the pane's Linux facts, and
//! the rule that stops reading once the pane's process is gone (a read under `\\wsl.localhost` can
//! start a stopped distro, so nothing is read from Linux files after the pane ended).

use crate::session_chat::LinuxFacts;
use crate::wsl::model::PanePlace;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The WSL clock can drift from Windows' after a sleep: file times may be off by this much.
pub const CLOCK_SLACK: Duration = Duration::from_secs(60);
/// After the pane's process exits, the chat reader may still look for this long (one last read).
pub const LAST_READ_GRACE: Duration = Duration::from_secs(2);
/// Entries of long-gone panes are dropped after this.
const FORGET_AFTER: Duration = Duration::from_secs(600);

/// The chat facts of a Linux pane. `None` when the place lacks the Linux folder or the home as Windows
/// reads it: nothing is read then (never the Windows default folders).
pub fn facts_of(place: &PanePlace) -> Option<LinuxFacts> {
    if place.kind != "linux" || place.cwd.is_empty() || place.home_windows.is_empty() {
        return None;
    }
    Some(LinuxFacts {
        cwd: place.cwd.clone(),
        home_windows: PathBuf::from(&place.home_windows),
        clock_slack: CLOCK_SLACK,
    })
}

/// When each exited Linux pane was first looked at after it ended.
pub struct LastRead {
    first_seen: Mutex<BTreeMap<String, Instant>>,
}

impl LastRead {
    pub const fn new() -> Self {
        Self { first_seen: Mutex::new(BTreeMap::new()) }
    }

    /// May the chat reader look at this pane now? Always while the process runs; after it exited, for
    /// `LAST_READ_GRACE` from the first look, then never.
    pub fn allow(&self, pane_id: &str, exited: bool, now: Instant) -> bool {
        let mut seen = self.first_seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !exited {
            seen.remove(pane_id);
            return true;
        }
        seen.retain(|_, at| now.saturating_duration_since(*at) < FORGET_AFTER);
        let first = *seen.entry(pane_id.to_string()).or_insert(now);
        now.saturating_duration_since(first) < LAST_READ_GRACE
    }
}

static LAST_READ: LastRead = LastRead::new();

/// `LastRead::allow` on the process-wide record, with the clock.
pub fn reader_may_look(pane_id: &str, exited: bool) -> bool {
    LAST_READ.allow(pane_id, exited, Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> PanePlace {
        PanePlace {
            kind: "linux".into(),
            distro: "Ubuntu-Test".into(),
            cwd: "/mnt/d/work/shop".into(),
            mount_root: "/mnt/".into(),
            unc_root: r"\\wsl.localhost\Ubuntu-Test".into(),
            home_windows: r"\\wsl.localhost\Ubuntu-Test\home\you".into(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn a_linux_place_gives_the_linux_cwd_the_home_and_the_clock_slack() {
        let facts = facts_of(&place()).unwrap();
        assert_eq!(facts.cwd, "/mnt/d/work/shop");
        assert_eq!(facts.home_windows, PathBuf::from(r"\\wsl.localhost\Ubuntu-Test\home\you"));
        assert_eq!(facts.clock_slack, Duration::from_secs(60));
    }

    #[test]
    fn a_place_without_a_home_gives_no_facts_so_no_default_folder_is_read() {
        let mut no_home = place();
        no_home.home_windows = String::new();
        assert!(facts_of(&no_home).is_none());
        let mut other = place();
        other.kind = "windows".into();
        assert!(facts_of(&other).is_none());
    }

    #[test]
    fn a_running_pane_is_always_readable() {
        let last = LastRead::new();
        let now = Instant::now();
        assert!(last.allow("p", false, now));
        assert!(last.allow("p", false, now + Duration::from_secs(3600)));
    }

    #[test]
    fn an_exited_pane_gets_one_last_read_window_then_none() {
        let last = LastRead::new();
        let now = Instant::now();
        assert!(last.allow("p", true, now));
        assert!(last.allow("p", true, now + Duration::from_secs(1)), "the same tick may look more than once");
        assert!(!last.allow("p", true, now + LAST_READ_GRACE));
        assert!(!last.allow("p", true, now + Duration::from_secs(60)));
        // Another pane is not affected.
        assert!(last.allow("q", true, now + Duration::from_secs(60)));
    }

    #[test]
    fn a_pane_id_that_runs_again_is_readable_again() {
        let last = LastRead::new();
        let now = Instant::now();
        assert!(last.allow("p", true, now));
        assert!(!last.allow("p", true, now + Duration::from_secs(10)));
        assert!(last.allow("p", false, now + Duration::from_secs(11)));
        assert!(last.allow("p", true, now + Duration::from_secs(12)));
    }
}
