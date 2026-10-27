//! Which agents can be a companion (spec 2.3), and the launch arguments a companion may keep.
//!
//! Generic on purpose: it asks the registry about the agent's features and its spawn gate, never by
//! name. The spawn gate is the same first step `tui::registry::admit` takes; `admit` also runs the
//! binary probe (`canary::before_run`), which may start a process. The setup sheet must not start
//! anything, so it stops after the gate. The spawn itself (P2) goes through `admit`.

use crate::tui::declarative::wizard::bypass;
use crate::tui::{self, Kind};

use super::model::{CompanionError, CompanionResult};

/// The one reason a companion cannot use an agent that the person can act on from the chat.
pub const CHAT_CANNOT: &str = "Chat can't send messages to this agent yet.";
const NEEDS_APPROVAL: &str = "Approve this agent in Settings > Agents first.";
const NOT_INSTALLED: &str = "This agent is not installed on this computer.";

/// `Ok` when a companion can run on `agent_type` in `folder`; otherwise the reason in plain words.
pub fn agent_ok(agent_type: &str, folder: &str) -> Result<(), &'static str> {
    if agent_type.trim().is_empty() {
        return Err(CHAT_CANNOT);
    }
    if !tui::is_registered(agent_type) {
        // A wizard-added agent waiting for approval is not registered yet; say why.
        return Err(match tui::registry::unavailable(agent_type) {
            Some(err) => reason_for_code(&err.code),
            None => CHAT_CANNOT,
        });
    }
    let profile = tui::profile_for(agent_type);
    let features = profile.features();
    if features.kind != Kind::Cli || !features.dispatchable {
        return Err(CHAT_CANNOT);
    }
    match profile.spawn_gate(folder) {
        Ok(_) => Ok(()),
        Err(gate) => Err(reason_for_code(gate.code)),
    }
}

/// The plain reason for a spawn-gate or definition error code.
fn reason_for_code(code: &str) -> &'static str {
    match code {
        "AGENT_DEF_UNAPPROVED" | "AGENT_DEF_APPROVAL_STALE" | "AGENT_DEF_BINARY_CHANGED" => NEEDS_APPROVAL,
        "AGENT_NOT_INSTALLED" => NOT_INSTALLED,
        _ => CHAT_CANNOT,
    }
}

/// Refuses launch arguments that turn off the agent's own safety questions. A companion runs
/// unattended check-ins, so it must keep those questions.
pub fn check_launch_args(args: &[String]) -> CompanionResult<()> {
    match bypass::hits_in_argv(args).first() {
        Some(hit) => Err(CompanionError::new(
            "COMPANION_BYPASS_FLAG",
            format!("{} is not allowed: {}. A companion must keep the agent's own questions.", hit.token, hit.why),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_agent_gets_the_chat_reason() {
        assert_eq!(agent_ok("no-such-agent-xyz", "C:\\x"), Err(CHAT_CANNOT));
        assert_eq!(agent_ok("  ", "C:\\x"), Err(CHAT_CANNOT));
    }

    #[test]
    fn shell_panes_are_not_companions() {
        // A plain shell is registered but has no agent, so it cannot take a chat message.
        assert_eq!(agent_ok("powershell", "C:\\x"), Err(CHAT_CANNOT));
    }

    #[test]
    fn gate_codes_map_to_plain_reasons() {
        assert_eq!(reason_for_code("AGENT_DEF_UNAPPROVED"), NEEDS_APPROVAL);
        assert_eq!(reason_for_code("AGENT_NOT_INSTALLED"), NOT_INSTALLED);
        assert_eq!(reason_for_code("SOMETHING_ELSE"), CHAT_CANNOT);
    }

    #[test]
    fn bypass_flags_are_refused_and_normal_flags_pass() {
        let ok = vec!["--model".to_string(), "small".to_string()];
        assert!(check_launch_args(&ok).is_ok());
        let bad = vec!["--dangerously-skip-permissions".to_string()];
        let err = check_launch_args(&bad).unwrap_err();
        assert_eq!(err.code, "COMPANION_BYPASS_FLAG");
        assert!(err.message.contains("--dangerously-skip-permissions"));
        let split = vec!["--ask-for-approval".to_string(), "never".to_string()];
        assert_eq!(check_launch_args(&split).unwrap_err().code, "COMPANION_BYPASS_FLAG");
    }
}
