//! `/companions` routes for the phone (spec section 11). Reading needs the device cap `watch`, keeping
//! or dropping a memory needs `answer`, wake and sleep need `control` (`mobile_pairing/caps.rs`).
//!
//! Phone only in spirit: every reply is the phone copy (no folder, redacted text). Making and editing
//! companions is desktop-only (Tauri commands). An MCP session is refused here as well as by
//! `mcp_sessions::authorize`, so the gate does not depend on one layer.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::bridge::http::query_param;
use crate::companion::model::CompanionError;
use crate::companion::{live, view, Service};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments.first() != Some(&"companions") {
        return None;
    }
    let service = live::service();
    respond(cx.method, cx.segments, cx.query, &cx.request_body, cx.session, service.as_deref())
}

/// The route table without the Tauri context, so tests can drive it over a fake host.
pub(in crate::bridge) fn respond(
    method: &str,
    segments: &[&str],
    query: &str,
    body: &Value,
    session: Option<&str>,
    service: Option<&Service>,
) -> Option<RouteResult> {
    if segments.first() != Some(&"companions") {
        return None;
    }
    if session.is_some_and(|s| !s.trim().is_empty()) {
        return Some(Err(local_only()));
    }
    let Some(service) = service else {
        return Some(Err((503, json!({ "code": "COMPANION_NOT_READY", "error": "Companions are not ready yet.", "message": "Companions are not ready yet." }))));
    };
    Some(match (method, &segments[1..]) {
        ("GET", []) => Ok((200, json!({ "v": 1, "companions": service.views(true) }))),
        ("GET", [id]) => service.view(id, true).map(|v| (200, json!(v))).map_err(fail),
        ("GET", [id, "feed"]) => {
            let since = query_param(query, "since").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
            service
                .feed_list(id, since)
                .map(|items| (200, crate::redact::json(json!({ "items": items })).into_inner()))
                .map_err(fail)
        }
        ("GET", [id, "memory"]) => memory_reply(service.memory_view(id, true)),
        ("POST", [id, "memory", "pending", item]) => match body.get("decision").and_then(Value::as_str) {
            Some(word @ ("keep" | "drop")) => memory_reply(service.pending_decide(id, item, word == "keep", true)),
            _ => Err(bad("decision must be \"keep\" or \"drop\"")),
        },
        ("POST", [id, "wake"]) => service.wake(id).map(|v| (200, json!(view::phone_copy(&v)))).map_err(fail),
        ("POST", [id, "sleep"]) => service.sleep(id).map(|v| (200, json!(view::phone_copy(&v)))).map_err(fail),
        _ => Err((404, json!({ "code": "NOT_FOUND", "error": "not found", "message": "No such companion route." }))),
    })
}

fn memory_reply(result: Result<crate::companion::manage::MemoryView, CompanionError>) -> RouteResult {
    result
        .map_err(fail)
        .and_then(|memory| serde_json::to_value(memory).map_err(|e| (500, json!({ "code": "INTERNAL", "error": e.to_string() }))))
        .map(|value| (200, crate::redact::json(value).into_inner()))
}

fn local_only() -> (u16, Value) {
    let message = "companions can only be used from the app and the paired phone, not MCP sessions";
    (403, json!({ "code": "COMPANION_LOCAL_ONLY", "error": message, "message": message, "recoverable": false }))
}

fn bad(message: &str) -> (u16, Value) {
    (400, json!({ "code": "INVALID_ARGUMENT", "error": message, "message": message }))
}

fn fail(error: CompanionError) -> (u16, Value) {
    let status = match error.code {
        "COMPANION_NOT_FOUND" => 404,
        "COMPANION_PENDING_STALE" | "COMPANION_MEMORY_FULL" | "COMPANION_SKILLS_FULL" => 409,
        "COMPANION_SKILL_ON_COMPUTER" => 403,
        "COMPANION_STORE_INVALID" => 500,
        _ => 400,
    };
    (status, json!({ "code": error.code, "error": error.message, "message": error.message }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::test_host::service_with_one;

    fn get(service: &Service, path: &str, session: Option<&str>) -> RouteResult {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        respond("GET", &segments, query, &json!({}), session, Some(service)).expect("a companions route")
    }

    fn post(service: &Service, path: &str, body: Value, session: Option<&str>) -> RouteResult {
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        respond("POST", &segments, "", &body, session, Some(service)).expect("a companions route")
    }

    #[test]
    fn an_mcp_session_gets_403_companion_local_only_on_every_route() {
        let (service, _host, _dir) = service_with_one();
        for path in ["/companions", "/companions/mochi", "/companions/mochi/feed", "/companions/mochi/memory"] {
            let (status, body) = get(&service, path, Some("mcp-session-1")).unwrap_err();
            assert_eq!(status, 403, "{path}");
            assert_eq!(body["code"], "COMPANION_LOCAL_ONLY", "{path}");
        }
        for path in ["/companions/mochi/wake", "/companions/mochi/sleep", "/companions/mochi/memory/pending/abc"] {
            let (status, body) = post(&service, path, json!({ "decision": "keep" }), Some("mcp-session-1")).unwrap_err();
            assert_eq!((status, body["code"].as_str()), (403, Some("COMPANION_LOCAL_ONLY")), "{path}");
        }
        // The local app (no session header) is served.
        assert!(get(&service, "/companions", None).is_ok());
    }

    #[test]
    fn the_mcp_gate_itself_refuses_companion_paths_with_a_403() {
        // Before any route runs: `mcp_sessions::authorize` for a request that carries an MCP session id.
        for path in ["/companions", "/companions/mochi/memory", "/companions/mochi/wake"] {
            let method = if path.ends_with("wake") { "POST" } else { "GET" };
            let error = crate::mcp_sessions::authorize(Some("mcp-session-1"), method, path, &json!({})).unwrap_err();
            assert_eq!(error.code, "COMPANION_LOCAL_ONLY", "{path}");
            let (status, body) = crate::bridge::errors::operation_http_error(error);
            assert_eq!(status, 403);
            assert_eq!(body["code"], "COMPANION_LOCAL_ONLY");
        }
        // The app's own session (no MCP header) passes the gate.
        assert!(crate::mcp_sessions::authorize(None, "GET", "/companions", &json!({})).is_ok());
    }

    #[test]
    fn the_phone_view_has_no_folder_in_the_list_or_the_single_reply() {
        let (service, _host, _dir) = service_with_one();
        let (status, list) = get(&service, "/companions", None).unwrap();
        assert_eq!(status, 200);
        assert_eq!(list["v"], 1);
        let text = list.to_string();
        assert!(!text.contains("\"folder\""), "{text}");
        assert!(!text.contains("PuppetMaster"), "{text}");
        assert_eq!(list["companions"][0]["id"], "mochi");
        assert_eq!(list["companions"][0]["folderName"], "mochi");
        let (_, one) = get(&service, "/companions/mochi", None).unwrap();
        assert!(one.get("folder").is_none());
        assert!(one.get("checkins").is_none(), "the phone copy has no check-in list, only the next one");
        let (status, missing) = get(&service, "/companions/nobody", None).unwrap_err();
        assert_eq!((status, missing["code"].as_str()), (404, Some("COMPANION_NOT_FOUND")));
    }

    #[test]
    fn feed_and_memory_are_redacted_and_the_phone_memory_has_no_outside_changes() {
        let (service, host, dir) = service_with_one();
        host.advance_ms(5_000);
        service.tick();
        std::fs::write(
            std::path::Path::new(&service.folder_of("mochi").unwrap()).join("memory").join("pending.md"),
            "- note: Ren likes tea.\n",
        )
        .unwrap();
        let (_, memory) = get(&service, "/companions/mochi/memory", None).unwrap();
        assert_eq!(memory["pending"][0]["text"], "Ren likes tea.");
        assert_eq!(memory["outsideChanges"], json!([]));
        assert_eq!(memory["caps"]["memory"], 2200);
        let (_, feed) = get(&service, "/companions/mochi/feed?since=0", None).unwrap();
        assert!(feed["items"].as_array().is_some());
        drop(dir);
    }

    #[test]
    fn no_companion_route_serves_the_content_of_today_md() {
        let (service, host, dir) = service_with_one();
        host.advance_ms(5_000);
        service.tick();
        // A line a person (or a bug) put there with a key in it: the routes read the memory files, the feed and the
        // view, never this file, so the text cannot reach a phone from here.
        let marker = "SECRET-DAY-LINE-ZX81";
        let home = std::path::PathBuf::from(service.folder_of("mochi").unwrap());
        std::fs::write(home.join("today.md"), format!("# What my agents did today\n09:05 · A · b · {marker}\n")).unwrap();
        for path in ["/companions", "/companions/mochi", "/companions/mochi/memory", "/companions/mochi/feed?since=0"] {
            let (status, body) = get(&service, path, None).unwrap();
            assert_eq!(status, 200, "{path}");
            assert!(!body.to_string().contains(marker), "{path} must not carry today.md");
        }
        drop(dir);
    }

    // ---- no secret reaches the phone (safety net) ----

    // Made at run time so the secret scanners of this repository see no key-shaped literal.
    fn fake_anthropic_key() -> String {
        format!("sk-ant-api03-{}", "Zq9Xv3LmN7pRt2YhB5cWd8KfJ4sG6aUe1oIxTn0V")
    }
    fn fake_github_token() -> String {
        format!("ghp_{}", "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8")
    }
    fn fake_github_pat() -> String {
        format!("github_pat_{}", "11ABCDEFG0aBcDeFgHiJkL_mNoPqRsTuVwXyZ0123456789")
    }

    /// One finished turn on the fake host: the agent works, then says `text`, then reads idle.
    fn agent_says(service: &Service, host: &crate::companion::test_host::FakeHost, text: &str) {
        use crate::companion::host::Reply;
        use crate::companion::keeper::{Pane, Status};
        let tick = |ms: i64| {
            host.advance_ms(ms);
            service.tick();
        };
        host.set_pane("companion-mochi", Pane::Alive(Status::Idle));
        tick(1_000);
        host.set_pane("companion-mochi", Pane::Alive(Status::Running));
        tick(1_000);
        let id = format!("{}:0", host.reply.lock().as_ref().map_or(1, |r| r.id.len() + 1));
        *host.reply.lock() = Some(Reply { id, text: text.into() });
        host.set_pane("companion-mochi", Pane::Alive(Status::Idle));
        tick(1_000);
        tick(2_000);
        tick(1_000);
    }

    #[test]
    fn the_phone_reads_skill_titles_never_their_steps_and_cannot_approve_one() {
        let (service, _host, dir) = service_with_one();
        let home = std::path::PathBuf::from(service.folder_of("mochi").unwrap());
        crate::companion::skills::add(&dir, "mochi", &home, "Tea", "SECRET-STEPS").unwrap();
        std::fs::write(home.join("memory").join("pending.md"), "- skill: Weekly report
  SECRET-STEPS two
").unwrap();

        let (status, memory) = get(&service, "/companions/mochi/memory", None).unwrap();
        assert_eq!(status, 200);
        assert!(!memory.to_string().contains("SECRET-STEPS"), "{memory}");
        let waiting = &memory["pending"][0];
        assert_eq!((waiting["kind"].as_str(), waiting["text"].as_str()), (Some("skill"), Some("Weekly report")));
        assert!(waiting.get("body").is_none());

        let path = format!("/companions/mochi/memory/pending/{}", waiting["id"].as_str().unwrap());
        let (status, body) = post(&service, &path, json!({ "decision": "keep" }), None).unwrap_err();
        assert_eq!((status, body["code"].as_str()), (403, Some("COMPANION_SKILL_ON_COMPUTER")));
        assert!(!home.join("skills").join("weekly-report.md").exists());
        assert_eq!(memory["skills"][0]["title"], "Tea");
    }

    #[test]
    fn a_key_in_the_agents_reply_or_in_pending_memory_reaches_the_phone_masked() {
        let (service, host, _dir) = service_with_one();
        let (anthropic, github, pat) = (fake_anthropic_key(), fake_github_token(), fake_github_pat());
        // The first answer after a start belongs to the brief ("ready") and gets no line.
        agent_says(&service, &host, "ready");
        // The agent repeats a key it found in a file (the worst case for a feed line) ...
        agent_says(&service, &host, &format!("Found the key {anthropic} in .env, and a token {github}."));
        // ... and proposes to remember keys (a note and a user line) in the file only it may write.
        let home = std::path::PathBuf::from(service.folder_of("mochi").unwrap());
        std::fs::write(
            home.join("memory").join("pending.md"),
            format!("- note: The deploy key is {anthropic} for later.
- user: Uses the token {pat} at work.
- note: Likes tea.
"),
        )
        .unwrap();

        let (status, feed) = get(&service, "/companions/mochi/feed?since=0", None).unwrap();
        assert_eq!(status, 200);
        let lines = feed["items"].as_array().expect("feed items");
        let reply = lines.iter().find(|i| i["kind"] == "reply").unwrap_or_else(|| panic!("the reply is in the feed: {feed}"));
        assert!(reply["text"].as_str().unwrap().contains("[REDACTED]"), "the reply keeps its words around a mask: {reply}");

        let (_, memory) = get(&service, "/companions/mochi/memory", None).unwrap();
        let pending = memory["pending"].as_array().expect("pending items");
        assert_eq!(pending.len(), 3, "{memory}");
        assert!(pending.iter().any(|p| p["text"] == "Likes tea."), "an ordinary note is not touched: {memory}");

        for (what, body) in [("feed", feed.to_string()), ("memory", memory.to_string())] {
            for secret in [&anthropic, &github, &pat] {
                assert!(!body.contains(secret.as_str()), "{what} carries {secret}: {body}");
            }
            // Not even the long tail of the key (a mask that kept half a key would still leak it).
            assert!(!body.contains("Zq9Xv3LmN7pRt2Yh") && !body.contains("A1b2C3d4E5f6") && !body.contains("11ABCDEFG0aBcDe"), "{what}: {body}");
        }
    }

    #[test]
    fn a_key_in_the_approved_memory_files_reaches_the_phone_masked_too() {
        let (service, _host, _dir) = service_with_one();
        let home = std::path::PathBuf::from(service.folder_of("mochi").unwrap());
        let github = fake_github_token();
        for name in ["MEMORY.md", "USER.md", "SOUL.md"] {
            let path = home.join(name);
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, format!("{old}
A stray token {github} somebody pasted.
")).unwrap();
        }
        let (_, memory) = get(&service, "/companions/mochi/memory", None).unwrap();
        let body = memory.to_string();
        assert!(!body.contains(&github) && !body.contains("A1b2C3d4E5f6"), "{body}");
        assert!(body.contains("[REDACTED]"), "{body}");
    }

    #[test]
    fn keeping_a_memory_needs_a_decision_word_and_a_known_item() {
        let (service, _host, _dir) = service_with_one();
        let home = std::path::PathBuf::from(service.folder_of("mochi").unwrap());
        std::fs::write(home.join("memory").join("pending.md"), "- note: Likes tea.\n").unwrap();
        let (_, memory) = get(&service, "/companions/mochi/memory", None).unwrap();
        let item = memory["pending"][0]["id"].as_str().unwrap().to_string();
        let path = format!("/companions/mochi/memory/pending/{item}");
        let (status, _) = post(&service, &path, json!({}), None).unwrap_err();
        assert_eq!(status, 400);
        let (status, _) = post(&service, "/companions/mochi/memory/pending/zzzzzzzzzzzz", json!({ "decision": "keep" }), None).unwrap_err();
        assert_eq!(status, 409, "a stale id is a conflict");
        let (status, after) = post(&service, &path, json!({ "decision": "keep" }), None).unwrap();
        assert_eq!(status, 200);
        assert!(after["memory"].as_str().unwrap().contains("Likes tea."));
        assert_eq!(after["pending"], json!([]));
    }

    #[test]
    fn wake_and_sleep_reply_with_the_phone_copy() {
        let (service, _host, _dir) = service_with_one();
        let (_, slept) = post(&service, "/companions/mochi/sleep", json!({}), None).unwrap();
        assert_eq!(slept["state"], "sleeping");
        assert!(slept.get("folder").is_none());
        let (_, woke) = post(&service, "/companions/mochi/wake", json!({}), None).unwrap();
        assert!(woke.get("folder").is_none());
        assert_ne!(woke["state"], "sleeping");
    }

    #[test]
    fn other_paths_are_not_ours_and_unknown_companion_routes_are_404() {
        let (service, _host, _dir) = service_with_one();
        assert!(respond("GET", &["panes"], "", &json!({}), None, Some(&service)).is_none());
        let (status, _) = respond("DELETE", &["companions", "mochi"], "", &json!({}), None, Some(&service)).unwrap().unwrap_err();
        assert_eq!(status, 404);
        let (status, body) = respond("GET", &["companions"], "", &json!({}), None, None).unwrap().unwrap_err();
        assert_eq!((status, body["code"].as_str()), (503, Some("COMPANION_NOT_READY")));
    }
}
