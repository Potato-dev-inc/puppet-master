//! `/accounts`: list, add, rename and remove the user's agent accounts (see `crate::accounts`).
//! Not in the MCP catalog: agents only pass `account` to `run_agent`; people manage accounts.

use super::{Ctx, RouteResult};
use crate::accounts::{self, Store};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct AddBody {
    agent_id: String,
    label: String,
    #[serde(default)]
    home: Option<String>,
}

#[derive(Deserialize)]
struct RenameBody {
    id: String,
    label: String,
}

#[derive(Deserialize)]
struct RemoveBody {
    id: String,
    #[serde(default)]
    delete_home: bool,
}

fn bad(message: String) -> (u16, Value) {
    (400, json!({"code": "INVALID_ARGUMENT", "error": message.clone(), "message": message}))
}

fn body<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, (u16, Value)> {
    serde_json::from_value(value.clone()).map_err(|err| bad(format!("invalid body: {err}")))
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    respond(cx.method, cx.segments, &cx.request_body, &Store::default_location())
}

/// The route table, over an explicit store so tests never touch the real app data folder.
pub(in crate::bridge) fn respond(method: &str, segments: &[&str], request: &Value, store: &Store) -> Option<RouteResult> {
    let account = |a: accounts::Account| (200, serde_json::to_value(a).unwrap_or_default());
    match (method, segments) {
        ("GET", ["accounts"]) => Some(Ok((200, accounts::overview(store)))),
        ("POST", ["accounts"]) => Some(
            body::<AddBody>(request)
                .and_then(|b| store.add(&b.agent_id, &b.label, b.home.as_deref()).map(account).map_err(bad)),
        ),
        ("POST", ["accounts", "rename"]) => Some(
            body::<RenameBody>(request).and_then(|b| store.rename(&b.id, &b.label).map(account).map_err(bad)),
        ),
        ("POST", ["accounts", "remove"]) => Some(body::<RemoveBody>(request).and_then(|b| {
            store
                .remove(&b.id, b.delete_home)
                .map(|deleted| (200, json!({"removed": true, "home_deleted": deleted})))
                .map_err(bad)
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(store: &Store, method: &str, path: &str, body: Value) -> (u16, Value) {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match respond(method, &segments, &body, store).expect("route exists") {
            Ok(reply) | Err(reply) => reply,
        }
    }

    /// Request -> response transcript of the accounts routes (printed with --nocapture).
    #[test]
    fn accounts_routes_transcript() {
        let root = std::env::temp_dir().join(format!("pm-accounts-route-{}", uuid::Uuid::new_v4()));
        let store = Store::at(&root);

        let (status, overview) = call(&store, "GET", "/accounts", json!({}));
        println!("GET /accounts -> {status} {overview}");
        assert_eq!(status, 200);
        assert_eq!(overview["accounts"], json!([]));
        let agents: Vec<&str> = overview["agents"].as_array().unwrap().iter().map(|a| a["agent_id"].as_str().unwrap()).collect();
        assert!(agents.contains(&"claude") && agents.contains(&"codex") && !agents.contains(&"opencode"));
        let claude = overview["agents"].as_array().unwrap().iter().find(|a| a["agent_id"] == "claude").unwrap();
        assert_eq!(claude["home_env"], "CLAUDE_CONFIG_DIR");
        assert!(claude["login_hint"].as_str().unwrap().contains("log in"));

        let (status, added) = call(&store, "POST", "/accounts", json!({"agent_id": "claude", "label": "Work"}));
        println!("POST /accounts {{claude, Work}} -> {status} {added}");
        assert_eq!(status, 200);
        let id = added["id"].as_str().unwrap().to_string();

        let (status, dup) = call(&store, "POST", "/accounts", json!({"agent_id": "claude", "label": "work"}));
        println!("POST /accounts {{claude, work}} -> {status} {dup}");
        assert_eq!(status, 400);
        let (status, unsupported) = call(&store, "POST", "/accounts", json!({"agent_id": "opencode", "label": "x"}));
        println!("POST /accounts {{opencode, x}} -> {status} {unsupported}");
        assert_eq!(status, 400);
        assert!(unsupported["message"].as_str().unwrap().contains("does not support accounts"));
        let (status, _) = call(&store, "POST", "/accounts", json!({"label": "no agent"}));
        assert_eq!(status, 400);

        let (status, renamed) = call(&store, "POST", "/accounts/rename", json!({"id": id, "label": "Work (Max)"}));
        println!("POST /accounts/rename -> {status} {renamed}");
        assert_eq!((status, renamed["label"].as_str()), (200, Some("Work (Max)")));

        let home = added["home"].as_str().unwrap().to_string();
        let (status, removed) = call(&store, "POST", "/accounts/remove", json!({"id": id}));
        println!("POST /accounts/remove -> {status} {removed}");
        assert_eq!((status, removed["home_deleted"].as_bool()), (200, Some(false)));
        assert!(std::path::Path::new(&home).is_dir(), "the folder survives a plain remove");
        let (status, _) = call(&store, "POST", "/accounts/remove", json!({"id": id}));
        assert_eq!(status, 400, "already removed");

        assert!(respond("GET", &["accounts", "nope"], &json!({}), &store).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
