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
    /// Lets go of the session-chat reader this companion holds on the pane.
    fn release_reader(&self, pane_id: &str, companion_id: &str);

    /// The card that says the companion stopped, with `Try again`.
    fn raise_stopped(&self, companion_id: &str, name: &str);
    fn clear_stopped(&self, companion_id: &str);

    /// Whether the agent can be a companion here, and why not (`agents::agent_ok`).
    fn agent_ok(&self, agent_type: &str, folder: &str) -> Result<(), String>;
    fn agent_label(&self, agent_type: &str) -> String;
    /// The agent's declared instruction files (empty for an agent that declares none).
    fn instruction_files(&self, agent_type: &str) -> Vec<InstructionFile>;

    /// Something changed: `companion://changed` (with folders) and the phone event (without).
    fn publish(&self, views: &[CompanionView]);

    /// A reply went into the feed. The push layer decides who hears it (phones with `watch`, the
    /// once-a-minute window) and does nothing when `enabled` is false. Must not block the keeper.
    fn reply_pushed(&self, companion_id: &str, name: &str, text: &str, enabled: bool);
}
