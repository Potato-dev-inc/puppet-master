//! The outbox over the live app: the pane registry, the pane's own delivery code, the Tauri event
//! `pane-outbox` for the desktop windows and the SSE event of the same name for paired phones.

use super::draft_guard::{DraftGuard, DRAFT_PREFIX};
use super::host::{Attempt, Host, Look, PaneKind};
use super::model::{OutboxMessage, FAIL_IN_BOX, FAIL_NOT_TAKEN, FAIL_WRITE};
use super::Outbox;
use crate::agent_runs::delivery::{self, Delivery, PaneIo};
use crate::agent_runs::readiness;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tauri::AppHandle;

pub struct LiveHost {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
}

impl LiveHost {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) -> Self {
        Self { registry, app }
    }

    fn agent_type(&self, pane_id: &str) -> Option<String> {
        self.registry.lock().get(pane_id).map(|pane| pane.info.agent_type.clone())
    }
}

/// How a finished delivery reads for the outbox. `typed`: the message text went into the pane's box.
pub(super) fn attempt_of(outcome: Delivery, typed: bool) -> Attempt {
    match outcome {
        Delivery::Started | Delivery::SubmittedUnconfirmed => Attempt::Sent,
        Delivery::NotVisible { cleared: true } => Attempt::Failed(FAIL_NOT_TAKEN.into()),
        Delivery::NotVisible { cleared: false } => Attempt::Failed(FAIL_IN_BOX.into()),
        Delivery::Blocked(_) if typed => Attempt::Failed(FAIL_IN_BOX.into()),
        Delivery::Blocked(_) => Attempt::Gate,
        Delivery::AgentGone(_) => Attempt::Gone,
        Delivery::NotReady { .. } if typed => Attempt::Failed(FAIL_IN_BOX.into()),
        Delivery::NotReady { .. } => Attempt::NotTyped,
        Delivery::WriteFailed(error) if error.starts_with(DRAFT_PREFIX) => Attempt::Draft,
        Delivery::WriteFailed(_) => Attempt::Failed(FAIL_WRITE.into()),
    }
}

impl Host for LiveHost {
    fn pane_kind(&self, pane_id: &str) -> Option<PaneKind> {
        let agent = self.agent_type(pane_id)?;
        Some(PaneKind::of(crate::tui::profile_for(&agent).features().kind))
    }

    fn run_controls(&self, pane_id: &str) -> bool {
        let cwd = self.registry.lock().get(pane_id).map(|pane| pane.info.cwd.clone());
        cwd.is_some_and(|cwd| crate::bridge::pane_has_open_run(pane_id, &cwd))
    }

    fn look(&self, pane_id: &str) -> Option<Look> {
        let (agent, idle) = {
            let registry = self.registry.lock();
            let pane = registry.get(pane_id)?;
            let idle = pane.status.lock().as_str() == "idle";
            (pane.info.agent_type.clone(), idle)
        };
        let profile = crate::tui::profile_for(&agent);
        let verdict = readiness::assess(profile.as_ref(), delivery::observe_pane(&self.registry, pane_id));
        Some(Look::from_verdict(&verdict, idle))
    }

    fn type_message(&self, pane_id: &str, text: &str) -> Attempt {
        let Some(agent) = self.agent_type(pane_id) else { return Attempt::Gone };
        let profile = crate::tui::profile_for(&agent);
        let mut pane = PaneIo { registry: &self.registry, app: &self.app, pane_id };
        let mut io = DraftGuard::new(&mut pane);
        let mut trace = Vec::new();
        // No clear sequence: a person's message never wipes text the person typed in the terminal box.
        let outcome = delivery::deliver_text(&mut io, profile.as_ref(), None, text, &mut trace);
        let typed = trace.iter().any(|line| line.contains(delivery::TYPING_TRACE));
        tracing::debug!(pane_id, ?outcome, typed, "outbox delivery");
        if let Delivery::WriteFailed(error) = &outcome {
            if !error.starts_with(DRAFT_PREFIX) {
                tracing::warn!(pane_id, %error, "outbox could not type the message");
            }
        }
        attempt_of(outcome, typed)
    }

    fn publish(&self, pane_id: &str, messages: &[OutboxMessage]) {
        publish_sse(pane_id, messages);
        // Not in unit tests: emitting through the app handle links the windowing runtime into the lib test exe.
        #[cfg(not(test))]
        {
            use tauri::Emitter;
            let _ = self.app.emit("pane-outbox", json!({ "paneId": pane_id, "messages": messages }));
        }
    }
}

/// `event: pane-outbox` for paired phones. The text goes through `redact` first; the stream filter
/// (`mobile_pairing::caps::project`) leaves it out for a device that cannot watch.
fn publish_sse(pane_id: &str, messages: &[OutboxMessage]) {
    let shown: Vec<OutboxMessage> = messages.iter().map(redacted).collect();
    if let Ok(body) = serde_json::to_string(&json!({ "paneId": pane_id, "messages": shown })) {
        crate::bridge::push_sse(format!("event: pane-outbox\ndata: {body}\n\n"));
    }
}

/// The copy of a message that leaves this machine: its text redacted.
pub fn redacted(message: &OutboxMessage) -> OutboxMessage {
    OutboxMessage { text: crate::redact::text(&message.text).into_inner(), ..message.clone() }
}

static GLOBAL: OnceLock<Outbox> = OnceLock::new();

/// The app's one outbox (the Tauri commands and the bridge routes share it).
pub fn global(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) -> &'static Outbox {
    GLOBAL.get_or_init(|| Outbox::new(Arc::new(LiveHost::new(registry.clone(), app.clone()))))
}
