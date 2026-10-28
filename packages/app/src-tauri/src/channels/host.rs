//! Text Your Agents: the `Host` trait. Everything the engine needs from the rest of the app goes
//! through it, so the engine is tested with a fake (`test_host`) and never touches a pane, the
//! attention list or the audit log directly. The real one is `host_live`.

use super::config::TelegramConfig;
use crate::attention::answer::{Reply, Request};
use crate::attention::caps::DeviceCaps;
use crate::attention::model::Snapshot;

/// One pane in the `/panes` list. `heading` is the sidebar name ("auth refactor", "Codex 2"),
/// `status` a short word ("working", "needs you", "idle"). No pane id is ever shown to the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLine {
    pub id: String,
    pub heading: String,
    pub agent_label: String,
    pub status: String,
}

/// One companion. `pane_id` is the pane its text is typed into; `awake` is false while it sleeps
/// (waking a companion is `control`, never from the chat).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionLine {
    pub id: String,
    pub name: String,
    pub pane_id: String,
    pub awake: bool,
}

pub trait Host: Send + Sync {
    fn now_ms(&self) -> u64;
    /// The saved settings, read fresh each call.
    fn config(&self) -> TelegramConfig;
    fn save_config(&self, c: &TelegramConfig) -> Result<(), String>;
    fn panes(&self) -> Vec<PaneLine>;
    /// The attention list as of now.
    fn snapshot(&self) -> Snapshot;
    fn question(&self, id: &str) -> Option<crate::questions::Question>;
    /// Hands text to the pane's message box (`pane_outbox`). It is typed only when the agent is
    /// ready; the error is a plain sentence.
    fn send_to_pane(&self, pane_id: &str, text: &str) -> Result<(), String>;
    fn companions(&self) -> Vec<CompanionLine>;
    /// The shared answer path (`attention::answer::answer` with `Via::Telegram`).
    fn answer(&self, item_id: &str, req: &Request, caps: DeviceCaps) -> Reply;
    /// One audit line. `detail` holds counts, kinds, labels and ids only, never message text.
    fn audit(&self, kind: crate::audit::Kind, summary: &str, detail: serde_json::Value);
    /// Tell the desktop that the Telegram status changed (`telegram://changed`).
    fn changed(&self);
}
