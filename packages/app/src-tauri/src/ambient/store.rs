//! Per-pane titles: the derived one, the user's own, and the first-line capture that feeds the
//! derived one. Kept in a process-wide map keyed by pane id so `PaneInfo` construction sites do not
//! change; `PaneState::info()` reads [`get`].

use super::title::{derive_title, LineCapture, MAX_TITLE_CHARS};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default)]
struct Entry {
    title: Option<String>,
    /// The user named it: the first-line capture never overwrites it.
    user_set: bool,
    /// Present until a title is found.
    capture: Option<LineCapture>,
}

static STORE: Lazy<Mutex<HashMap<String, Entry>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn get(pane_id: &str) -> Option<String> {
    STORE.lock().get(pane_id).and_then(|e| e.title.clone())
}

/// Observe one input write to a pane running `agent_type`. `screen_line` is the last non-blank line
/// of its screen. Returns true when it set the pane's title.
///
/// Only agent panes are captured: a typed line in a plain shell is often a password, a token or a
/// command, so shells never get an automatic title (the user can still rename them).
pub fn observe_input(pane_id: &str, agent_type: &str, screen_line: &str, text: &str, append_newline: bool) -> bool {
    if !crate::tui::is_agent_backend(agent_type) {
        return false;
    }
    let mut store = STORE.lock();
    let entry = store.entry(pane_id.to_string()).or_insert_with(|| Entry { capture: Some(LineCapture::new()), ..Entry::default() });
    let Some(capture) = entry.capture.as_mut() else { return false };
    match capture.feed(text, append_newline, screen_line) {
        Some(title) => {
            entry.title = Some(title);
            entry.capture = None;
            true
        }
        None => {
            if capture.gave_up() {
                entry.capture = None;
            }
            false
        }
    }
}

/// Set the title from a task text directly (run_agent). Does nothing if a title exists.
pub fn set_from_task(pane_id: &str, task: &str) -> bool {
    let Some(title) = derive_title(task) else { return false };
    let mut store = STORE.lock();
    let entry = store.entry(pane_id.to_string()).or_default();
    if entry.title.is_some() {
        return false;
    }
    entry.title = Some(title);
    entry.capture = None;
    true
}

/// The user renames a pane. Blank clears the name and resumes automatic titling.
pub fn set_user_title(pane_id: &str, raw: &str) -> Option<String> {
    let clean = super::title::sanitize(raw);
    let clean: String = clean.chars().take(MAX_TITLE_CHARS).collect();
    let clean = clean.trim().to_string();
    let mut store = STORE.lock();
    let entry = store.entry(pane_id.to_string()).or_default();
    if clean.is_empty() {
        entry.title = None;
        entry.user_set = false;
        entry.capture = Some(LineCapture::new());
        None
    } else {
        entry.title = Some(clean.clone());
        entry.user_set = true;
        entry.capture = None;
        Some(clean)
    }
}

pub fn is_user_set(pane_id: &str) -> bool {
    STORE.lock().get(pane_id).is_some_and(|e| e.user_set)
}

/// Drop entries of panes that are gone.
fn retain_in(map: &mut HashMap<String, Entry>, live: &[String]) {
    map.retain(|id, _| live.iter().any(|l| l == id));
}

pub fn retain_live(live: &[String]) {
    retain_in(&mut STORE.lock(), live);
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &str = "claude";
    const SHELL: &str = "bash";

    #[test]
    fn typing_then_enter_titles_the_pane_once() {
        let id = "store-test-1";
        assert!(!observe_input(id, AGENT, "", "refactor the ", false));
        assert!(!observe_input(id, AGENT, "", "auth module", false));
        assert!(observe_input(id, AGENT, "", "\r", false));
        assert_eq!(get(id).as_deref(), Some("refactor the auth module"));
        // The second line never replaces it.
        assert!(!observe_input(id, AGENT, "", "something else\r", false));
        assert_eq!(get(id).as_deref(), Some("refactor the auth module"));
    }

    #[test]
    fn a_user_name_wins_and_blank_resumes_automatic_titles() {
        let id = "store-test-2";
        assert_eq!(set_user_title(id, "  my \u{1b}[31mplan  "), Some("my plan".into()));
        assert!(is_user_set(id));
        assert!(!observe_input(id, AGENT, "", "ignored line\r", false));
        assert_eq!(get(id).as_deref(), Some("my plan"));
        assert_eq!(set_user_title(id, "   "), None);
        assert_eq!(get(id), None);
        assert!(observe_input(id, AGENT, "", "fix flaky test\r", false));
        assert_eq!(get(id).as_deref(), Some("fix flaky test"));
    }

    #[test]
    fn a_shell_pane_never_gets_an_automatic_title() {
        let id = "store-test-shell";
        assert!(!observe_input(id, SHELL, "$ ", "fix the login bug\r", false));
        assert_eq!(get(id), None);
        // Renaming still works on a shell.
        assert_eq!(set_user_title(id, "deploy box"), Some("deploy box".into()));
    }

    #[test]
    fn a_typed_password_at_a_secret_prompt_is_never_the_title() {
        let id = "store-test-password";
        assert!(!observe_input(id, AGENT, "Password:", "hunter2\r", false));
        assert_eq!(get(id), None);
        // The next normal line on an ordinary screen becomes the title.
        assert!(observe_input(id, AGENT, "> ", "fix the parser\r", false));
        assert_eq!(get(id).as_deref(), Some("fix the parser"));
    }

    #[test]
    fn capture_gives_up_after_three_secret_prompt_submissions() {
        let id = "store-test-give-up";
        for _ in 0..3 {
            assert!(!observe_input(id, AGENT, "Enter API key:", "sk-live\r", false));
        }
        assert!(!observe_input(id, AGENT, "> ", "fix the parser\r", false));
        assert_eq!(get(id), None);
    }

    #[test]
    fn task_text_sets_the_title_only_when_none_exists() {
        let id = "store-test-3";
        assert!(set_from_task(id, "Please add dark mode to settings."));
        assert_eq!(get(id).as_deref(), Some("add dark mode to settings"));
        assert!(!set_from_task(id, "another task"));
    }

    #[test]
    fn retain_forgets_closed_panes() {
        let mut map: HashMap<String, Entry> = HashMap::new();
        map.insert("a".into(), Entry { title: Some("x title".into()), ..Entry::default() });
        map.insert("b".into(), Entry::default());
        retain_in(&mut map, &["b".to_string()]);
        assert!(!map.contains_key("a") && map.contains_key("b"));
    }
}
