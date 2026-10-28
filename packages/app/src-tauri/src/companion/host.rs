//! What the companion service needs from the outside: the pane registry, the outbox, the attention
//! list, the clock and the tell-everyone events. `live.rs` implements it over the real app; the
//! service tests implement it with a fake and a virtual clock, so the whole life of a companion
//! (spawn, crash, restart, check-ins, nap) runs in unit tests with no terminal.

use super::keeper::Pane;
use super::model::CompanionView;
use crate::tui::InstructionFile;

/// A pane to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    /// `companion-<id>`.
    pub pane_id: String,
    pub agent_type: String,
    /// The home folder.
    pub cwd: String,
    pub extra_args: Vec<String>,
    pub account: Option<String>,
}

/// The attention item that blocks a pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocking {
    pub item_id: String,
    pub title: String,
}

/// A line the agent wrote, with an id that changes when the text does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub id: String,
    pub text: String,
}

/// One finished turn of another pane, for `today.md`. Plain facts only: the sentence is the turn
/// summary (never raw chat text) and the folder is a name, not a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedTurn {
    pub pane_id: String,
    pub turn: u32,
    pub ended_at_ms: i64,
    /// `idle`, `waiting_input` or `error` (how the turn came to rest).
    pub end: String,
    /// The agent's display name.
    pub agent: String,
    /// The last segment of the pane's project folder; empty when it is not known.
    pub folder_name: String,
    /// The display name of the agent that started this pane as its helper, when there is one.
    pub parent_label: Option<String>,
    pub sentence: String,
}

/// An agent that is known to be at its usage limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentLimit {
    /// When it can be used again (epoch ms), when it said so.
    pub reset_at_ms: Option<i64>,
}

/// One thing that was said in a companion's chats, found by the search index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallHit {
    pub at_ms: i64,
    /// `user` (the person) or `assistant` (the companion).
    pub kind: String,
    /// Already redacted by the index; the service redacts again before it leaves.
    pub snippet: String,
}

/// The answer of a recall search. `enabled` is false when the person turned the index off in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallAnswer {
    pub enabled: bool,
    pub hits: Vec<RecallHit>,
}

pub trait Host: Send + Sync + 'static {
    /// Epoch milliseconds.
    fn now_ms(&self) -> i64;
    /// The local zone as seconds east of UTC.
    fn offset_secs(&self) -> i32;

    /// What the registry shows for this pane id.
    fn pane(&self, pane_id: &str) -> Pane;
    /// Starts the pane through the spawn gate (`tui::registry::admit`) and the registry. The error is a
    /// plain sentence. Never opens a window or takes focus.
    fn spawn(&self, spec: &SpawnSpec) -> Result<(), String>;
    /// Closes the pane (the keeper's own kill).
    fn kill(&self, pane_id: &str);

    /// Adds a message to the pane's outbox (`pane_outbox::Outbox::send`). It is never typed any other way.
    fn send(&self, pane_id: &str, text: &str) -> Result<(), String>;
    /// Texts of the messages still waiting or being typed in the pane's outbox.
    fn waiting(&self, pane_id: &str) -> Vec<String>;

    /// The open item in the attention list that blocks this pane.
    fn blocking(&self, pane_id: &str) -> Option<Blocking>;
    /// The pane shows a startup screen only the person may pass (sign-in) that has no card of its own.
    fn sign_in_gate(&self, pane_id: &str) -> bool;
    /// The pane's activity phrase ("Reading notes.md").
    fn activity(&self, pane_id: &str) -> Option<String>;

    /// The agent's newest assistant text after the person's last message (session chat).
    fn last_reply(&self, pane_id: &str, companion_id: &str) -> Option<Reply>;
    /// The one-line sentence of the last finished turn (turn summary): the fallback for `last_reply`.
    fn last_sentence(&self, pane_id: &str) -> Option<Reply>;
    /// Every pane's turns that ended at or after `since_ms` (the turn summaries), oldest first.
    /// The default knows none.
    fn finished_turns(&self, _since_ms: i64) -> Vec<FinishedTurn> {
        Vec::new()
    }
    /// Lets go of the session-chat reader this companion holds on the pane.
    fn release_reader(&self, pane_id: &str, companion_id: &str);

    /// Whether the chat search index is on (it feeds `companion_recall`). The default says no.
    fn recall_on(&self) -> bool {
        false
    }
    /// Searches what was said in the chats of exactly these panes, newest first (the chat search index).
    /// Never looks anywhere else. The default has no index.
    fn recall(&self, _panes: &[String], _query: &str, _limit: usize) -> RecallAnswer {
        RecallAnswer { enabled: false, hits: Vec::new() }
    }

    /// The card that says the companion stopped, with `Try again`.
    fn raise_stopped(&self, companion_id: &str, name: &str);
    fn clear_stopped(&self, companion_id: &str);

    /// Whether the agent can be a companion here, and why not (`agents::agent_ok`).
    fn agent_ok(&self, agent_type: &str, folder: &str) -> Result<(), String>;
    fn agent_label(&self, agent_type: &str) -> String;
    /// The agent's declared instruction files (empty for an agent that declares none).
    fn instruction_files(&self, agent_type: &str) -> Vec<InstructionFile>;
    /// Whether this agent is known to be at its usage limit right now (the hand-off detector saw it and
    /// it has not recovered), under this account (`None` = its own login). The default knows none.
    fn agent_limit(&self, _agent_type: &str, _account: Option<&str>) -> Option<AgentLimit> {
        None
    }

    /// Something changed: `companion://changed` (with folders) and the phone event (without).
    fn publish(&self, views: &[CompanionView]);

    /// A reply went into the feed. The push layer decides who hears it (phones with `watch`, the
    /// once-a-minute window) and does nothing when `enabled` is false. Must not block the keeper.
    fn reply_pushed(&self, companion_id: &str, name: &str, text: &str, enabled: bool);
}
