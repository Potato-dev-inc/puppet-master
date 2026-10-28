//! Tauri commands for Branch from here (`crate::branch_from`, SPEC n33 section 3.8).
//!
//! Desktop window only: no bridge route, no MCP tool, no card. Each command checks its input (pane id,
//! agent text, save point id, brief id) and then calls `branch_from::live`. The chat items come from
//! the same reader the chat export uses (`read_items`, holder `branch-from`). Git and the store run on
//! a blocking thread, never on the async runtime. The window's names and argument names are the ones
//! `src/lib/branch-from.ts` invokes; a test below reads that file to keep them equal.
//!
//! The change event (`branch://changed`) is sent by `branch_from::live_app::AppIo::changed`, through the
//! bridge's app handle, so these commands do not emit it again.

use super::chat_export::read_items;
use super::session_chat::SessionChatState;
use super::AppState;
use crate::audit::Who;
use crate::branch_from::live::{self, PreviewInput, StartInput};
use crate::branch_from::live_app;
use crate::branch_from::model::{
    BranchFrom, ERR_BRANCH_BAD_TARGET, ERR_BRANCH_NOT_HERE, ERR_BRANCH_NO_SAVE_POINT, MSG_BRANCH_BAD_TARGET,
    MSG_BRANCH_NOT_HERE, MSG_BRANCH_NO_SAVE_POINT,
};
use crate::branch_from::point::checkpoint_id_ok;
use crate::branch_from::store::id_ok;
use crate::branch_from::view::{BranchView, Lineage, Preview};
use crate::handoff::keep_going::model::Target;
use tauri::{AppHandle, State};

/// Longest pane id accepted (as `keep_going`).
const PANE_ID_MAX: usize = 128;
/// Longest chat item id accepted from the window.
const ITEM_ID_MAX: usize = 256;
/// The holder name the chat reader is leased under while the items are read.
const HOLDER: &str = "branch-from";

/// `CODE: message` as the other commands send errors to the window.
fn fail(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

/// A pane id is a plain id: letters, digits, `-`, `_` or `.`, at most `PANE_ID_MAX` characters.
fn checked_pane(pane_id: &str) -> Result<&str, String> {
    let plain = !pane_id.is_empty()
        && pane_id.len() <= PANE_ID_MAX
        && pane_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if plain {
        Ok(pane_id)
    } else {
        Err(fail(ERR_BRANCH_NOT_HERE, MSG_BRANCH_NOT_HERE))
    }
}

/// `agent` or `agent@account` from the window's text.
fn target_of(text: &str) -> Result<Target, String> {
    Target::parse(text).ok_or_else(|| fail(ERR_BRANCH_BAD_TARGET, MSG_BRANCH_BAD_TARGET))
}

/// The start point must be plain: a save point id is digits only, a chat turn names a non-empty item id.
/// Whether the id is one of the pane's own is decided later, by `live` (never trusted from the window).
fn checked_from(from: &BranchFrom) -> Result<(), String> {
    let ok = match from {
        BranchFrom::SavePoint { checkpoint_id } => checkpoint_id_ok(checkpoint_id),
        BranchFrom::Message { user_item_id, .. } => !user_item_id.is_empty() && user_item_id.len() <= ITEM_ID_MAX,
    };
    if ok {
        Ok(())
    } else {
        Err(fail(ERR_BRANCH_NO_SAVE_POINT, MSG_BRANCH_NO_SAVE_POINT))
    }
}

/// A branch id is `bf-` + 12 hex characters. Anything else is refused before the store is read.
fn checked_brief_id(id: &str) -> Result<&str, String> {
    if id_ok(id) {
        Ok(id)
    } else {
        Err(fail(ERR_BRANCH_NOT_HERE, "That branch has no brief."))
    }
}

/// Run blocking work off the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| fail("INTERNAL", &e.to_string()))
}

/// The chat items of `pane_id`, read once. A pane whose chat cannot be read cannot branch.
async fn chat_items(app: &AppHandle, state: &AppState, chat: &SessionChatState, pane_id: &str) -> Result<Vec<crate::session_chat::model::ChatItem>, String> {
    let (items, _, _) = read_items(app, state, chat, pane_id, HOLDER)
        .await
        .map_err(|_| fail(ERR_BRANCH_NOT_HERE, MSG_BRANCH_NOT_HERE))?;
    Ok(items)
}

/// What the branch sheet shows for a save point or a chat turn. Writes nothing. Refused for a pane that is
/// not open or whose chat cannot be read. A `target` that is given must parse.
#[tauri::command]
pub async fn branch_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_id: String,
    from: BranchFrom,
    target: Option<String>,
    message: Option<String>,
) -> Result<Preview, String> {
    let pane_id = checked_pane(&pane_id)?.to_string();
    checked_from(&from)?;
    if let Some(text) = target.as_deref() {
        target_of(text)?;
    }
    let items = chat_items(&app, &state, &chat, &pane_id).await?;
    blocking(move || {
        let input = PreviewInput {
            pane_id: &pane_id,
            from: &from,
            target: target.as_deref(),
            message: message.as_deref(),
            items: &items,
        };
        live::preview(&*live_app::app_io(), &input)
    })
    .await
}

/// Start the branch the person chose. Guards and the brief run now, so a refusal comes back as an error.
/// The copy and the agent run on a worker. Returns the `preparing` view.
#[tauri::command]
pub async fn branch_start(
    app: AppHandle,
    state: State<'_, AppState>,
    chat: State<'_, SessionChatState>,
    pane_id: String,
    from: BranchFrom,
    target: String,
    message: String,
) -> Result<BranchView, String> {
    let pane_id = checked_pane(&pane_id)?.to_string();
    checked_from(&from)?;
    target_of(&target)?;
    let items = chat_items(&app, &state, &chat, &pane_id).await?;
    let input = StartInput { pane_id, from, target, message, items, who: Who::Desktop };
    blocking(move || live::start(live_app::app_io(), input))
        .await?
        .map_err(|e| fail(&e.code, &e.message))
}

/// Both sides of a pane's branches: "Branched from" and "Branches". A bad pane id reads as none.
#[tauri::command]
pub async fn branch_lineage(pane_id: String) -> Result<Lineage, String> {
    if checked_pane(&pane_id).is_err() {
        return Ok(Lineage { from: None, children: Vec::new() });
    }
    blocking(move || live::lineage(&*live_app::app_io(), &pane_id)).await
}

/// The stored brief (already cleaned) of one branch. Desktop window only (S3). A bad id is refused.
#[tauri::command]
pub async fn branch_brief(id: String) -> Result<String, String> {
    let id = checked_brief_id(&id)?.to_string();
    blocking(move || live::brief(&*live_app::app_io(), &id))
        .await?
        .map_err(|e| fail(&e.code, &e.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's client module. The names and argument names checked here are the ones it invokes.
    const WINDOW_CLIENT: &str = include_str!("../../../src/lib/branch-from.ts");

    #[test]
    fn pane_ids_must_be_plain() {
        assert!(checked_pane("3f2a9c1e-0000-4000-8000-000000000001").is_ok());
        assert!(checked_pane("branch-pane_1.a").is_ok());
        assert!(checked_pane(&"p".repeat(PANE_ID_MAX)).is_ok());
        let long = "p".repeat(PANE_ID_MAX + 1);
        for bad in ["", "pane 1", "../x", "a/b", "a\\b", long.as_str()] {
            let error = checked_pane(bad).expect_err(bad);
            assert!(error.starts_with("BRANCH_NOT_HERE: "), "{error}");
        }
    }

    #[test]
    fn target_text_must_parse() {
        assert_eq!(target_of("codex").unwrap(), Target { agent_id: "codex".into(), account: None });
        assert_eq!(target_of(" codex@work ").unwrap(), Target { agent_id: "codex".into(), account: Some("work".into()) });
        let long = "x".repeat(65);
        for bad in ["a@b@c", "../x", "", "@work", "codex@", long.as_str()] {
            let error = target_of(bad).expect_err(bad);
            assert!(error.starts_with("BRANCH_BAD_TARGET: "), "{error}");
        }
    }

    #[test]
    fn save_point_ids_must_be_digits() {
        for good in ["1", "5", "123456789"] {
            assert!(checked_from(&BranchFrom::SavePoint { checkpoint_id: good.into() }).is_ok(), "{good}");
        }
        for bad in ["", "0a", "1234567890", "-1", "../5", "5 ", "٥"] {
            let error = checked_from(&BranchFrom::SavePoint { checkpoint_id: bad.into() }).expect_err(bad);
            assert!(error.starts_with("BRANCH_NO_SAVE_POINT: "), "{error}");
        }
    }

    #[test]
    fn chat_turn_item_ids_must_be_present_and_short() {
        let when = crate::branch_from::model::When::After;
        assert!(checked_from(&BranchFrom::Message { user_item_id: "u-1".into(), when }).is_ok());
        assert!(checked_from(&BranchFrom::Message { user_item_id: String::new(), when }).is_err());
        let long = "u".repeat(ITEM_ID_MAX + 1);
        assert!(checked_from(&BranchFrom::Message { user_item_id: long, when }).is_err());
    }

    #[test]
    fn a_brief_with_a_bad_id_is_refused() {
        assert!(checked_brief_id("bf-0123456789ab").is_ok());
        for bad in ["../x", "bf-zzzz", "", "bf-0123456789ab/../../x", "kg-0123456789ab"] {
            let error = checked_brief_id(bad).expect_err(bad);
            assert_eq!(error, "BRANCH_NOT_HERE: That branch has no brief.", "{bad}");
        }
    }

    #[test]
    fn the_window_sends_the_start_point_shape_these_commands_read() {
        let turn: BranchFrom = serde_json::from_value(serde_json::json!({"kind": "message", "user_item_id": "u-1", "when": "before"})).unwrap();
        assert!(matches!(turn, BranchFrom::Message { .. }));
        let save: BranchFrom = serde_json::from_value(serde_json::json!({"kind": "save_point", "checkpoint_id": "5"})).unwrap();
        assert!(matches!(save, BranchFrom::SavePoint { .. }));
    }

    #[test]
    fn the_window_invokes_these_command_names_with_these_argument_names() {
        for call in [
            "invoke<BranchPreview>('branch_preview', { paneId, from, target: target ?? null, message: message ?? null })",
            "invoke<BranchView>('branch_start', { paneId, from, target, message })",
            "invoke<BranchLineage>('branch_lineage', { paneId })",
            "invoke<string>('branch_brief', { id })",
        ] {
            assert!(WINDOW_CLIENT.contains(call), "the window client no longer matches: {call}");
        }
    }
}
