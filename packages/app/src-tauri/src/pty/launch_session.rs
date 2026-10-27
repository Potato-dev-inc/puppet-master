//! The session id a pane's agent was told to use (or named) at launch (`TuiProfile::launch_session`), kept per
//! pane until the pane is taken out of the registry. Memory only. The chat view reads it
//! (`session_chat::PaneFacts`) so each pane finds its own session file.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

static IDS: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(Default::default);

/// Remember the pane's launch session id, or (`None`) forget a previous one: a respawn under the same pane id.
pub fn set(pane_id: &str, session_id: Option<String>) {
    let mut ids = IDS.lock();
    match session_id {
        Some(id) => {
            ids.insert(pane_id.to_string(), id);
        }
        None => {
            ids.remove(pane_id);
        }
    }
}

pub fn get(pane_id: &str) -> Option<String> {
    IDS.lock().get(pane_id).cloned()
}

/// The launch session ids of every pane but `pane_id`.
pub fn all_except(pane_id: &str) -> Vec<String> {
    IDS.lock().iter().filter(|(id, _)| id.as_str() != pane_id).map(|(_, session)| session.clone()).collect()
}

/// The pane is gone.
pub fn forget(pane_id: &str) {
    IDS.lock().remove(pane_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_kept_until_the_pane_is_forgotten() {
        set("launch-session-test", Some("abc".into()));
        assert_eq!(get("launch-session-test").as_deref(), Some("abc"));
        set("launch-session-other", Some("xyz".into()));
        assert!(all_except("launch-session-test").contains(&"xyz".to_string()));
        assert!(!all_except("launch-session-other").contains(&"xyz".to_string()));
        forget("launch-session-other");
        set("launch-session-test", None);
        assert_eq!(get("launch-session-test"), None);
        set("launch-session-test", Some("def".into()));
        forget("launch-session-test");
        assert_eq!(get("launch-session-test"), None);
    }
}
