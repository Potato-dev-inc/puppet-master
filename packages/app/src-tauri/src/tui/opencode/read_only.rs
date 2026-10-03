//! Read-only enforcement for panes this app spawns for `read_only` opencode_native runs.
//!
//! Verified against `opencode` 1.18.34: a server started with `OPENCODE_CONFIG_CONTENT` carrying a
//! primary agent whose permission is `{"*":"deny", read/grep/glob/list/question/todowrite: allow}`
//! and `default_agent` pointing at it never offers `bash`, `edit`, `write`, `webfetch` or any MCP
//! tool to the model, even when the project's own `opencode.json` allows them. (A top-level
//! `permission` block is not enough: a later project rule such as `pencil_*: allow` wins, so the
//! agent-level ruleset is used.) Every prompt, including follow-ups and TUI input, runs as that
//! default agent. See docs/orchestrator/opencode-native-worker.md.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

pub const READ_ONLY_AGENT: &str = "pm-read-only";

/// Inline config for `opencode serve` (env `OPENCODE_CONFIG_CONTENT`).
pub fn config_content() -> String {
    let config: Value = json!({
        "default_agent": READ_ONLY_AGENT,
        "agent": {
            READ_ONLY_AGENT: {
                "mode": "primary",
                "description": "Puppet Master read-only worker: read, search and ask only",
                "permission": {
                    "*": "deny",
                    "read": "allow",
                    "grep": "allow",
                    "glob": "allow",
                    "list": "allow",
                    "question": "allow",
                    "todowrite": "allow"
                }
            }
        }
    });
    config.to_string()
}

fn enforced_panes() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// Record whether the serve behind `pane_id` was started with the read-only config.
pub fn set_enforced(pane_id: &str, enforced: bool) {
    let mut set = enforced_panes().lock();
    if enforced {
        set.insert(pane_id.to_string());
    } else {
        set.remove(pane_id);
    }
}

pub fn is_enforced(pane_id: &str) -> bool {
    enforced_panes().lock().contains(pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_denies_everything_except_reads_and_makes_it_the_default_agent() {
        let config: Value = serde_json::from_str(&config_content()).unwrap();
        assert_eq!(config["default_agent"], READ_ONLY_AGENT);
        let permission = &config["agent"][READ_ONLY_AGENT]["permission"];
        assert_eq!(permission["*"], "deny");
        for tool in ["bash", "edit", "write", "webfetch"] {
            assert!(permission.get(tool).is_none(), "{tool} must fall under the deny-all rule");
        }
        assert_eq!(permission["read"], "allow");
    }

    #[test]
    fn enforcement_is_tracked_per_pane() {
        set_enforced("ro-pane-test", true);
        assert!(is_enforced("ro-pane-test"));
        set_enforced("ro-pane-test", false);
        assert!(!is_enforced("ro-pane-test"));
    }
}
