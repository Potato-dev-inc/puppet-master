//! Which agents can run as a terminal chat, and what the person is told about each.

use serde::Serialize;

use crate::tui::{Kind, TuiProfile};

/// `PaneInfo.lane` of a terminal chat pane (the serde value of `RunLane::TerminalChat`).
pub const LANE: &str = "terminal_chat";

/// A session id that passes `valid_session_id`, used only to ask whether the agent has a resume option.
const PROBE_SESSION: &str = "00000000-0000-4000-8000-000000000000";

/// What an eligible agent offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalChatInfo {
    pub label: String,
    /// The chat is read from the agent's session file (always true for an eligible agent; the chat view falls back to
    /// the screen only until the file shows).
    pub reads_file: bool,
    /// A closed chat can be continued (the agent has resume arguments).
    pub can_resume: bool,
    /// The profile has its own prompt parser, so a question can be answered from the chat.
    pub can_answer_prompts: bool,
    pub note: String,
}

/// `Ok` when `profile` can be started in a pane and has a session reader. A shell has no turns; an agent with no
/// session file keeps its chat made some other way (the headless bridge).
pub fn info_for_profile(profile: &dyn TuiProfile, label: &str, visible: bool) -> Result<TerminalChatInfo, String> {
    if profile.features().kind == Kind::Shell {
        return Err(format!("{label} is a shell, not an agent"));
    }
    if !visible {
        return Err(format!("{label} cannot run in a terminal pane"));
    }
    if profile.session_chat().is_none() {
        return Err(format!("{label} has no session file Puppet Master can read"));
    }
    let can_resume = profile.session_history().is_some_and(|history| history.resume_args(PROBE_SESSION).is_some());
    let can_answer_prompts = profile.has_prompt_parser();
    let note = match (can_resume, can_answer_prompts) {
        (true, true) => "Continues old chats. Its questions can be answered here.",
        (true, false) => "Continues old chats. Its questions are answered in the terminal.",
        (false, true) => "Cannot resume old sessions. Its questions can be answered here.",
        (false, false) => "Cannot resume old sessions. Its questions are answered in the terminal.",
    };
    Ok(TerminalChatInfo { label: label.to_string(), reads_file: true, can_resume, can_answer_prompts, note: note.into() })
}

/// `Ok` when `agent_id` is registered and can run as a terminal chat.
pub fn available_for(agent_id: &str) -> Result<TerminalChatInfo, String> {
    let descriptor = crate::tui::descriptor_for(agent_id).ok_or_else(|| format!("{agent_id} is not a known agent"))?;
    let profile = crate::tui::profile_for(agent_id);
    info_for_profile(&*profile, &descriptor.label, descriptor.modes.visible)
}

/// One row per eligible agent, for the lane view: `{id, label, reads_file, can_resume, can_answer_prompts, note}`.
pub fn eligible_rows() -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = crate::tui::registry::snapshot()
        .iter()
        .filter_map(|profile| {
            let id = profile.id();
            let info = available_for(id).ok()?;
            let mut row = serde_json::to_value(&info).ok()?;
            row["id"] = serde_json::json!(id);
            Some(row)
        })
        .collect();
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    rows
}

/// The plain sentence on the Add chat screen for the `terminal_chat` option (spec section 3.5).
pub fn summary_sentence(info: &TerminalChatInfo) -> String {
    format!(
        "Puppet Master runs {} in a terminal and shows it as a chat. You get its tool cards and history, and you can answer its questions here. {}",
        info.label,
        if info.can_resume { "It can continue the conversation later." } else { "It cannot resume old sessions." }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::profile_for;

    #[test]
    fn a_reader_and_a_parser_make_the_full_offer() {
        let info = available_for("claude").expect("eligible");
        assert!(info.reads_file && info.can_answer_prompts && info.can_resume);
        assert_eq!(info.label, crate::tui::descriptor_for("claude").unwrap().label);
    }

    /// A profile with a session reader borrowed from a built-in one and no prompt parser of its own.
    struct ReaderOnly;
    impl TuiProfile for ReaderOnly {
        fn id(&self) -> &'static str {
            "reader_only"
        }
        fn session_chat(&self) -> Option<&'static dyn crate::session_chat::SessionSource> {
            profile_for("claude").session_chat()
        }
    }

    /// A profile with neither a reader nor a parser.
    struct Bare;
    impl TuiProfile for Bare {
        fn id(&self) -> &'static str {
            "bare"
        }
    }

    #[test]
    fn a_reader_without_a_parser_is_eligible_and_read_only() {
        let info = info_for_profile(&ReaderOnly, "Agent", true).expect("reads its session");
        assert!(info.reads_file);
        assert!(!info.can_answer_prompts, "no prompt parser: the card stays read-only");
        assert!(!info.can_resume, "no history reader, no resume");
        assert_eq!(info.note, "Cannot resume old sessions. Its questions are answered in the terminal.");
    }

    #[test]
    fn a_profile_without_a_reader_is_refused() {
        assert!(info_for_profile(&Bare, "Agent", true).unwrap_err().contains("no session file"));
    }

    #[test]
    fn no_reader_means_not_available() {
        let eligible: Vec<String> = eligible_rows().iter().filter_map(|r| r["id"].as_str().map(str::to_string)).collect();
        let mut checked = 0;
        for profile in crate::tui::registry::snapshot().iter() {
            let shell = profile.features().kind == Kind::Shell;
            let listed = eligible.iter().any(|id| id == profile.id());
            if shell || profile.session_chat().is_none() {
                assert!(!listed, "{} has no reader or is a shell", profile.id());
                assert!(available_for(profile.id()).is_err());
                checked += 1;
            }
        }
        assert!(checked > 0, "at least the shell is refused");
        assert!(available_for("no_such_agent").unwrap_err().contains("not a known agent"));
    }

    #[test]
    fn invisible_agents_are_refused() {
        let claude = profile_for("claude");
        assert!(info_for_profile(&*claude, "Claude", false).unwrap_err().contains("cannot run in a terminal pane"));
    }

    #[test]
    fn the_summary_sentence_is_exact() {
        let with = TerminalChatInfo { label: "Kilo".into(), reads_file: true, can_resume: true, can_answer_prompts: true, note: String::new() };
        assert_eq!(
            summary_sentence(&with),
            "Puppet Master runs Kilo in a terminal and shows it as a chat. You get its tool cards and history, and you can answer its questions here. It can continue the conversation later."
        );
        let without = TerminalChatInfo { can_resume: false, ..with };
        assert!(summary_sentence(&without).ends_with("It cannot resume old sessions."));
    }
}
