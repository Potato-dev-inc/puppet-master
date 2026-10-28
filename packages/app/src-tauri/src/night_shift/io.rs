//! Night Shift: the `NightIo` trait the runner drives (SPEC n32 3.5), and the plain helpers the live
//! adapter shares with the tests.
//!
//! The trait has no method that types, presses a key or answers a prompt. The runner can only
//! start a task, read state and stop a run. That is the whole of invariant S1: there is no write
//! path into a pane to misuse. The live implementation (`LiveIo`) is in `io_live.rs` and is not
//! compiled in unit tests; the runner tests use a fake.
//!
//! Changes from the spec's listing, all needed to reach the right project: `watch`, `stop` and
//! `touched` take the `Task` (its project is the key for operations and missions);
//! `other_active_missions` takes the run (it subtracts its own missions); `load`, `persist`,
//! `archive` and `utc_offset_min` are the store and the clock the runner needs; `Watch::Failed` is
//! a single turn that failed for a reason that is not a gate.

use serde_json::Value;

use super::classify::{gate_sentence, Facts};
use super::io_facts::asked_text;
use super::model::{Attempt, NightRun, Task};
use super::plan::Cand;
use crate::missions::model::AgentChoice;
use crate::operations::USAGE_LIMIT_KIND;

pub use super::io_start::StartFail;

/// What a started task hands back. A mission has an id; a single turn only a run handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub mission_id: Option<String>,
    pub handle: String,
    pub pane_ids: Vec<String>,
}

/// What one open attempt is doing right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    Working,
    /// Parked at a usage limit. `parked_ms` is how long the limit has been on screen.
    Limit { reset_at_ms: Option<u64>, self_resume_armed: bool, parked_ms: u64 },
    /// The agent is waiting for an answer. `waited_ms` is how long. The question is redacted and cut.
    Asked { question: String, waited_ms: u64 },
    /// A trust, permission or sign-in question. Never answered.
    Gate { code: String, sentence: String },
    /// The mission is over (green, stopped, failed, handed off, or it needs the person), or the turn completed.
    Done,
    /// A single turn failed and it was not a gate.
    Failed { code: String, sentence: String },
    /// The record or the pane vanished.
    Gone,
}

/// The panes and run handles an attempt has opened so far, so the runner can keep them quiet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touched {
    pub pane_ids: Vec<String>,
    pub handles: Vec<String>,
}

pub trait NightIo: Send + Sync {
    fn now_ms(&self) -> u64;
    /// Minutes east of UTC on this computer's clock.
    fn utc_offset_min(&self) -> i32;
    /// The saved run (current or last), if there is one.
    fn load(&self) -> Option<NightRun>;
    /// Save the run (revision +1) and tell the desktop.
    fn persist(&self, run: &mut NightRun);
    /// Keep a copy of a finished run in the history.
    fn archive(&self, run: &NightRun);
    /// The ticked plans as the scheduler sees them now (limit-checked, labelled).
    fn candidates(&self, run: &NightRun) -> Vec<Cand>;
    /// Missions that are active and not ours: they hold slots Night Shift must not take.
    fn other_active_missions(&self, run: &NightRun) -> usize;
    fn start(&self, run: &NightRun, task: &Task, plan: &AgentChoice, n: u8) -> Result<Started, StartFail>;
    fn watch(&self, task: &Task, a: &Attempt) -> Watch;
    fn touched(&self, task: &Task, a: &Attempt) -> Touched;
    /// Stop the attempt's turn. The copy and the pane stay. Never types anything.
    fn stop(&self, task: &Task, a: &Attempt, by: &str);
    /// What the copy and the record say about the task's last attempt (redacted).
    fn facts(&self, run: &NightRun, task: &Task) -> Facts;
    fn keep_awake(&self, on: bool) -> Result<(), String>;
    /// Send the one morning push (held by the live adapter until the stop time).
    fn push_morning(&self, run: &NightRun);
    fn audit(&self, kind: &str, summary: &str);
}

/// The error code a gate of this `required_action.kind` ends with, or `None` when it is a plain
/// question. Matching is by the generic words of the kind (trust, startup, permission, approval,
/// command, sign-in): no agent is named.
pub fn gate_code(kind: &str) -> Option<&'static str> {
    let kind = kind.to_ascii_lowercase();
    if kind.contains("trust") {
        Some("WORKSPACE_TRUST_DECLINED")
    } else if kind.contains("startup") {
        Some("STARTUP_SCREEN_NOT_COMPLETED")
    } else if ["permission", "approval", "command", "login", "sign_in", "signin", "auth"].iter().any(|w| kind.contains(w)) {
        Some("PANE_BLOCKED_BY_PROMPT")
    } else {
        None
    }
}

/// What a turn that waits for input means: a usage limit, a gate, or a question. `label` is the
/// agent's name as data, `waited_ms` how long it has been waiting, `clean` the redaction.
pub fn read_waiting(action: &Value, label: &str, waited_ms: u64, clean: &dyn Fn(&str) -> String) -> Watch {
    let kind = action.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind == USAGE_LIMIT_KIND {
        return Watch::Limit {
            reset_at_ms: action.get("reset_at_ms").and_then(Value::as_u64),
            self_resume_armed: action.get("self_resume_armed").and_then(Value::as_bool).unwrap_or(false),
            parked_ms: waited_ms,
        };
    }
    if let Some(code) = gate_code(kind) {
        let sentence = gate_sentence(code, label).unwrap_or_else(|| format!("{label} stopped at a question."));
        return Watch::Gate { code: code.to_string(), sentence };
    }
    Watch::Asked { question: asked_text(action, clean), waited_ms }
}

#[cfg(not(test))]
#[path = "io_live.rs"]
mod live;
#[cfg(not(test))]
#[allow(unused_imports)]
pub use live::{live, LiveIo};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn same(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn a_usage_limit_carries_its_reset_and_whether_the_cli_continues_itself() {
        let action = json!({"kind": "usage_limit", "reset_at_ms": 5_000_u64, "self_resume_armed": true});
        assert_eq!(
            read_waiting(&action, "Fake A", 90_000, &same),
            Watch::Limit { reset_at_ms: Some(5_000), self_resume_armed: true, parked_ms: 90_000 }
        );
        let bare = json!({"kind": "usage_limit"});
        assert_eq!(
            read_waiting(&bare, "Fake A", 0, &same),
            Watch::Limit { reset_at_ms: None, self_resume_armed: false, parked_ms: 0 }
        );
    }

    #[test]
    fn trust_startup_and_permission_questions_are_gates_with_a_plain_sentence() {
        for (kind, code, sentence) in [
            ("workspace_trust", "WORKSPACE_TRUST_DECLINED", "Fake A asked to trust the folder."),
            ("startup_screen", "STARTUP_SCREEN_NOT_COMPLETED", "Fake A stopped at a question before it started."),
            ("permission_required", "PANE_BLOCKED_BY_PROMPT", "Fake A stopped at a question before it started."),
            ("command_approval", "PANE_BLOCKED_BY_PROMPT", "Fake A stopped at a question before it started."),
        ] {
            let got = read_waiting(&json!({"kind": kind, "summary": "x"}), "Fake A", 1, &same);
            assert_eq!(got, Watch::Gate { code: code.into(), sentence: sentence.into() }, "{kind}");
        }
    }

    #[test]
    fn any_other_question_is_asked_with_redacted_cut_text() {
        let clean = |s: &str| s.replace("sk-ant-secret", "[secret]");
        let action = json!({"kind": "unknown_question", "question": "Use sk-ant-secret for the key?"});
        match read_waiting(&action, "Fake A", 301_000, &clean) {
            Watch::Asked { question, waited_ms } => {
                assert_eq!(question, "Use [secret] for the key?");
                assert_eq!(waited_ms, 301_000);
            }
            other => panic!("not asked: {other:?}"),
        }
        let long = json!({"kind": "question", "question": "q".repeat(900)});
        match read_waiting(&long, "Fake A", 1, &same) {
            Watch::Asked { question, .. } => assert!(question.chars().count() <= 400),
            other => panic!("not asked: {other:?}"),
        }
    }

    #[test]
    fn gate_code_names_no_agent_and_knows_only_generic_words() {
        assert_eq!(gate_code("Workspace_Trust"), Some("WORKSPACE_TRUST_DECLINED"));
        assert_eq!(gate_code("unknown_question"), None);
        assert_eq!(gate_code(""), None);
    }
}
