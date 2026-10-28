//! The companion service over the real app: the pane registry, the pane outbox, the attention list,
//! the session chat readers, and the thread `companion-keeper` that calls `Service::tick` once a second.
//!
//! A companion's pane is spawned through the spawn gate (`tui::registry::admit`) and the cockpit's
//! `Origin::Background` placement: no window opens and nothing takes focus. Messages reach the pane
//! only through `pane_outbox::Outbox::send`. Nothing here types into a pane, presses a key or answers
//! a dialog.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;
use tauri::{AppHandle, Manager};

use super::host::{AgentLimit, Blocking, FinishedTurn, Host, RecallAnswer, RecallHit, Reply, SpawnSpec};
use super::keeper::Pane;
use super::model::CompanionView;
use super::service::Service;
use super::{agents, view};
use crate::agent_runs::delivery;
use crate::agent_runs::readiness::{self, Readiness};
use crate::attention;
use crate::attention::model::{Action, Kind, Lane, Raised, State};
use crate::commands::SessionChatState;
use crate::pty::{registry_kill_pane, registry_spawn_pane, PaneRegistry, SpawnPaneArgs};
use crate::session_chat::{ChatItem, Opening};
use crate::tui::InstructionFile;

/// The first pass waits this long, so the bridge and the registries are up.
const FIRST_TICK: Duration = Duration::from_secs(3);
const TICK: Duration = Duration::from_secs(1);

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();

/// The running service (`None` before `start`).
pub fn service() -> Option<Arc<Service>> {
    SERVICE.get().cloned()
}

/// Starts the keeper thread. Call once, from the app setup, after the bridge and the registries.
pub fn start(app: AppHandle, registry: Arc<Mutex<PaneRegistry>>) {
    let root = crate::app_paths::app_data_dir();
    let host = Arc::new(LiveHost { app, registry, readers: Mutex::new(HashSet::new()), parents: Mutex::new(HashMap::new()) });
    let service = Arc::new(Service::new(host, root));
    if SERVICE.set(service.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("companion-keeper".into()).spawn(move || {
        std::thread::sleep(FIRST_TICK);
        loop {
            // A panic in one pass must not end the keeper for the rest of the run.
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| service.tick()));
            if pass.is_err() {
                tracing::error!("companion keeper pass panicked; trying again next second");
            }
            std::thread::sleep(TICK);
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "companion keeper thread did not start");
    }
}

pub struct LiveHost {
    app: AppHandle,
    registry: Arc<Mutex<PaneRegistry>>,
    /// `<pane>|<companion>` pairs whose session-chat reader this companion already holds.
    readers: Mutex<HashSet<String>>,
    /// Pane id -> (its parent pane id, the parent's agent label), remembered while panes are alive: a
    /// helper's pane is usually closed by the time its turn is listed.
    parents: Mutex<HashMap<String, (String, String)>>,
}

impl LiveHost {
    fn agent_of(&self, pane_id: &str) -> Option<String> {
        self.registry.lock().get(pane_id).map(|pane| pane.info.agent_type.clone())
    }

    fn panes_changed(&self) {
        crate::bridge::push_panes_sse(&self.registry);
        // Not in unit tests: emitting through the app handle links the windowing runtime into the lib test exe.
        #[cfg(not(test))]
        {
            use tauri::Emitter;
            let _ = self.app.emit("pty://panes-changed", json!({ "changed": true }));
        }
    }
}

impl Host for LiveHost {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn offset_secs(&self) -> i32 {
        chrono::Local::now().offset().local_minus_utc()
    }

    fn pane(&self, pane_id: &str) -> Pane {
        let status = {
            let registry = self.registry.lock();
            match registry.get(pane_id) {
                Some(pane) => pane.status.lock().as_str().to_string(),
                None => return Pane::Missing,
            }
        };
        let seen = Pane::from_status(&status);
        if seen != Pane::Dead && delivery::pane_process_state(&self.registry, pane_id) == readiness::AgentProcess::Exited {
            // The host shell is still there but the agent has ended (a crash, `/exit`, a declined dialog).
            return Pane::Dead;
        }
        seen
    }

    fn spawn(&self, spec: &SpawnSpec) -> Result<(), String> {
        crate::tui::registry::admit(&spec.agent_type, &spec.cwd).map_err(|e| e.message)?;
        let args = SpawnPaneArgs {
            agent_type: spec.agent_type.clone(),
            cwd: Some(spec.cwd.clone()),
            cols: None,
            rows: None,
            extra_args: (!spec.extra_args.is_empty()).then(|| spec.extra_args.clone()),
            pane_id: Some(spec.pane_id.clone()),
            read_only: false,
            account: spec.account.clone(),
        };
        let pane_id = registry_spawn_pane(&self.registry, &self.app, args)?;
        let info = self.registry.lock().list().into_iter().find(|pane| pane.id == pane_id);
        if let Some(pane) = info {
            // `Background` places nothing: no window, no focus. The person opens it from the sidebar.
            crate::cockpit::present_pane(&self.app, &pane.id, &pane.agent_type, pane.cols, pane.rows, crate::cockpit::Origin::Background);
        }
        self.panes_changed();
        Ok(())
    }

    fn kill(&self, pane_id: &str) {
        let _ = registry_kill_pane(&self.registry, pane_id);
        self.panes_changed();
    }

    fn send(&self, pane_id: &str, text: &str) -> Result<(), String> {
        crate::pane_outbox::global(&self.registry, &self.app).send(pane_id, text).map(|_| ())
    }

    fn waiting(&self, pane_id: &str) -> Vec<String> {
        crate::pane_outbox::global(&self.registry, &self.app)
            .list(pane_id)
            .into_iter()
            .filter(|message| message.state.active())
            .map(|message| message.text)
            .collect()
    }

    fn blocking(&self, pane_id: &str) -> Option<Blocking> {
        attention::snapshot()
            .items
            .into_iter()
            .find(|item| {
                item.state == State::Open && item.lane == Lane::Blocking && item.pane.as_ref().is_some_and(|p| p.id == pane_id)
            })
            .map(|item| Blocking { item_id: item.id, title: item.title })
    }

    fn sign_in_gate(&self, pane_id: &str) -> bool {
        let Some(agent) = self.agent_of(pane_id) else { return false };
        let profile = crate::tui::profile_for(&agent);
        let verdict = readiness::assess(profile.as_ref(), delivery::observe_pane(&self.registry, pane_id));
        // A startup screen the person must pass (sign-in, onboarding). Puppet Master never passes it.
        matches!(verdict.readiness(), Readiness::Gate(gate) if gate.dismiss.is_none())
    }

    fn activity(&self, pane_id: &str) -> Option<String> {
        crate::pane_activity::snapshot().get(pane_id).map(|activity| activity.text.clone())
    }

    fn last_reply(&self, pane_id: &str, companion_id: &str) -> Option<Reply> {
        let agent = self.agent_of(pane_id)?;
        let opening = Opening::for_agent(&agent)?;
        let watchers = self.app.state::<SessionChatState>().watchers(&self.app, &self.registry);
        let holder = format!("companion#{companion_id}");
        if self.readers.lock().insert(format!("{pane_id}|{companion_id}")) {
            let _ = opening.open(&watchers, pane_id, holder.as_str().into());
        }
        let items = watchers.since(pane_id, None, 80)?.items;
        // The newest assistant text after the person's (or the check-in's) last message.
        for item in items.iter().rev() {
            match item {
                ChatItem::Assistant { id, text, .. } if !text.trim().is_empty() => {
                    return Some(Reply { id: id.clone(), text: text.clone() });
                }
                ChatItem::User { .. } => return None,
                _ => {}
            }
        }
        None
    }

    fn last_sentence(&self, pane_id: &str) -> Option<Reply> {
        let summary = crate::turn_summary::tracker().summaries(pane_id).pop()?;
        Some(Reply { id: format!("turn-{}-{}", summary.turn, summary.ended_at_ms), text: summary.sentence })
    }

    fn finished_turns(&self, since_ms: i64) -> Vec<FinishedTurn> {
        // Remember who each live pane's parent is, and what agent the parent runs.
        {
            let registry = self.registry.lock();
            let panes = registry.list();
            let mut parents = self.parents.lock();
            for pane in &panes {
                if let Some(parent) = &pane.parent_pane_id {
                    let label = panes
                        .iter()
                        .find(|p| &p.id == parent)
                        .map(|p| crate::tui::descriptor_for(&p.agent_type).map(|d| d.label).unwrap_or_else(|| p.agent_type.clone()))
                        .or_else(|| parents.get(&pane.id).map(|(_, label)| label.clone()))
                        .unwrap_or_else(|| "another agent".to_string());
                    parents.insert(pane.id.clone(), (parent.clone(), label));
                }
            }
        }
        let parents = self.parents.lock();
        crate::turn_summary::tracker()
            .since_with_project(since_ms.max(0) as u64)
            .into_iter()
            .map(|(summary, project)| FinishedTurn {
                parent_label: parents.get(&summary.pane_id).map(|(_, label)| label.clone()),
                pane_id: summary.pane_id,
                turn: summary.turn,
                ended_at_ms: summary.ended_at_ms as i64,
                end: summary.end.kind,
                agent: summary.agent,
                folder_name: project.map(|p| view::folder_name(&p.display().to_string())).unwrap_or_default(),
                sentence: summary.sentence,
            })
            .collect()
    }

    fn release_reader(&self, pane_id: &str, companion_id: &str) {
        if self.readers.lock().remove(&format!("{pane_id}|{companion_id}")) {
            if let Some(chat) = self.app.try_state::<SessionChatState>() {
                chat.watchers(&self.app, &self.registry).close(pane_id, &format!("companion#{companion_id}"));
            }
        }
    }

    fn recall_on(&self) -> bool {
        crate::chat_index::enabled()
    }

    fn recall(&self, panes: &[String], query: &str, limit: usize) -> RecallAnswer {
        let off = RecallAnswer { enabled: false, hits: Vec::new() };
        if !crate::chat_index::enabled() {
            return off;
        }
        let Some(chat) = self.app.try_state::<SessionChatState>() else { return off };
        let Some(index) = chat.index(&self.registry) else { return off };
        match index.search_in(query, limit, panes) {
            Ok(answer) => RecallAnswer {
                enabled: answer.enabled,
                hits: answer
                    .hits
                    .into_iter()
                    .map(|hit| RecallHit { at_ms: hit.at_ms, kind: hit.kind, snippet: hit.snippet })
                    .collect(),
            },
            Err(error) => {
                tracing::warn!(%error, "companion recall: search failed");
                RecallAnswer { enabled: true, hits: Vec::new() }
            }
        }
    }

    fn raise_stopped(&self, companion_id: &str, name: &str) {
        attention::raise(Raised {
            key: format!("companion:{companion_id}"),
            kind: Kind::Setup,
            pane_id: None,
            title: format!("{name} stopped 3 times. Open the terminal to see why."),
            // The phone draws a note, not the button: restarting is a computer action.
            sentence: Some("Open on the computer to try again.".to_string()),
            expires_at_ms: None,
            actions: vec![Action { id: "restart".into(), label: "Try again".into(), style: "primary".into(), needs: "control".into() }],
        });
    }

    fn clear_stopped(&self, companion_id: &str) {
        attention::clear(&format!("companion:{companion_id}"), None);
    }

    fn agent_ok(&self, agent_type: &str, folder: &str) -> Result<(), String> {
        agents::agent_ok(agent_type, folder).map_err(str::to_string)
    }

    fn agent_label(&self, agent_type: &str) -> String {
        crate::tui::descriptor_for(agent_type).map(|d| d.label).unwrap_or_else(|| agent_type.to_string())
    }

    fn instruction_files(&self, agent_type: &str) -> Vec<InstructionFile> {
        crate::tui::profile_for(agent_type).instruction_files()
    }

    fn agent_limit(&self, agent_type: &str, account: Option<&str>) -> Option<AgentLimit> {
        crate::handoff::agent_limit(agent_type, account).map(|reset| AgentLimit { reset_at_ms: reset.and_then(|r| i64::try_from(r).ok()) })
    }

    fn publish(&self, views: &[CompanionView]) {
        let phone: Vec<CompanionView> = views.iter().map(view::phone_copy).collect();
        if let Ok(body) = serde_json::to_string(&json!({ "v": 1, "companions": phone })) {
            crate::bridge::push_sse(format!("event: companion\ndata: {body}\n\n"));
        }
        #[cfg(not(test))]
        {
            use tauri::Emitter;
            let _ = self.app.emit("companion://changed", views);
        }
    }

    fn reply_pushed(&self, companion_id: &str, name: &str, text: &str, enabled: bool) {
        // The push layer returns at once: it sends from its own thread, never from the keeper's.
        crate::push::notify_companion_reply(companion_id, name, text, enabled);
    }
}
