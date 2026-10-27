//! Which of Claude Code's session files a pane is writing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::session_file::{config_dir, newest_session, project_dir, session_path, ClaudeSessionFile};
use crate::session_chat::{plain_path, Located, PaneFacts, SessionFormat, SessionSource};
#[cfg(test)]
use crate::session_chat::LaunchFacts;

/// A session file can be a few seconds older than the pane when the clocks or the filesystem round.
const SLACK: Duration = Duration::from_secs(5);

pub struct ClaudeSource;

pub static SOURCE: ClaudeSource = ClaudeSource;

impl SessionSource for ClaudeSource {
    fn format(&self) -> &dyn SessionFormat {
        &ClaudeSessionFile
    }

    /// The pane's own account folder, then `CLAUDE_CONFIG_DIR` / `~/.claude`.
    fn locate(&self, pane: &PaneFacts) -> Located {
        locate_in(&config_dirs(pane), pane)
    }

    fn looking_in(&self, pane: &PaneFacts) -> Option<PathBuf> {
        config_dirs(pane).first().map(|config| existing_project_dir(config, &pane.cwd))
    }
}

/// Where this pane's session files may be: its own account folder first, and the CLI's default folder
/// too (an account folder that holds nothing for the pane must not hide the file in the default one).
fn config_dirs(pane: &PaneFacts) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = pane.home.iter().cloned().collect();
    if let Some(default) = config_dir().filter(|default| !dirs.contains(default)) {
        dirs.push(default);
    }
    dirs
}

/// The file the pane's agent reported (hook session id) when it exists; else the newest session file in the
/// pane's project folder written since the pane started; else, with an id, the file that id will have.
///
/// A pane launched with `--session-id` (`LaunchFacts::from_launch`) owns exactly `<id>.jsonl`: other panes in the
/// folder write other files, so "the newest file" is never a stand-in while its own is missing (`own_file`).
fn locate_in(configs: &[PathBuf], pane: &PaneFacts) -> Located {
    let id = pane.provider_session_id.as_deref().filter(|id| !id.is_empty());
    let dir_of = |config: &PathBuf| existing_project_dir(config, &pane.cwd);
    if let Some(id) = id {
        let existing = configs.iter().map(|config| session_path(&dir_of(config), id)).find(|path| path.is_file());
        if pane.launch.from_launch {
            return match (existing, configs.first()) {
                (Some(path), _) => Located::Found(cleared_into(&path, pane).unwrap_or(path)),
                (None, Some(config)) => Located::Found(session_path(&dir_of(config), id)),
                (None, None) => Located::NotYet,
            };
        }
        if let Some(path) = existing {
            return Located::Found(path);
        }
    }
    let since = pane.started_at.checked_sub(SLACK).unwrap_or(pane.started_at);
    if let Some(path) = configs.iter().find_map(|config| newest_session(&dir_of(config), since)) {
        return Located::Found(path);
    }
    match (id, configs.first()) {
        (Some(id), Some(config)) => Located::Found(session_path(&dir_of(config), id)),
        _ => Located::NotYet,
    }
}

/// `/clear` starts a new session id and file. Hooks report it; without them the pane's launch file just stops growing
/// while a newer one appears in its folder. That newer file is taken only when it can only be this pane's: no other
/// live pane of the agent works in the folder (`siblings == 0`). With siblings a newer file may be theirs, so the
/// view stays on the launch file until a hook reports the new id (the known limit without hooks).
fn cleared_into(own: &Path, pane: &PaneFacts) -> Option<PathBuf> {
    if pane.launch.siblings > 0 {
        return None;
    }
    let written = own.metadata().ok()?.modified().ok()?;
    let newest = newest_session(own.parent()?, written)?;
    let newer = newest.metadata().ok()?.modified().ok()? > written;
    (newest != own && newer).then_some(newest)
}

/// `<config>/projects/<encoded cwd>`. A `\\?\` prefix and a trailing separator on the cwd are ignored, and
/// where the folder exists only under another letter case (Windows paths) that folder is used.
fn existing_project_dir(config: &Path, cwd: &Path) -> PathBuf {
    let text = plain_path(&cwd.to_string_lossy());
    let trimmed = text.trim_end_matches(['/', '\\']);
    let exact = project_dir(config, Path::new(if trimmed.is_empty() { text.as_str() } else { trimmed }));
    if exact.is_dir() {
        return exact;
    }
    let wanted = exact.file_name().map(|name| name.to_string_lossy().to_lowercase());
    let folded = std::fs::read_dir(config.join("projects")).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .find(|entry| Some(entry.file_name().to_string_lossy().to_lowercase()) == wanted)
            .map(|entry| entry.path())
    });
    folded.unwrap_or(exact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::SystemTime;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A fake config folder in the system temp dir, removed when the guard drops.
    struct Config(PathBuf);

    impl Config {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("pm-claude-source-{}-{unique}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Config(dir)
        }

        /// Writes `<config>/projects/<folder>/<name>` modified at `modified`.
        fn session(&self, folder: &str, name: &str, modified: SystemTime) -> PathBuf {
            let dir = self.0.join("projects").join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, b"{}\n").unwrap();
            File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
            path
        }
    }

    impl Drop for Config {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const CWD: &str = r"C:\Users\you\proj";
    const FOLDER: &str = "C--Users-you-proj";

    fn pane(started_at: SystemTime, id: Option<&str>) -> PaneFacts {
        PaneFacts {
            cwd: PathBuf::from(CWD),
            started_at,
            provider_session_id: id.map(str::to_string),
            home: None,
            launch: Default::default(),
        }
    }

    /// A pane launched with its own session id, with `siblings` other panes of the agent in the folder.
    fn launched(started_at: SystemTime, id: &str, siblings: usize) -> PaneFacts {
        let mut facts = pane(started_at, Some(id));
        facts.launch = LaunchFacts { from_launch: true, siblings };
        facts
    }

    fn ago(seconds: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(seconds)
    }

    #[test]
    fn a_known_session_id_names_the_file_even_before_it_exists() {
        let config = Config::new();
        let dir = config.0.join("projects").join(FOLDER);
        assert_eq!(
            locate_in(&[config.0.clone()], &pane(ago(60), Some("abc-1"))),
            Located::Found(dir.join("abc-1.jsonl"))
        );
    }

    #[test]
    fn a_known_session_id_wins_over_a_newer_file() {
        let config = Config::new();
        let mine = config.session(FOLDER, "mine.jsonl", ago(30));
        config.session(FOLDER, "other.jsonl", ago(1));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(60), Some("mine"))), Located::Found(mine));
    }

    #[test]
    fn without_an_id_the_newest_file_since_the_pane_started_is_taken() {
        let config = Config::new();
        config.session(FOLDER, "old.jsonl", ago(600));
        let first = config.session(FOLDER, "first.jsonl", ago(20));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::Found(first));
        let newer = config.session(FOLDER, "newer.jsonl", ago(2));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::Found(newer));
    }

    #[test]
    fn a_file_a_few_seconds_older_than_the_pane_still_counts() {
        let config = Config::new();
        let file = config.session(FOLDER, "slack.jsonl", ago(33));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::Found(file.clone()));
        config.session(FOLDER, "too-old.jsonl", ago(40));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::Found(file));
    }

    #[test]
    fn no_file_since_the_pane_started_is_not_yet() {
        let config = Config::new();
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::NotYet);
        config.session(FOLDER, "old.jsonl", ago(600));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::NotYet);
    }

    #[test]
    fn other_projects_are_not_looked_at() {
        let config = Config::new();
        config.session("C--Users-you-other", "x.jsonl", ago(1));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), None)), Located::NotYet);
    }

    #[test]
    fn an_extended_length_prefix_on_the_cwd_is_ignored() {
        // A folder picked in the app reached the registry as `\\?\D:\...`; the CLI writes under `D--...`.
        let config = Config::new();
        let file = config.session(FOLDER, "a.jsonl", ago(1));
        let mut facts = pane(ago(30), None);
        facts.cwd = PathBuf::from(format!(r"\\?\{CWD}"));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(file.clone()));
        facts.cwd = PathBuf::from(format!(r"\\?\{CWD}\"));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(file));
    }

    #[test]
    fn a_session_id_whose_file_does_not_exist_falls_back_to_the_newest_file() {
        let config = Config::new();
        let file = config.session(FOLDER, "real.jsonl", ago(3));
        assert_eq!(locate_in(&[config.0.clone()], &pane(ago(30), Some("not-written"))), Located::Found(file));
    }

    #[test]
    fn a_session_id_whose_file_appears_later_takes_over_from_the_fallback() {
        let config = Config::new();
        let stale = config.session(FOLDER, "stale.jsonl", ago(20));
        let facts = pane(ago(30), Some("mine"));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(stale));
        let mine = config.session(FOLDER, "mine.jsonl", ago(1));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(mine));
    }

    #[test]
    fn an_account_folder_without_the_file_also_tries_the_default_folder() {
        let account = Config::new();
        let default = Config::new();
        let file = default.session(FOLDER, "a.jsonl", ago(1));
        let both = [account.0.clone(), default.0.clone()];
        assert_eq!(locate_in(&both, &pane(ago(30), None)), Located::Found(file.clone()));
        assert_eq!(locate_in(&both, &pane(ago(30), Some("a"))), Located::Found(file));
        // The account folder wins when it has a file too.
        let own = account.session(FOLDER, "b.jsonl", ago(1));
        assert_eq!(locate_in(&both, &pane(ago(30), None)), Located::Found(own));
    }

    #[test]
    fn two_panes_in_one_folder_each_find_their_own_file() {
        let config = Config::new();
        let first = config.session(FOLDER, "id-one.jsonl", ago(5));
        let second = config.session(FOLDER, "id-two.jsonl", ago(1));
        let both = [config.0.clone()];
        // The newest file is the second pane's; the first pane must not follow it.
        assert_eq!(locate_in(&both, &launched(ago(30), "id-one", 1)), Located::Found(first));
        assert_eq!(locate_in(&both, &launched(ago(30), "id-two", 1)), Located::Found(second));
    }

    #[test]
    fn a_launched_pane_waits_for_its_own_file_instead_of_taking_another() {
        let config = Config::new();
        config.session(FOLDER, "other.jsonl", ago(1));
        let dir = config.0.join("projects").join(FOLDER);
        assert_eq!(
            locate_in(&[config.0.clone()], &launched(ago(30), "mine", 1)),
            Located::Found(dir.join("mine.jsonl"))
        );
    }

    #[test]
    fn after_clear_a_lone_pane_follows_the_new_file() {
        let config = Config::new();
        let launch_file = config.session(FOLDER, "mine.jsonl", ago(20));
        let facts = launched(ago(60), "mine", 0);
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(launch_file));
        let cleared = config.session(FOLDER, "after-clear.jsonl", ago(2));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(cleared.clone()));
        // A second clear moves on again.
        let again = config.session(FOLDER, "after-clear-2.jsonl", ago(1));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(again));
    }

    #[test]
    fn after_clear_a_pane_with_siblings_stays_on_its_launch_file() {
        // Known limit without hooks: a newer file may belong to a sibling, so nothing is guessed.
        let config = Config::new();
        let launch_file = config.session(FOLDER, "mine.jsonl", ago(20));
        config.session(FOLDER, "newer.jsonl", ago(2));
        assert_eq!(locate_in(&[config.0.clone()], &launched(ago(60), "mine", 1)), Located::Found(launch_file));
    }

    #[test]
    fn a_hook_reported_id_is_not_treated_as_the_launch_id() {
        // After `/clear` a hook reports the new id: its file wins, and the old fallback rules apply again.
        let config = Config::new();
        config.session(FOLDER, "launch.jsonl", ago(20));
        let hooked = config.session(FOLDER, "hooked.jsonl", ago(10));
        config.session(FOLDER, "sibling.jsonl", ago(1));
        let mut facts = pane(ago(60), Some("hooked"));
        facts.launch = LaunchFacts { from_launch: false, siblings: 1 };
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(hooked));
    }

    #[test]
    fn the_folder_looked_in_is_named_for_the_person() {
        let config = Config::new();
        let mut facts = pane(ago(30), None);
        facts.home = Some(config.0.clone());
        facts.cwd = PathBuf::from(format!(r"\\?\{CWD}"));
        assert_eq!(SOURCE.looking_in(&facts), Some(config.0.join("projects").join(FOLDER)));
    }

    #[test]
    fn a_trailing_separator_on_the_cwd_is_ignored() {
        let config = Config::new();
        let file = config.session(FOLDER, "a.jsonl", ago(1));
        let mut facts = pane(ago(30), None);
        facts.cwd = PathBuf::from(format!(r"{CWD}\"));
        assert_eq!(locate_in(&[config.0.clone()], &facts), Located::Found(file));
    }

    #[test]
    fn a_folder_that_differs_only_in_letter_case_is_found() {
        let config = Config::new();
        let file = config.session("c--users-you-proj", "a.jsonl", ago(1));
        // A case-insensitive filesystem answers with the cwd's own spelling of the folder.
        let Located::Found(found) = locate_in(&[config.0.clone()], &pane(ago(30), None)) else {
            panic!("not found");
        };
        assert_eq!(found.to_string_lossy().to_lowercase(), file.to_string_lossy().to_lowercase());
    }

    #[test]
    fn the_pane_home_is_used_instead_of_the_default_config_folder() {
        let config = Config::new();
        let file = config.session(FOLDER, "a.jsonl", ago(1));
        let mut facts = pane(ago(30), None);
        facts.home = Some(config.0.clone());
        assert_eq!(SOURCE.locate(&facts), Located::Found(file));
    }

    /// The whole path for a pane: the profile hands out the source, the watcher finds the file under the pane's
    /// config folder and reads the fixture through the Claude format.
    #[test]
    fn a_watcher_reads_the_fixture_through_the_profile_hook() {
        use crate::session_chat::{ChatItem, PaneLookup, Sink, Watchers};
        use std::sync::Arc;

        struct Quiet;
        impl Sink for Quiet {
            fn send(&self, _pane_id: &str, _items: Vec<ChatItem>, _reset: bool) {}
        }
        struct Pane(PaneFacts);
        impl PaneLookup for Pane {
            fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
                Some(self.0.clone())
            }
        }

        let config = Config::new();
        let fixture = include_str!("fixtures/session_basic.jsonl").replace("\r\n", "\n");
        let dir = config.0.join("projects").join(FOLDER);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fixture-session.jsonl"), fixture).unwrap();

        let mut facts = pane(ago(30), Some("fixture-session"));
        facts.home = Some(config.0.clone());
        let source = crate::tui::profile_for("claude").session_chat().expect("claude has a session source");
        let watchers = Watchers::new(Arc::new(Quiet), Arc::new(Pane(facts)));
        let items = watchers.open("pane-1", "w", source);
        watchers.close_all();

        assert!(items.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text == "list the files")));
        assert!(items.iter().any(|item| matches!(item, ChatItem::ToolCall { name, .. } if name == "Bash")));
        assert!(items.iter().any(|item| matches!(item, ChatItem::ToolResult { .. })));
    }

    // ---- two Claude panes in one folder (safety net) ----

    const ID_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const ID_B: &str = "bbbbbbbb-0000-4000-8000-000000000002";

    #[test]
    fn two_panes_with_their_own_session_ids_each_find_their_own_file_whichever_is_newer() {
        let config = Config::new();
        let a = config.session(FOLDER, &format!("{ID_A}.jsonl"), ago(5));
        let b = config.session(FOLDER, &format!("{ID_B}.jsonl"), ago(1));
        let started = ago(60);
        assert_eq!(locate_in(&[config.0.clone()], &pane(started, Some(ID_A))), Located::Found(a), "A has the older file and keeps it");
        assert_eq!(locate_in(&[config.0.clone()], &pane(started, Some(ID_B))), Located::Found(b));
    }

    /// The pick that falls back to "the newest file" (a pinned session whose file is not written yet) is the pane
    /// next to it in a shared folder: the watcher refuses a file that carries another pane's session id, so the
    /// fresh pane shows nothing until its own file comes, and the other pane's chat never shows in it.
    #[test]
    fn a_fresh_pane_next_to_a_busy_one_does_not_show_its_conversation_while_its_own_file_is_not_written() {
        use crate::session_chat::{ChatItem, PaneLookup, Sink, Watchers};
        use std::collections::HashMap;
        use std::sync::Arc;

        struct InConfig(Vec<PathBuf>);
        impl SessionSource for InConfig {
            fn format(&self) -> &dyn SessionFormat {
                &ClaudeSessionFile
            }
            fn locate(&self, pane: &PaneFacts) -> Located {
                locate_in(&self.0, pane)
            }
        }
        struct Quiet;
        impl Sink for Quiet {
            fn send(&self, _pane_id: &str, _items: Vec<ChatItem>, _reset: bool) {}
        }
        struct Two(HashMap<&'static str, PaneFacts>);
        impl PaneLookup for Two {
            fn facts(&self, pane_id: &str) -> Option<PaneFacts> {
                self.0.get(pane_id).cloned()
            }
            fn other_sessions(&self, pane_id: &str) -> Vec<String> {
                self.0.iter().filter(|(id, _)| **id != pane_id).filter_map(|(_, facts)| facts.provider_session_id.clone()).collect()
            }
        }
        let user_texts = |watchers: &Watchers, pane: &str| -> Vec<String> {
            watchers
                .since(pane, None, 100)
                .map(|delta| delta.items)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| if let ChatItem::User { text, .. } = item { Some(text) } else { None })
                .collect()
        };

        let config = Config::new();
        let dir = config.0.join("projects").join(FOLDER);
        std::fs::create_dir_all(&dir).unwrap();
        let fixture: String = include_str!("fixtures/session_basic.jsonl").lines().collect::<Vec<_>>().join("\n");
        std::fs::write(dir.join(format!("{ID_A}.jsonl")), &fixture).unwrap();

        let panes = Two(HashMap::from([("a", pane(ago(60), Some(ID_A))), ("b", pane(ago(30), Some(ID_B)))]));
        let source: &'static InConfig = Box::leak(Box::new(InConfig(vec![config.0.clone()])));
        let fast = Duration::from_millis(10);
        let tuning = crate::session_chat::watch::Tuning { poll: fast, screen_poll: fast, api_poll: fast, relocate: fast, ..Default::default() };
        let watchers = Watchers::with_tuning(Arc::new(Quiet), Arc::new(panes), tuning);
        let a_items = watchers.open("a", "w", source);
        assert!(a_items.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text == "list the files")), "A shows its own conversation");
        let b_items = watchers.open("b", "w", source);
        std::thread::sleep(Duration::from_millis(200)); // B looked again several times
        assert!(b_items.is_empty(), "B showed A's conversation: {b_items:?}");
        assert!(user_texts(&watchers, "b").is_empty(), "B showed A's conversation");
        assert!(watchers.waiting_for("b").is_some(), "B says it waits for its file");

        // B's agent writes its file: B shows it, A is unchanged.
        std::fs::write(dir.join(format!("{ID_B}.jsonl")), fixture.replace("list the files", "say bravo")).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(user_texts(&watchers, "b"), ["say bravo", "thanks, that is all"]);
        assert_eq!(user_texts(&watchers, "a"), ["list the files", "thanks, that is all"]);
        watchers.close_all();
    }
}
