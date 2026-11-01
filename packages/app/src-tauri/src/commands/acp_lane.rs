//! Tauri commands for the per-agent "How to run": Terminal (today's way, the default) or Chat (ACP).
//! Only the app's own webview can call these; the bridge has no route for them, so a phone or an agent
//! cannot change how an agent runs. The choice is read when a session starts without a lane of its own: the + menu's
//! default (`useAcpLanes`) and `run_agent` without `lane` (`acp::chat::surface::wants`).

use crate::acp::lane::{effective_lane, LaneStore, RunLane};
use crate::acp::overlay::OverlayStore;
use crate::acp::{self, config};
use serde_json::{json, Value};

pub(crate) fn view() -> Value {
    let overlay = OverlayStore::in_app_data();
    let mut view = lane_view(&LaneStore::in_app_data(), &overlay, acp::chat_enabled());
    // Each row says whether it has an add-on on disk and its size (the Remove sheet offers to delete it).
    if let Some(rows) = view["agents"].as_array_mut() {
        for row in rows {
            let id = row["id"].as_str().unwrap_or_default().to_string();
            if let Some(addon) = super::acp_remove::addon_info(&config::RealHost, &overlay, &id) {
                row["addon"] = addon;
            }
        }
    }
    view
}

fn row(cfg: &config::AgentConfig, chosen: &std::collections::BTreeMap<String, RunLane>, labs: bool, overlay: bool) -> Value {
    let lane = chosen.get(&cfg.id).copied().unwrap_or_default();
    let mut row = json!({
        "id": cfg.id,
        "label": cfg.label,
        "lane": lane.as_str(),
        "effective_lane": effective_lane(lane, labs, !cfg.disabled).as_str(),
        "disabled": cfg.disabled,
        "disabled_note": cfg.disabled_note,
    });
    if overlay {
        row["overlay"] = json!(true);
    }
    row
}

/// The view over given stores (tests use temp ones). `agents` = agents with an `[acp]` table, then the agents
/// served by the person's chat overlay (`overlay: true`, so the screen can offer Remove); `can_add_chat` = agents
/// that could get one.
pub(crate) fn lane_view(store: &LaneStore, overlay: &OverlayStore, labs: bool) -> Value {
    let chosen = store.all();
    let mut agents: Vec<Value> = config::builtin_agents().iter().map(|cfg| row(cfg, &chosen, labs, false)).collect();
    agents.extend(acp::overlay::overlay_agents(overlay).iter().map(|cfg| row(cfg, &chosen, labs, true)));
    let can_add: Vec<Value> = acp::overlay::addable_agents(overlay)
        .into_iter()
        .map(|a| {
            let mut v = json!({"id": a.id, "label": a.label});
            if let Some(reason) = a.reason {
                v["reason"] = json!(reason);
            }
            v
        })
        .collect();
    // Agents that can run as a terminal chat (no ACP needed), each with whether the person chose it.
    let terminal_chat: Vec<Value> = crate::terminal_chat::lane::eligible_rows()
        .into_iter()
        .map(|mut row| {
            let chosen_here = row["id"].as_str().is_some_and(|id| chosen.get(id) == Some(&RunLane::TerminalChat));
            row["chosen"] = json!(chosen_here);
            row
        })
        .collect();
    json!({ "labs_enabled": labs, "agents": agents, "can_add_chat": can_add, "terminal_chat": terminal_chat })
}

/// `{labs_enabled, agents: [{id, label, lane, effective_lane, disabled, disabled_note, overlay?}], can_add_chat: [{id, label, reason?}],
/// terminal_chat: [{id, label, reads_file, can_resume, can_answer_prompts, note, chosen}]}`. `terminal_chat` lists every agent that
/// can run as a chat over its own terminal (`crate::terminal_chat`); `chosen` = its saved lane is `terminal_chat`.
/// `agents` lists every agent that has chat: its own ACP definition, or the person's overlay (`overlay: true`).
/// `lane` is the saved choice, `effective_lane` what a new session would use (Chat needs the Labs switch).
#[tauri::command]
pub fn agent_lane_get() -> Value {
    view()
}

/// Saves one agent's choice (`terminal` | `acp` | `terminal_chat`). Returns the new view.
#[tauri::command]
pub fn agent_lane_set(id: String, lane: String) -> Result<Value, String> {
    let lane = RunLane::parse(&lane).ok_or_else(|| format!("INVALID_ARGUMENT: unknown lane {lane}"))?;
    if lane == RunLane::Acp {
        match config::find_builtin(&id) {
            None => return Err(format!("INVALID_ARGUMENT: {id} has no chat (ACP) definition")),
            Some(cfg) if cfg.disabled => return Err(format!("INVALID_ARGUMENT: chat for {id} is off: {}", cfg.disabled_note)),
            Some(_) => {}
        }
    }
    if lane == RunLane::TerminalChat {
        crate::terminal_chat::available_for(&id).map_err(|reason| format!("INVALID_ARGUMENT: {reason}"))?;
    }
    LaneStore::in_app_data().set(&id, lane)?;
    Ok(view())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (LaneStore, OverlayStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-lane-view-{}", uuid::Uuid::new_v4()));
        (LaneStore::at(dir.join("agent_lane.json")), OverlayStore::at(dir.join("acp_overlay.json")), dir)
    }

    fn ids(v: &Value, key: &str) -> Vec<String> {
        v[key].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_view_lists_who_can_add_chat_and_marks_overlay_rows() {
        let (lanes, overlay, dir) = temp();
        let before = lane_view(&lanes, &overlay, true);
        assert_eq!(before["labs_enabled"], json!(true));
        let can_add = ids(&before, "can_add_chat");
        assert!(!can_add.is_empty(), "the registry has agents with no chat table");
        let own = ids(&before, "agents");
        assert!(can_add.iter().all(|id| !own.contains(id)), "an agent with its own table is not offered");
        assert!(before["agents"].as_array().unwrap().iter().all(|a| a.get("overlay").is_none()));
        for a in before["agents"].as_array().unwrap() {
            for field in ["id", "label", "lane", "effective_lane", "disabled", "disabled_note"] {
                assert!(a.get(field).is_some(), "{field} stays");
            }
        }
        assert!(can_add.iter().all(|id| !crate::tui::is_shell(id)), "no shell is offered");

        let pick = can_add[0].clone();
        overlay.put(&pick, &acp::overlay::OverlayEntry { command: "adapter".into(), args: vec!["acp".into()], ..Default::default() }).unwrap();
        let after = lane_view(&lanes, &overlay, true);
        assert!(!ids(&after, "can_add_chat").contains(&pick), "not offered once it has an overlay");
        let row = after["agents"].as_array().unwrap().iter().find(|a| a["id"] == json!(pick)).expect("an overlay row");
        assert_eq!(row["overlay"], json!(true));
        assert_eq!(row["lane"], json!("terminal"));
        assert_eq!(row["effective_lane"], json!("terminal"));
        lanes.set(&pick, RunLane::Acp).unwrap();
        let chosen = lane_view(&lanes, &overlay, true);
        let row = chosen["agents"].as_array().unwrap().iter().find(|a| a["id"] == json!(pick)).unwrap();
        assert_eq!(row["effective_lane"], json!("acp"));
        overlay.remove(&pick).unwrap();
        assert!(ids(&lane_view(&lanes, &overlay, true), "can_add_chat").contains(&pick));
        // a bridge overlay is an ordinary overlay row (Remove works), for an agent that has a headless mode
        if let Some(h) = ids(&lane_view(&lanes, &overlay, true), "can_add_chat").into_iter().find(|id| acp::bridge::available_for(id).is_ok()) {
            overlay.put_item(&h, &acp::overlay::OverlayItem::Bridge(acp::overlay::BridgeEntry { raw: json!({"source": "bridge"}) })).unwrap();
            let v = lane_view(&lanes, &overlay, true);
            let row = v["agents"].as_array().unwrap().iter().find(|a| a["id"] == json!(h)).expect("a bridge row");
            assert_eq!(row["overlay"], json!(true));
            overlay.remove(&h).unwrap();
            assert!(ids(&lane_view(&lanes, &overlay, true), "can_add_chat").contains(&h));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_view_lists_terminal_chat_agents_and_the_choice() {
        let (lanes, overlay, dir) = temp();
        let before = lane_view(&lanes, &overlay, false);
        let rows = before["terminal_chat"].as_array().expect("a terminal_chat list");
        let claude = rows.iter().find(|r| r["id"] == json!("claude")).expect("an agent with a reader");
        for field in ["label", "reads_file", "can_resume", "can_answer_prompts", "note", "chosen"] {
            assert!(claude.get(field).is_some(), "{field}");
        }
        assert_eq!(claude["chosen"], json!(false));
        assert!(rows.iter().all(|r| !crate::tui::is_shell(r["id"].as_str().unwrap())), "no shell is offered");
        lanes.set("claude", RunLane::TerminalChat).unwrap();
        let after = lane_view(&lanes, &overlay, false);
        let claude = after["terminal_chat"].as_array().unwrap().iter().find(|r| r["id"] == json!("claude")).unwrap().clone();
        assert_eq!(claude["chosen"], json!(true));
        let _ = std::fs::remove_dir_all(dir);
    }
}
