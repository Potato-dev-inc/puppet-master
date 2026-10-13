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

/// Observe one input write. Returns true when it set the pane's title.
pub fn observe_input(pane_id: &str, text: &str, append_newline: bool) -> bool {
    let mut store = STORE.lock();
    let entry = store.entry(pane_id.to_string()).or_insert_with(|| Entry { capture: Some(LineCapture::new()), ..Entry::default() });
    let Some(capture) = entry.capture.as_mut() else { return false };
    match capture.feed(text, append_newline) {
        Some(title) => {
            entry.title = Some(title);
            entry.capture = None;
            true
        }
        None => false,
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

    #[test]
    fn typing_then_enter_titles_the_pane_once() {
        let id = "store-test-1";
        assert!(!observe_input(id, "refactor the ", false));
        assert!(!observe_input(id, "auth module", false));
        assert!(observe_input(id, "\r", false));
        assert_eq!(get(id).as_deref(), Some("refactor the auth module"));
        // The second line never replaces it.
        assert!(!observe_input(id, "something else\r", false));
        assert_eq!(get(id).as_deref(), Some("refactor the auth module"));
    }

    #[test]
    fn a_user_name_wins_and_blank_resumes_automatic_titles() {
        let id = "store-test-2";
        assert_eq!(set_user_title(id, "  my \u{1b}[31mplan  "), Some("my plan".into()));
        assert!(is_user_set(id));
        assert!(!observe_input(id, "ignored line\r", false));
        assert_eq!(get(id).as_deref(), Some("my plan"));
        assert_eq!(set_user_title(id, "   "), None);
        assert_eq!(get(id), None);
        assert!(observe_input(id, "fix flaky test\r", false));
        assert_eq!(get(id).as_deref(), Some("fix flaky test"));
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
