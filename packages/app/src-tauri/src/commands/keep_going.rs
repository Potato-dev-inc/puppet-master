//! Tauri commands for Keep going (`crate::handoff::keep_going`, SPEC n32 section 3.6).
//!
//! Desktop window only: no bridge route and no MCP tool. Each command checks its input and then
//! calls `live` (the hand-over) or `live` / `store` (records, settings, briefs). The brief is read
//! only here, through its own command (S3).

use crate::audit::{self, Who};
use crate::handoff::keep_going::live;
use crate::handoff::keep_going::model::{Offer, Settings, Target, ERR_NOT_HERE, ERR_NO_TARGET};
use crate::handoff::keep_going::view::{HandoverView, Lineage};
use crate::questions::{self, now_ms};
use tauri::AppHandle;

/// The move-back switch is per hand-over in the sheet, but the engine reads the saved setting.
/// Until that is wired, a switch that differs from the setting is refused instead of ignored.
const ERR_BACK_SWITCH: &str = "KEEP_GOING_BACK_SWITCH";
const ERR_NO_BRIEF: &str = "KEEP_GOING_NO_BRIEF";
const ERR_SAVE_FAILED: &str = "KEEP_GOING_SAVE_FAILED";
/// Longest pane id accepted.
const PANE_ID_MAX: usize = 128;

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
        Err(fail(ERR_NOT_HERE, "That pane isn't open any more."))
    }
}

/// `agent` or `agent@account` from the window's text.
fn target_of(text: &str) -> Result<Target, String> {
    Target::parse(text).ok_or_else(|| fail(ERR_NO_TARGET, "That agent isn't a valid choice."))
}

/// Every target in the list must be a valid id. Repeats and the cap are handled by the store.
fn clean_settings(settings: &Settings) -> Result<Settings, String> {
    let valid = settings.order.iter().all(|target| Target::parse(&target.key()).as_ref() == Some(target));
    if !valid {
        return Err(fail(ERR_NO_TARGET, "One agent in the list isn't valid. Remove it and try again."));
    }
    Ok(settings.clone())
}

/// The move-back switch must match the saved setting (see `ERR_BACK_SWITCH`).
fn check_back(back: bool, settings: &Settings) -> Result<(), String> {
    if back == settings.ask_back {
        Ok(())
    } else {
        Err(fail(
            ERR_BACK_SWITCH,
            "Move-back can't be changed for one hand-over yet. Set it in Settings > Agents > Keep going, then try again.",
        ))
    }
}

/// The stored brief of record `id`. A bad or unknown id is refused with no path in the message.
fn brief_of(id: &str) -> Result<String, String> {
    live::brief_text(id).map_err(|_| fail(ERR_NO_BRIEF, "That brief isn't here."))
}

/// Run blocking work off the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| fail("INTERNAL", &e.to_string()))
}

/// The limit card the person answered in the sheet. Dismissed so it does not also start a second
/// hand-over. Only a limit card of this same pane counts.
fn dismiss_limit_card(app: &AppHandle, card_id: &str, pane_id: &str) {
    let now = now_ms();
    let store = questions::store();
    let Some(question) = store.get(card_id, now) else { return };
    if question.pane_id.as_deref() != Some(pane_id) || question.limit.is_none() {
        return;
    }
    if store.dismiss(card_id, now).is_ok() {
        questions::routes::publish_changed(app);
    }
}

/// The picker rows for a pane (`[]` with an error for a pane that cannot hand over).
#[tauri::command]
pub async fn keep_going_targets(pane_id: String) -> Result<Vec<Offer>, String> {
    let pane_id = checked_pane(&pane_id)?.to_string();
    blocking(move || live::targets(&pane_id))
        .await?
        .map_err(|e| fail(&e.code, &e.message))
}

/// Start a hand-over the person chose in the sheet. Guards and the brief run now, so a refusal
/// comes back as an error. The rest runs on a worker. Returns the `Stopping` record.
#[tauri::command]
pub async fn keep_going_continue(
    app: AppHandle,
    pane_id: String,
    target: String,
    back: bool,
    card_id: Option<String>,
) -> Result<HandoverView, String> {
    let pane_id = checked_pane(&pane_id)?.to_string();
    let target = target_of(&target)?;
    check_back(back, &live::settings())?;
    let started = blocking({
        let pane_id = pane_id.clone();
        move || live::begin(&pane_id, target, Who::Desktop)
    })
    .await?
    .map_err(|e| fail(&e.code, &e.message))?;
    if let Some(card_id) = card_id {
        dismiss_limit_card(&app, &card_id, &pane_id);
    }
    Ok(HandoverView::of(&started))
}

/// Both sides of a pane's hand-overs: "Continued from" and "Continued on". A bad id reads as none.
#[tauri::command]
pub async fn keep_going_lineage(pane_id: String) -> Result<Lineage, String> {
    if checked_pane(&pane_id).is_err() {
        return Ok(Lineage { from: None, to: None });
    }
    Ok(live::lineage(&pane_id))
}

/// The stored brief (already redacted) of one hand-over. Desktop window only (S3).
#[tauri::command]
pub async fn keep_going_brief(id: String) -> Result<String, String> {
    brief_of(&id)
}

#[tauri::command]
pub async fn keep_going_settings_get() -> Result<Settings, String> {
    Ok(live::settings())
}

/// Save the settings. Bad targets are refused; repeats are dropped and the list is capped by the
/// store. The move-back switch change is audited.
#[tauri::command]
pub async fn keep_going_settings_set(settings: Settings) -> Result<Settings, String> {
    let clean = clean_settings(&settings)?;
    let before = live::settings();
    let saved = live::save_settings(&clean).map_err(|_| fail(ERR_SAVE_FAILED, "The Keep going settings could not be saved."))?;
    audit::record_setting("keep_going.ask_back", "Keep going: ask to move it back", before.ask_back, saved.ask_back);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::keep_going::model::SETTINGS_ORDER_CAP;

    fn target(agent: &str) -> Target {
        Target { agent_id: agent.into(), account: None }
    }

    #[test]
    fn target_text_must_be_a_valid_id() {
        assert_eq!(target_of("codex").unwrap(), target("codex"));
        assert_eq!(target_of(" codex@work ").unwrap(), Target { agent_id: "codex".into(), account: Some("work".into()) });
        let long = "x".repeat(65);
        for bad in ["a@b@c", "../x", "", "@work", "codex@", long.as_str()] {
            let error = target_of(bad).expect_err(bad);
            assert!(error.starts_with("KEEP_GOING_NO_TARGET: "), "{error}");
        }
    }

    #[test]
    fn pane_ids_must_be_plain() {
        assert!(checked_pane("3f2a9c1e-0000-4000-8000-000000000001").is_ok());
        assert!(checked_pane("companion-1_a.b").is_ok());
        assert!(checked_pane(&"p".repeat(PANE_ID_MAX)).is_ok());
        let long = "p".repeat(PANE_ID_MAX + 1);
        for bad in ["", "pane 1", "../x", "a/b", "a\\b", long.as_str()] {
            let error = checked_pane(bad).expect_err(bad);
            assert!(error.starts_with("KEEP_GOING_NOT_HERE: "), "{error}");
        }
    }

    #[test]
    fn settings_targets_must_parse() {
        let good = Settings { order: vec![target("codex"), Target { agent_id: "codex".into(), account: Some("work".into()) }], ask_back: false };
        assert_eq!(clean_settings(&good).unwrap(), good);
        let bad = Settings { order: vec![target("codex"), target("../x")], ask_back: true };
        assert!(clean_settings(&bad).unwrap_err().starts_with("KEEP_GOING_NO_TARGET: "));
        let crooked = Settings { order: vec![Target { agent_id: "codex".into(), account: Some("a@b".into()) }], ask_back: true };
        assert!(clean_settings(&crooked).is_err());
    }

    #[test]
    fn repeats_and_a_long_list_pass_validation_for_the_store_to_trim() {
        let many = Settings { order: (0..SETTINGS_ORDER_CAP + 2).map(|n| target(&format!("a{n}"))).collect(), ask_back: true };
        assert_eq!(clean_settings(&many).unwrap().order.len(), SETTINGS_ORDER_CAP + 2);
        let twice = Settings { order: vec![target("codex"), target("codex")], ask_back: true };
        assert!(clean_settings(&twice).is_ok());
    }

    #[test]
    fn move_back_switch_must_match_the_setting() {
        let saved = Settings { order: vec![], ask_back: true };
        assert!(check_back(true, &saved).is_ok());
        let error = check_back(false, &saved).unwrap_err();
        assert!(error.starts_with("KEEP_GOING_BACK_SWITCH: "), "{error}");
    }

    #[test]
    fn a_brief_with_a_bad_id_is_refused() {
        for bad in ["../x", "kg-zzzz", "", "kg-000000000001/../../x"] {
            let error = brief_of(bad).expect_err(bad);
            assert_eq!(error, "KEEP_GOING_NO_BRIEF: That brief isn't here.", "{bad}");
        }
    }
}
