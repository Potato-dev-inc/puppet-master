//! Clip It (the chat's "Make clip"): a short video of one finished agent turn. Rust cuts the turn, replays the
//! terminal record and redacts it (`clip::plan`); the window only draws what it is handed. These are window commands
//! only: no bridge route, no MCP tool, no phone. Nothing is uploaded; the video is saved where the person picks.

use super::chat_export::read_items;
use super::session_chat::SessionChatState;
use super::AppState;
use crate::clip::model::{ClipPlan, MAX_STAGE_BYTES};
use crate::clip::plan::{self, PlanInput};
use crate::clip::store::ClipState;
use tauri::{AppHandle, State};

/// The chat reader is held only while the items are read; it lets go at once.
const HOLDER: &str = "clip";
const BAD_BODY: &str = "The video data did not arrive. Make the clip again.";
const BAD_KIND: &str = "Only mp4 and webm videos can be saved.";
const TOO_BIG: &str = "This video is too big to keep. Make a shorter clip.";

/// The plan of one finished turn of the pane: redacted request, steps, terminal replay and result. `user_item_id` is
/// the person's message that starts the turn; `None` is the last turn. The token in the plan gates `clip_stage` and
/// `clip_save`.
#[tauri::command]
pub async fn clip_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    clips: State<'_, ClipState>,
    pane_id: String,
    user_item_id: Option<String>,
) -> Result<ClipPlan, String> {
    let (items, agent_type, _cwd) = read_items(&app, &state, &chat, &pane_id, HOLDER).await?;
    let token = clips.plans.issue(&pane_id);
    let owned_pane = pane_id.clone();
    // The ring snapshot, the terminal replay and the redaction are CPU work: off the async threads.
    tauri::async_runtime::spawn_blocking(move || {
        let input = PlanInput {
            token,
            agent_label: crate::ambient::agent_label(&agent_type),
            items,
            user_item_id,
            summaries: crate::turn_summary::tracker().summaries(&owned_pane),
            ring: crate::flight::get(&owned_pane),
            ctx: (*crate::redact::known::context()).clone(),
        };
        plan::build(input)
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}

/// Keeps the video the window drew as the clip of `x-clip-token`. The body is the raw video bytes; `x-clip-ext` is
/// `mp4` or `webm`. Returns the byte count kept.
#[tauri::command]
pub async fn clip_stage(clips: State<'_, ClipState>, request: tauri::ipc::Request<'_>) -> Result<u64, String> {
    let header = |name: &str| request.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_string);
    let token = header("x-clip-token").ok_or_else(|| BAD_BODY.to_string())?;
    let ext = header("x-clip-ext").ok_or_else(|| BAD_KIND.to_string())?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(BAD_BODY.to_string());
    };
    if bytes.len() > MAX_STAGE_BYTES {
        return Err(TOO_BIG.to_string());
    }
    clips.plans.stage(&token, &ext, bytes.clone())
}

/// Writes the staged video of `token` to `path`, a place the save dialog returned. Returns the path written.
#[tauri::command]
pub async fn clip_save(clips: State<'_, ClipState>, token: String, path: String) -> Result<String, String> {
    clips.plans.save(&token, &path).map(|dest| dest.to_string_lossy().to_string())
}
