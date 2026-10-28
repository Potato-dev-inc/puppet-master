//! Text Your Agents: the real `Host`, wired to the app's pane registry, attention list, pane outbox,
//! companions, questions, audit log and the saved settings file.
//!
//! Thin on purpose: every rule lives in the engine or in the shared modules it calls. The one path
//! that changes a pane is `send_to_pane`, which goes through the pane outbox (waits for Ready, never
//! over the person's draft). The one answer path is `attention::answer::answer` with `Via::Telegram`,
//! which refuses everything a chat may not answer.

use super::config::{self, TelegramConfig};
use super::host::{CompanionLine, Host, PaneLine};
use crate::attention::answer::{Caller, RealEnv, Reply, Request, Via};
use crate::attention::caps::DeviceCaps;
use crate::attention::copy::{headings, PaneName};
use crate::attention::model::Snapshot;
use crate::audit::{Kind as AuditKind, Who};
use crate::companion::model::CompanionState;
use crate::companion::service::{is_pane_id, pane_id_of};
use crate::pane_outbox::PaneKind;
use crate::pty::{PaneInfo, PaneRegistry};
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

/// The name the chat is known by in the audit log and the answer ledger.
const CHANNEL: &str = "telegram";
/// Shown in the receipts and the audit summary.
const DEVICE_NAME: &str = "Telegram";
/// The event the desktop listens to for a status change.
const CHANGED_EVENT: &str = "telegram://changed";

pub struct LiveHost {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
}

impl LiveHost {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) -> Self {
        Self { registry, app }
    }
}

/// The word a chat shows for a pane status: `running` is "working", `waiting_input` is "needs you".
pub fn status_word(status: &str) -> &'static str {
    match status {
        "running" => "working",
        "waiting_input" => "needs you",
        "idle" => "idle",
        "error" => "error",
        _ => "unknown",
    }
}

/// Whether a companion can take a message now. A sleeping or stopped one cannot (waking is `control`,
/// never from the chat).
pub fn companion_awake(state: &CompanionState) -> bool {
    !matches!(state, CompanionState::Sleeping | CompanionState::Stopped)
}

/// The agent panes a chat may see, oldest first. Companion panes are left out (they are reached with
/// `/c` or plain text), and so are shell and protocol panes (the outbox refuses them). Headings use
/// the same numbering the sidebar uses ("Claude Code 2") over every pane.
pub fn pane_lines(infos: &[PaneInfo], label_of: &dyn Fn(&str) -> String, is_agent: &dyn Fn(&str) -> bool) -> Vec<PaneLine> {
    let names: Vec<PaneName> = infos
        .iter()
        .map(|p| PaneName { id: p.id.clone(), agent_type: p.agent_type.clone(), created_at: p.created_at, title: p.title.clone() })
        .collect();
    let heading_by_id = headings(&names, label_of);
    let mut shown: Vec<&PaneInfo> = infos.iter().filter(|p| !is_pane_id(&p.id) && is_agent(&p.agent_type)).collect();
    shown.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    shown
        .into_iter()
        .map(|p| PaneLine {
            id: p.id.clone(),
            heading: heading_by_id.get(&p.id).cloned().unwrap_or_else(|| label_of(&p.agent_type)),
            agent_label: label_of(&p.agent_type),
            status: status_word(&p.status).to_string(),
        })
        .collect()
}

/// The display name of an agent type, as the desktop shows it ("Claude Code"). Unknown types show as typed.
fn label_of(agent_type: &str) -> String {
    crate::tui::descriptor_for(agent_type).map(|d| d.label).unwrap_or_else(|| agent_type.to_string())
}

/// Only agent terminals take text from the chat.
fn is_agent(agent_type: &str) -> bool {
    PaneKind::of(crate::tui::profile_for(agent_type).features().kind) == PaneKind::Agent
}

impl Host for LiveHost {
    fn now_ms(&self) -> u64 {
        crate::attention::now_ms()
    }

    fn config(&self) -> TelegramConfig {
        config::load()
    }

    fn save_config(&self, c: &TelegramConfig) -> Result<(), String> {
        config::save(c)
    }

    fn panes(&self) -> Vec<PaneLine> {
        let infos = self.registry.lock().list();
        pane_lines(&infos, &label_of, &is_agent)
    }

    fn snapshot(&self) -> Snapshot {
        crate::attention::snapshot_now()
    }

    fn question(&self, id: &str) -> Option<crate::questions::Question> {
        crate::questions::store().get(id, crate::questions::now_ms())
    }

    fn send_to_pane(&self, pane_id: &str, text: &str) -> Result<(), String> {
        crate::pane_outbox::global(&self.registry, &self.app).send(pane_id, text).map(|_| ())
    }

    fn companions(&self) -> Vec<CompanionLine> {
        let Some(service) = crate::companion::live::service() else { return Vec::new() };
        service
            .views(true)
            .into_iter()
            .map(|view| CompanionLine {
                pane_id: pane_id_of(&view.id),
                awake: companion_awake(&view.state),
                id: view.id,
                name: view.name,
            })
            .collect()
    }

    fn answer(&self, item_id: &str, req: &Request, caps: DeviceCaps) -> Reply {
        let env = RealEnv { registry: self.registry.clone(), app: self.app.clone() };
        let caller = Caller { device_id: CHANNEL.into(), device_name: DEVICE_NAME.into(), caps, via: Via::Telegram };
        crate::attention::answer::answer(&env, &caller, item_id, req)
    }

    fn audit(&self, kind: AuditKind, summary: &str, detail: serde_json::Value) {
        crate::audit::record(kind, Who::Channel(CHANNEL.into()), summary, detail);
    }

    fn changed(&self) {
        // Not in unit tests: emitting through the app handle needs the windowing runtime.
        #[cfg(not(test))]
        {
            use tauri::Emitter;
            let _ = self.app.emit(CHANGED_EVENT, serde_json::json!({}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, agent: &str, created_at: i64, status: &str, title: Option<&str>) -> PaneInfo {
        PaneInfo {
            id: id.into(),
            agent_type: agent.into(),
            pid: 0,
            status: status.into(),
            created_at,
            last_output_at: None,
            cwd: "C:\\Users\\you\\demo".into(),
            cols: 80,
            rows: 24,
            viewer_held: false,
            reprints_on_resize: false,
            account: None,
            account_label: None,
            title: title.map(Into::into),
            parent_pane_id: None,
        }
    }

    fn label(agent: &str) -> String {
        match agent {
            "claude" => "Claude Code".into(),
            "codex" => "Codex".into(),
            other => other.into(),
        }
    }

    #[test]
    fn status_words_for_each_registry_status() {
        assert_eq!(status_word("running"), "working");
        assert_eq!(status_word("waiting_input"), "needs you");
        assert_eq!(status_word("idle"), "idle");
        assert_eq!(status_word("error"), "error");
        assert_eq!(status_word("something_new"), "unknown");
        assert_eq!(status_word(""), "unknown");
    }

    #[test]
    fn only_awake_companions_take_messages() {
        assert!(companion_awake(&CompanionState::Starting));
        assert!(companion_awake(&CompanionState::Idle));
        assert!(companion_awake(&CompanionState::Working));
        assert!(companion_awake(&CompanionState::NeedsYou));
        assert!(!companion_awake(&CompanionState::Sleeping));
        assert!(!companion_awake(&CompanionState::Stopped));
    }

    #[test]
    fn panes_are_listed_oldest_first_with_sidebar_numbering() {
        let infos = vec![
            info("p-b", "claude", 300, "running", None),
            info("p-a", "claude", 100, "waiting_input", None),
            info("p-c", "codex", 200, "idle", Some("auth refactor")),
        ];
        let lines = pane_lines(&infos, &label, &|_: &str| true);
        let ids: Vec<&str> = lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["p-a", "p-c", "p-b"]);
        assert_eq!(lines[0].heading, "Claude Code 1");
        assert_eq!(lines[0].status, "needs you");
        assert_eq!(lines[1].heading, "auth refactor");
        assert_eq!(lines[1].agent_label, "Codex");
        assert_eq!(lines[2].heading, "Claude Code 2");
        assert_eq!(lines[2].status, "working");
    }

    #[test]
    fn companion_panes_and_non_agent_panes_are_left_out() {
        let infos = vec![
            info("companion-mochi", "claude", 50, "idle", None),
            info("p-shell", "shell", 60, "idle", None),
            info("p-agent", "claude", 70, "idle", None),
        ];
        let lines = pane_lines(&infos, &label, &|agent: &str| agent != "shell");
        let ids: Vec<&str> = lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, vec!["p-agent"]);
    }
}
