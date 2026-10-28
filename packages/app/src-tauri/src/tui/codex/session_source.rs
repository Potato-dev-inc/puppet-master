//! Which of Codex's rollout files a pane is writing.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::session_file::{codex_home, recent_rollouts, rollout_meta, rollout_started, CodexRollout};
use crate::session_chat::{Located, PaneFacts, SessionFormat, SessionSource};

/// A rollout can start a few seconds before the pane's clock says (rounding, clock reads).
const SLACK: Duration = Duration::from_secs(5);

/// Two rollouts that started this close together cannot be told apart by their start time.
const APART: Duration = Duration::from_secs(2);

pub struct CodexSource;

pub static SOURCE: CodexSource = CodexSource;

impl SessionSource for CodexSource {
    fn format(&self) -> &dyn SessionFormat {
        &CodexRollout
    }

    /// The pane's own account folder, else `CODEX_HOME` / `~/.codex`.
    fn locate(&self, pane: &PaneFacts) -> Located {
        match pane.home.clone().or_else(codex_home) {
            Some(home) => locate_in(&home, pane),
            None => Located::NotYet,
        }
    }
}

struct Candidate {
    path: PathBuf,
    cwd: String,
    id: String,
    started: Option<SystemTime>,
}

/// Rollouts written since the pane started. With the thread id (reported by the agent's hooks) the file is
/// known. Without it, the rollouts of the pane's folder: one is taken; several are told apart by when they
/// started, and only left open when two started within `APART` of each other.
fn locate_in(home: &Path, pane: &PaneFacts) -> Located {
    let since = pane.started_at.checked_sub(SLACK).unwrap_or(pane.started_at);
    let all: Vec<Candidate> = recent_rollouts(home, since)
        .into_iter()
        .filter_map(|path| {
            let (cwd, id) = rollout_meta(&path)?;
            let started = rollout_started(&path);
            Some(Candidate { path, cwd, id, started })
        })
        .collect();

    if let Some(id) = pane.provider_session_id.as_deref().filter(|id| !id.is_empty()) {
        return all.into_iter().find(|c| c.id == id).map_or(Located::NotYet, |c| Located::Found(c.path));
    }
    let want = normalise(&pane.cwd.to_string_lossy());
    let mut mine: Vec<Candidate> = all.into_iter().filter(|c| normalise(&c.cwd) == want).collect();
    match mine.len() {
        0 => return Located::NotYet,
        1 => return Located::Found(mine.remove(0).path),
        _ => {}
    }
    // Rollouts that started after the pane did come first, the closest one first; then the rest, closest first.
    mine.sort_by_key(|c| match c.started {
        Some(at) if at >= pane.started_at => (0, at.duration_since(pane.started_at).unwrap_or_default()),
        Some(at) => (1, pane.started_at.duration_since(at).unwrap_or_default()),
        None => (2, Duration::ZERO),
    });
    let best = mine[0].started;
    let next = mine[1].started;
    let tied = match (best, next) {
        (Some(a), Some(b)) => a.max(b).duration_since(a.min(b)).unwrap_or_default() < APART,
        _ => true,
    };
    if tied {
        Located::Ambiguous(mine.into_iter().map(|c| c.path).collect())
    } else {
        Located::Found(mine.remove(0).path)
    }
}

/// A folder as compared: `\\?\` prefix and trailing separators gone, `/` for `\`, and on Windows no case.
fn normalise(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path).replace('\\', "/");
    let path = path.trim_end_matches('/');
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Local};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A fake Codex home in the system temp dir, removed when the guard drops.
    struct Home(PathBuf);

    impl Home {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("pm-codex-source-{}-{unique}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Home(dir)
        }

        /// Writes a rollout for `id` in the day folder of `started`, with a `session_meta` first line.
        fn rollout(&self, id: &str, cwd: &str, started: SystemTime) -> PathBuf {
            let local = DateTime::<Local>::from(started);
            let dir = self.0.join("sessions").join(local.format("%Y/%m/%d").to_string());
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("rollout-{}-{id}.jsonl", local.format("%Y-%m-%dT%H-%M-%S")));
            let stamp = DateTime::<chrono::Utc>::from(started).format("%Y-%m-%dT%H:%M:%S%.3fZ");
            let cwd = cwd.replace('\\', "\\\\");
            let line = format!(r#"{{"timestamp":"{stamp}","type":"session_meta","payload":{{"id":"{id}","cwd":"{cwd}"}}}}"#);
            std::fs::write(&path, format!("{line}\n")).unwrap();
            path
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const CWD: &str = r"C:\Users\you\proj";

    fn pane(started_at: SystemTime, id: Option<&str>) -> PaneFacts {
        PaneFacts {
            cwd: PathBuf::from(CWD),
            started_at,
            provider_session_id: id.map(str::to_string),
            home: None,
            launch: Default::default(),
            linux: None,
        }
    }

    fn ago(seconds: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(seconds)
    }

    #[test]
    fn no_rollout_is_not_yet() {
        let home = Home::new();
        assert_eq!(locate_in(&home.0, &pane(ago(30), None)), Located::NotYet);
    }

    #[test]
    fn the_only_rollout_of_the_folder_is_found() {
        let home = Home::new();
        home.rollout("t-other", r"C:\Users\you\elsewhere", ago(25));
        let mine = home.rollout("t-mine", CWD, ago(25));
        assert_eq!(locate_in(&home.0, &pane(ago(30), None)), Located::Found(mine));
    }

    #[test]
    fn a_resumed_rollout_that_started_long_ago_is_found_when_it_is_the_only_one() {
        let home = Home::new();
        let old = home.rollout("t-old", CWD, ago(3600));
        // Still written to, so its modified time is recent; a resumed session looks like this.
        std::fs::OpenOptions::new().write(true).open(&old).unwrap().set_modified(ago(1)).unwrap();
        assert_eq!(locate_in(&home.0, &pane(ago(30), None)), Located::Found(old));
    }

    #[test]
    fn a_rollout_just_before_the_pane_started_still_counts() {
        let home = Home::new();
        let file = home.rollout("t-a", CWD, ago(33));
        assert_eq!(locate_in(&home.0, &pane(ago(30), None)), Located::Found(file));
    }

    #[test]
    fn the_thread_id_picks_the_rollout() {
        let home = Home::new();
        let mine = home.rollout("t-mine", CWD, ago(25));
        home.rollout("t-other", CWD, ago(25));
        assert_eq!(locate_in(&home.0, &pane(ago(30), Some("t-mine"))), Located::Found(mine));
    }

    #[test]
    fn the_thread_id_is_matched_even_when_the_recorded_folder_reads_differently() {
        let home = Home::new();
        let mine = home.rollout("t-mine", r"D:\somewhere\else", ago(25));
        assert_eq!(locate_in(&home.0, &pane(ago(30), Some("t-mine"))), Located::Found(mine));
    }

    #[test]
    fn a_thread_id_with_no_rollout_yet_is_not_yet() {
        let home = Home::new();
        home.rollout("t-other", CWD, ago(25));
        assert_eq!(locate_in(&home.0, &pane(ago(30), Some("t-mine"))), Located::NotYet);
    }

    #[test]
    fn several_rollouts_are_told_apart_by_the_start_closest_after_the_pane() {
        let home = Home::new();
        let started = ago(60);
        home.rollout("t-before", CWD, started - Duration::from_secs(3));
        let mine = home.rollout("t-mine", CWD, started + Duration::from_secs(1));
        home.rollout("t-late", CWD, started + Duration::from_secs(20));
        assert_eq!(locate_in(&home.0, &pane(started, None)), Located::Found(mine));
    }

    #[test]
    fn rollouts_that_started_within_two_seconds_of_each_other_are_ambiguous() {
        let home = Home::new();
        let started = ago(60);
        let a = home.rollout("t-a", CWD, started + Duration::from_secs(1));
        let b = home.rollout("t-b", CWD, started + Duration::from_millis(2500));
        match locate_in(&home.0, &pane(started, None)) {
            Located::Ambiguous(mut paths) => {
                paths.sort();
                let mut want = vec![a, b];
                want.sort();
                assert_eq!(paths, want);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_id_settles_what_the_start_time_could_not() {
        let home = Home::new();
        let started = ago(60);
        home.rollout("t-a", CWD, started + Duration::from_secs(1));
        let b = home.rollout("t-b", CWD, started + Duration::from_secs(2));
        assert_eq!(locate_in(&home.0, &pane(started, Some("t-b"))), Located::Found(b));
    }

    #[test]
    fn folders_compare_without_slash_style_trailing_separator_or_prefix() {
        assert_eq!(normalise(r"C:\Users\you\proj\"), normalise("C:/Users/you/proj"));
        assert_eq!(normalise(r"\\?\C:\Users\you\proj"), normalise(r"C:\Users\you\proj"));
        assert_ne!(normalise(r"C:\Users\you\proj"), normalise(r"C:\Users\you\proj2"));
    }

    #[cfg(windows)]
    #[test]
    fn folders_compare_without_letter_case_on_windows() {
        assert_eq!(normalise(r"c:\users\YOU\Proj"), normalise(r"C:\Users\you\proj"));
    }

    #[test]
    fn the_pane_home_is_used_instead_of_the_default_codex_home() {
        let home = Home::new();
        let mine = home.rollout("t-mine", CWD, ago(25));
        let mut facts = pane(ago(30), None);
        facts.home = Some(home.0.clone());
        assert_eq!(SOURCE.locate(&facts), Located::Found(mine));
    }

    #[test]
    fn a_finished_zst_rollout_is_found_like_a_plain_one() {
        let home = Home::new();
        let local = DateTime::<Local>::from(ago(25));
        let dir = home.0.join("sessions").join(local.format("%Y/%m/%d").to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-2026-10-27T09-00-00-0198a000.jsonl.zst");
        std::fs::write(&path, include_bytes!("fixtures/rollout_basic.jsonl.zst")).unwrap();
        assert_eq!(locate_in(&home.0, &pane(ago(30), None)), Located::Found(path));
    }

    /// The whole path for a pane: the profile hands out the source, the watcher finds the rollout under the
    /// pane's Codex home and reads the fixture through the rollout format.
    #[test]
    fn a_watcher_reads_the_fixture_through_the_profile_hook() {
        use crate::session_chat::{PaneLookup, Sink, Watchers};
        use std::sync::Arc;

        struct Quiet;
        impl Sink for Quiet {
            fn send(&self, _pane_id: &str, _items: Vec<crate::session_chat::ChatItem>, _reset: bool) {}
        }
        struct Pane(PaneFacts);
        impl PaneLookup for Pane {
            fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
                Some(self.0.clone())
            }
        }

        let home = Home::new();
        let path = home.rollout("t-fixture", CWD, ago(25));
        std::fs::write(&path, include_str!("fixtures/rollout_basic.jsonl").replace("\r\n", "\n")).unwrap();

        let mut facts = pane(ago(30), None);
        facts.home = Some(home.0.clone());
        let source = crate::tui::profile_for("codex").session_chat().expect("codex has a session source");
        let watchers = Watchers::new(Arc::new(Quiet), Arc::new(Pane(facts)));
        let items = watchers.open("pane-1", "w", source);
        watchers.close_all();

        assert!(!items.is_empty());
        assert!(items.iter().any(|item| matches!(item, crate::session_chat::ChatItem::User { .. })));
    }
}
