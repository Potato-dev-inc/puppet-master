//! The guard for an agent that is sent CI output (Red to Green, `ci_fix`).
//!
//! The old "Send back to <agent>" path (the raw log into a file, typed into the author's pane) is
//! gone: CI logs are untrusted text, and `ci_fix` is the only path that reads them now (SPEC n33,
//! S16). What is left here is [`pane_if_guarded`]: a pane may be given CI output only when it
//! cannot push and it works inside the copy.
//!
//! Generic code: it names no agent.

use crate::operations::OperationError;
use crate::pty::env_guard;

/// The pane that may be sent the log: the newest run of the copy must run under the `no_push`
/// guard (it cannot push to GitHub) and its pane must be inside the copy. Anything else is refused.
/// `panes_in_copy` are the ids of the panes whose folder is inside the copy.
pub fn pane_if_guarded(
    guard_env: Option<&str>,
    pane_id: Option<&str>,
    panes_in_copy: &[String],
) -> Result<String, OperationError> {
    if guard_env != Some(env_guard::NO_PUSH) {
        return Err(OperationError::new(
            "SEND_BACK_CAN_PUSH",
            "This agent could push to GitHub, so Puppet Master will not hand it CI output. Fix the checks yourself, or start a new agent in this copy that cannot push.",
            false,
        ));
    }
    let Some(pane_id) = pane_id else {
        return Err(OperationError::new(
            "AGENT_GONE",
            "That agent's pane is closed, so there is nobody to send this to.",
            false,
        ));
    };
    if !panes_in_copy.iter().any(|id| id == pane_id) {
        return Err(OperationError::new("SEND_BACK_OUTSIDE", "That agent is not working in this copy.", false));
    }
    Ok(pane_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_that_can_push_is_refused() {
        let panes = vec!["pane-1".to_string()];
        for guard in [None, Some(""), Some("other")] {
            let refused = pane_if_guarded(guard, Some("pane-1"), &panes).unwrap_err();
            assert_eq!(refused.code, "SEND_BACK_CAN_PUSH", "{guard:?}");
            assert!(refused.message.contains("could push"), "{}", refused.message);
        }
    }

    #[test]
    fn only_the_guarded_pane_inside_the_copy_is_allowed() {
        let panes = vec!["pane-1".to_string()];
        assert_eq!(pane_if_guarded(Some(env_guard::NO_PUSH), Some("pane-1"), &panes).unwrap(), "pane-1");
        assert_eq!(pane_if_guarded(Some(env_guard::NO_PUSH), None, &panes).unwrap_err().code, "AGENT_GONE");
        assert_eq!(
            pane_if_guarded(Some(env_guard::NO_PUSH), Some("pane-9"), &panes).unwrap_err().code,
            "SEND_BACK_OUTSIDE"
        );
    }

    #[test]
    fn no_agent_name_in_non_test_code() {
        let text = include_str!("sendback.rs");
        let code = text.split("#[cfg(test)]").next().unwrap_or_default().to_ascii_lowercase();
        for agent in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!code.contains(agent), "names an agent outside tests: {agent}");
        }
    }
}
