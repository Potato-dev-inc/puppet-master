//! Tauri commands of Point and Fix (SPEC n32 section 3.5). The logic lives in `crate::point`; these are the
//! thin wrappers that the Point box, the preview chip and the Chat tab call.
//!
//! Sending goes only through the pane outbox (`send_with`). A refused message keeps the picks and shows the
//! outbox's own sentence. Point never writes to a pane itself.

use super::AppState;
use crate::app_windows;
use crate::pane_outbox::{self, OutboxMessage};
use crate::point::box_window;
use crate::point::message;
use crate::point::session::{PointState, SESSION};
use crate::point::shot;
use crate::point::target::TargetFacts;
use crate::point::window::{self, PreviewInfo};
use crate::point::Pick;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

/// The pane is gone from the registry (closed between the list and the send).
const NO_PANE: &str = "This pane is not open any more.";
/// A screenshot that cannot be copied into the agent's folder. The message still goes, without it.
const COPY_FAILED: &str = "The screenshot could not be copied into the agent's folder.";

/// Opens the preview of a dev server pane's port in its own window. The frontend arms it after.
#[tauri::command(async)]
pub fn point_open_preview(
    app: AppHandle,
    _state: State<'_, AppState>,
    pane_id: String,
    port: u16,
) -> Result<PreviewInfo, String> {
    window::open(&app, &pane_id, port, false)
}

/// The Point box's state: picks, the "To:" list, the default pane and the notice.
#[tauri::command]
pub fn point_state(app: AppHandle, _state: State<'_, AppState>) -> PointState {
    window::current_state(&app)
}

/// Focuses the newest preview window and arms its picker.
#[tauri::command]
pub fn point_arm(app: AppHandle) -> Result<(), String> {
    window::arm_newest(&app)
}

/// Removes one pick (its screenshot file goes too) and returns the new state.
#[tauri::command]
pub fn point_remove_pick(app: AppHandle, _state: State<'_, AppState>, pick_id: String) -> PointState {
    window::remove_pick(&app, &pick_id)
}

/// The message text for the picks and the request, as the target pane's folder sees it. Used by Copy.
#[tauri::command]
pub fn point_compose(state: State<'_, AppState>, target_pane_id: String, request: String) -> Result<String, String> {
    let target = target_of(&state, &target_pane_id)?;
    compose_text(live_picks(), &request, Path::new(&target.cwd))
}

/// Sends the message to the target pane through the pane outbox. On success the session ends (its
/// screenshots stay, the message names them) and the preview shows a short note.
#[tauri::command]
pub async fn point_send(
    app: AppHandle,
    state: State<'_, AppState>,
    target_pane_id: String,
    request: String,
) -> Result<OutboxMessage, String> {
    let target = target_of(&state, &target_pane_id)?;
    let picks = live_picks();
    let preview_id = picks.last().map(|p| p.preview_id.clone()).unwrap_or_default();
    let text = compose_text(picks, &request, Path::new(&target.cwd))?;
    let note = sent_note(&target);
    let host = LiveOutbox { app: app.clone(), registry: state.registry.clone() };
    let sent = tauri::async_runtime::spawn_blocking(move || send_with(&host, &target_pane_id, &text))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))??;
    SESSION.lock().discard_keep_files();
    box_window::hide(&app);
    window::toast(&app, &preview_id, &note);
    Ok(sent)
}

/// "Edit in Chat": keeps the composed text as that pane's draft, ends the session, and opens the pane's
/// Chat tab. The composer takes the draft (`point_take_draft`).
#[tauri::command]
pub fn point_edit_in_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    target_pane_id: String,
    request: String,
) -> Result<(), String> {
    let target = target_of(&state, &target_pane_id)?;
    let text = compose_text(live_picks(), &request, Path::new(&target.cwd))?;
    {
        let mut session = SESSION.lock();
        session.put_draft(&target_pane_id, text);
        session.discard_keep_files();
    }
    box_window::hide(&app);
    app_windows::open_cockpit_window(&app, Some(&target_pane_id), true);
    let _ = app.emit("point://draft", serde_json::json!({ "paneId": target_pane_id }));
    Ok(())
}

/// Hands a pane its pending draft, once.
#[tauri::command]
pub fn point_take_draft(pane_id: String) -> Option<String> {
    SESSION.lock().take_draft(&pane_id)
}

/// Throws the session away: deletes its screenshots, hides the box and turns the pickers off.
#[tauri::command]
pub fn point_discard(app: AppHandle) {
    window::discard_all(&app);
}

/// Hides the Point box. The session stays.
#[tauri::command]
pub fn point_box_hide(app: AppHandle) {
    box_window::hide(&app);
}

/// Opens the last link the preview refused, in the browser. Only http and https (checked again there).
#[tauri::command]
pub fn point_open_blocked_link(_app: AppHandle) -> Result<(), String> {
    let url = SESSION.lock().blocked_url().map(str::to_string).ok_or("No blocked link to open.")?;
    crate::app_lifecycle::open_external_url(url)
}

// ---------------------------------------------------------------- helpers

/// The one way a message reaches a pane: the pane outbox.
trait SendHost: Send + Sync {
    fn send(&self, pane_id: &str, text: &str) -> Result<OutboxMessage, String>;
}

/// The live outbox (`pane_outbox::global`).
struct LiveOutbox {
    app: AppHandle,
    registry: Arc<Mutex<PaneRegistry>>,
}

impl SendHost for LiveOutbox {
    fn send(&self, pane_id: &str, text: &str) -> Result<OutboxMessage, String> {
        pane_outbox::global(&self.registry, &self.app).send(pane_id, text)
    }
}

/// Hands the text to the pane outbox, once. Its error is returned unchanged (the person reads it under the box).
fn send_with(host: &dyn SendHost, pane_id: &str, text: &str) -> Result<OutboxMessage, String> {
    host.send(pane_id, text)
}

/// The pane the person picked in the "To:" list, from the live registry.
fn target_of(state: &AppState, pane_id: &str) -> Result<TargetFacts, String> {
    let infos = state.registry.lock().list();
    window::target_facts(&infos).into_iter().find(|t| t.pane_id == pane_id).ok_or_else(|| NO_PANE.to_string())
}

/// The picks that wait for a request. An expired session has none (the box shows none either).
fn live_picks() -> Vec<Pick> {
    let session = SESSION.lock();
    if session.is_expired(now_ms()) {
        Vec::new()
    } else {
        session.picks().to_vec()
    }
}

/// The picks with their screenshots copied into the agent's folder, so the path in the message is one the
/// agent can open. A screenshot that cannot be copied is dropped with a reason; the message still goes.
fn picks_for_folder(picks: Vec<Pick>, cwd: &Path) -> Vec<Pick> {
    picks
        .into_iter()
        .map(|mut pick| {
            if let Some(shot) = pick.shot.clone() {
                match shot::copy_into(&shot, cwd) {
                    Ok(copied) => pick.shot = Some(copied),
                    Err(_) => {
                        pick.shot = None;
                        pick.no_shot_reason = Some(COPY_FAILED);
                    }
                }
            }
            pick
        })
        .collect()
}

/// The message text for these picks and this request, for the agent in `cwd`.
fn compose_text(picks: Vec<Pick>, request: &str, cwd: &Path) -> Result<String, String> {
    let picks = picks_for_folder(picks, cwd);
    let refs: Vec<&Pick> = picks.iter().collect();
    message::compose(&refs, request, cwd)
}

/// The short note the preview shows after a send.
fn sent_note(target: &TargetFacts) -> String {
    format!("Sent to {} \u{b7} {}. It waits until the agent is free.", target.agent_label, target.title)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::{PickFacts, Rect, Shot, Viewport};
    use std::path::PathBuf;

    /// Records every call. Point must make exactly one, and only to the outbox.
    #[derive(Default)]
    struct FakeOutbox {
        calls: std::sync::Mutex<Vec<(String, String)>>,
        answer: Option<String>,
    }

    impl SendHost for FakeOutbox {
        fn send(&self, pane_id: &str, text: &str) -> Result<OutboxMessage, String> {
            self.calls.lock().unwrap().push((pane_id.to_string(), text.to_string()));
            match &self.answer {
                Some(refusal) => Err(refusal.clone()),
                None => Ok(OutboxMessage {
                    id: "m1".into(),
                    text: text.into(),
                    created_at: 0,
                    state: crate::pane_outbox::State::Queued,
                    waiting_for: None,
                    error: None,
                }),
            }
        }
    }

    fn pick(shot: Option<Shot>) -> Pick {
        Pick {
            id: "p1".into(),
            preview_id: "preview-1".into(),
            facts: PickFacts {
                page: "localhost:5173/pricing".into(),
                url: "http://localhost:5173/pricing".into(),
                title: "Pricing".into(),
                tag: "button".into(),
                label: "button.card-cta".into(),
                selector: "button.card-cta".into(),
                text: "Get started".into(),
                html: "<button>".into(),
                rect: Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 },
                viewport: Viewport { w: 800.0, h: 600.0, dpr: 1.0 },
                attrs: vec![],
                styles: vec![],
                component: None,
                source_hint: None,
            },
            source: None,
            shot,
            no_shot_reason: None,
            created_ms: 0,
        }
    }

    #[test]
    fn send_makes_one_outbox_call_with_the_target_and_text() {
        let host = FakeOutbox::default();
        let sent = send_with(&host, "pane-7", "make it green").expect("the outbox accepts");
        assert_eq!(sent.text, "make it green");
        let calls = host.calls.lock().unwrap().clone();
        assert_eq!(calls, vec![("pane-7".to_string(), "make it green".to_string())]);
    }

    #[test]
    fn a_refused_send_gives_the_outbox_sentence_back_unchanged() {
        let host = FakeOutbox { answer: Some("The agent is working. Sends when the turn ends.".into()), ..Default::default() };
        let err = send_with(&host, "pane-7", "make it green").unwrap_err();
        assert_eq!(err, "The agent is working. Sends when the turn ends.");
        assert_eq!(host.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn no_picks_or_no_request_give_the_plain_sentences() {
        let cwd = Path::new("C:/project");
        assert_eq!(compose_text(vec![], "make it green", cwd).unwrap_err(), "Point at something first.");
        assert_eq!(compose_text(vec![pick(None)], "  ", cwd).unwrap_err(), "Write what should change.");
    }

    #[test]
    fn a_screenshot_that_cannot_be_copied_is_dropped_with_a_reason() {
        // The source file does not exist, so the copy fails; the pick stays, without its shot.
        let missing = Shot {
            abs_path: PathBuf::from("point-pf13-missing/pick-20261028-000000-1.png"),
            rel_path: ".puppet-master/shots/pick-20261028-000000-1.png".into(),
            width: 10,
            height: 10,
        };
        let agent = std::env::temp_dir().join(format!("pm-pf13-agent-{}", uuid::Uuid::new_v4().simple()));
        let out = picks_for_folder(vec![pick(Some(missing))], &agent);
        assert!(out[0].shot.is_none());
        assert_eq!(out[0].no_shot_reason, Some(COPY_FAILED));
    }

    #[test]
    fn the_sent_note_names_the_agent_and_the_pane() {
        let target = TargetFacts {
            pane_id: "pane-7".into(),
            agent_label: "Codex".into(),
            title: "fix-pricing".into(),
            cwd: "C:/project".into(),
            kind: crate::pane_outbox::PaneKind::Agent,
            run_controls: false,
            last_output_at: None,
        };
        assert_eq!(sent_note(&target), "Sent to Codex \u{b7} fix-pricing. It waits until the agent is free.");
    }
}
