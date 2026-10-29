//! `GET /review-desk`: the Review Desk summary on the paired phone (SPEC n33 3.9, section 4 S1, S5, S14).
//!
//! Read only. The answer is `view::phone_view` of the same desk the window shows: the counts and at most
//! `PHONE_ITEMS` rows of words and numbers (title cut, strict redaction, no keys, pane ids, project names,
//! paths, file names or diffs). Keep, Ask to fix and Throw away are desktop actions only; nothing in this
//! file reaches them.
//!
//! Who may call:
//! - A paired phone with `watch`, the default cap of a GET (`route_need`). Without it the auth refuses first.
//! - A plain program on this computer (`Caller::Local`).
//! - Never an MCP session (an agent): refused with 403 `REVIEW_DESK_NOT_FOR_AGENTS`.
//! - Any other method than GET: 405.
//!
//! Every request first goes through `authorize_caller`, so the bridge-auth invariant holds the same as for
//! every other route: a request with a forwarder header, a foreign `Host` or a foreign `Origin` needs a token.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::{Ctx, RouteResult};
use crate::mobile_pairing::Caller;
use crate::pty::PaneRegistry;
use crate::review_desk::gather::gather;
use crate::review_desk::live::{DeskIo, LiveIo};
use crate::review_desk::model::Clean;
use crate::review_desk::view::{phone_view, DeskView};

/// What the route touches. The live one is [`Live`]; tests use a fake with a fixed desk.
pub(crate) trait DeskPort {
    /// The desk as the window shows it, gathered now.
    fn desk(&self) -> DeskView;
    /// Strict redaction for one line of text, the same the desk uses for its titles.
    fn clean(&self) -> Clean;
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let port = Live { registry: cx.registry.clone(), app: cx.app.clone() };
    handle_request(cx.method, cx.segments, cx.headers, cx.peer_loopback, cx.session, &port)
}

/// `None` when the request is not `/review-desk`.
pub(crate) fn handle_request(
    method: &str,
    raw_segments: &[&str],
    headers: &str,
    peer_loopback: bool,
    session: Option<&str>,
    port: &dyn DeskPort,
) -> Option<RouteResult> {
    if crate::mobile_pairing::normalize_bridge_segments(raw_segments) != ["review-desk"] {
        return None;
    }
    match crate::mobile_pairing::authorize_caller(headers, peer_loopback, raw_segments, method) {
        Ok(Caller::Public) => return Some(not_for_agents()),
        Ok(Caller::Local | Caller::Device { .. }) => {}
        Err(refusal) => return Some(Err(refusal)),
    }
    if session.is_some() {
        return Some(not_for_agents());
    }
    if method != "GET" {
        return Some(Err((405, json!({ "code": "METHOD_NOT_ALLOWED", "error": "METHOD_NOT_ALLOWED", "message": "Use GET." }))));
    }
    Some(read(port))
}

fn read(port: &dyn DeskPort) -> RouteResult {
    let view = phone_view(&port.desk(), &port.clean());
    let value = serde_json::to_value(view).unwrap_or(Value::Null);
    Ok((200, crate::redact::json(value).into_inner()))
}

fn not_for_agents() -> RouteResult {
    Err((
        403,
        json!({
            "code": "REVIEW_DESK_NOT_FOR_AGENTS",
            "error": "review_desk_not_for_agents",
            "message": "Review Desk is for the person. An agent cannot read it from here.",
        }),
    ))
}

/// The live port: the same gather the window uses, over the app's pane registry.
struct Live {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
}

impl Live {
    fn io(&self) -> LiveIo {
        LiveIo { registry: self.registry.clone(), app: self.app.clone() }
    }
}

impl DeskPort for Live {
    fn desk(&self) -> DeskView {
        gather(&self.io())
    }

    fn clean(&self) -> Clean {
        self.io().clean()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile_pairing::{init_pairing_store, pairing_store, route_need, Cap, Caps, Need, PairRequestBody};
    use crate::redact::RedactContext;
    use crate::review_desk::live::clean_line;
    use crate::review_desk::model::{Facts, FileRow, Item, ItemKey, Keep, Level, Risk, Source, Throw};
    use crate::review_desk::view::desk_view;

    /// Answers with a fixed desk and the strict redaction of `clean_line` (no app state).
    struct Fake {
        desk: DeskView,
    }

    impl DeskPort for Fake {
        fn desk(&self) -> DeskView {
            self.desk.clone()
        }

        fn clean(&self) -> Clean {
            Clean { text: Box::new(|raw| clean_line(raw, &RedactContext::default(), None)) }
        }
    }

    const LOCAL: &str = "Host: 127.0.0.1:17321\r\n";
    const FORWARDED: &str = "Host: 127.0.0.1:17321\r\nX-Forwarded-For: 203.0.113.9\r\n";

    fn row(path: &str, added: u32, deleted: u32) -> FileRow {
        FileRow { path: path.into(), status: 'M', added, deleted, binary: false }
    }

    /// A finished copy with one or more file rows. The project folder and the pane are local only.
    fn item(key: &str, title: &str, level: Level, files: Vec<FileRow>) -> Item {
        let added = files.iter().map(|f| f.added).sum();
        let deleted = files.iter().map(|f| f.deleted).sum();
        Item {
            key: ItemKey::parse(key).expect("valid key"),
            source: Source::Copy,
            fingerprint: "abcdef012345".into(),
            title: title.into(),
            agent_label: "Fixture agent".into(),
            account: None,
            where_label: "Separate copy of main".into(),
            project: r"C:\Users\you\app".into(),
            pane_id: Some("pane-1".into()),
            finished_at_ms: 100,
            turns: 2,
            group: None,
            facts: Facts { files_total: files.len() as u32, added, deleted, files: Some(files), lines_scanned: true, ..Facts::default() },
            risk: Risk { tier: 7, level, reasons: Vec::new() },
            keep: Keep::No { why: "Held.".into() },
            throw: Throw::No { why: "Closed.".into() },
            fix: Ok("pane-1".into()),
        }
    }

    fn fake(items: Vec<Item>) -> Fake {
        Fake { desk: desk_view(&items, Vec::new()) }
    }

    fn call(method: &str, segments: &[&str], headers: &str, loopback: bool, session: Option<&str>, port: &dyn DeskPort) -> (u16, Value) {
        match handle_request(method, segments, headers, loopback, session, port).expect("a review desk route") {
            Ok(ok) => ok,
            Err(err) => err,
        }
    }

    fn refused(result: (u16, Value), status: u16, code: &str) {
        assert_eq!(result.0, status, "{}", result.1);
        assert_eq!(result.1["code"], code, "{}", result.1);
    }

    /// A paired phone with `caps`, paired the same way the night route tests pair one.
    fn pair(caps: Caps) -> String {
        if pairing_store().is_none() {
            let dir = std::env::temp_dir().join(format!("pm-review-bridge-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let _ = init_pairing_store(dir.join("pairing.json"));
        }
        let store = pairing_store().unwrap();
        let mut guard = store.lock();
        let s = guard.create_pairing_session_with_caps("http://127.0.0.1:1".into(), caps);
        let paired = guard
            .pair_device(PairRequestBody { pairing_code: s.pairing_code, device_name: "Test phone".into(), device_public_key: s.server_public_key })
            .unwrap();
        paired.device_token
    }

    /// Headers of a request from a phone over the tunnel: a remote peer with its bearer token.
    fn phone_headers(token: &str) -> String {
        format!("Host: pm.example.com\r\nAuthorization: Bearer {token}\r\n")
    }

    fn two_items() -> Fake {
        fake(vec![
            item("copy:h-tamper", "Make the cart total round", Level::High, vec![row("src/secret-name.ts", 3, 1), row("test/cart.test.ts", 12, 8)]),
            item("copy:h-small", "Rename helper", Level::Low, vec![row("src/other.ts", 1, 1)]),
        ])
    }

    #[test]
    fn the_read_is_the_phone_view_of_the_desk() {
        let port = two_items();
        let (status, body) = call("GET", &["review-desk"], LOCAL, true, None, &port);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["count"], 2);
        assert_eq!(body["high"], 1);
        assert_eq!(body["items"][0]["title"], "Make the cart total round");
        assert_eq!(body["items"][0]["levelWord"], "High risk");
        assert_eq!(body["items"][0]["agentLabel"], "Fixture agent");
        assert_eq!(body["items"][0]["whereWord"], "Copy");
        assert_eq!(body["items"][0]["files"], 2);
        assert_eq!(body["items"][0]["added"], 15);
        assert_eq!(body["items"][0]["deleted"], 9);
        assert_eq!(body["items"][1]["levelWord"], "Low risk");
        // The bridge prefix is the same route.
        assert_eq!(call("GET", &["bridge", "review-desk"], LOCAL, true, None, &port).0, 200);
    }

    #[test]
    fn no_key_pane_project_path_or_file_name_reaches_the_phone() {
        let port = two_items();
        let (_, body) = call("GET", &["review-desk"], LOCAL, true, None, &port);
        let text = body.to_string();
        assert!(!text.contains('\\'), "a backslash leaked: {text}");
        for leak in ["copy:h-tamper", "pane-1", "Users", "C:", "src/", "secret-name", "cart.test", "abcdef012345", "Separate copy"] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
        for field in ["key", "paneId", "project", "fingerprint", "files"] {
            // `files` is the count on the phone, a number, never the row list.
            if field == "files" {
                assert!(body["items"][0]["files"].is_number(), "{text}");
            } else {
                assert!(body["items"][0].get(field).is_none(), "{field} leaked: {text}");
                assert!(body.get(field).is_none(), "{field} leaked: {text}");
            }
        }
        assert_eq!(body.as_object().unwrap().keys().count(), 3, "{text}");
    }

    #[test]
    fn a_title_of_200_characters_is_cut_at_60() {
        // Plain words: a long run of one letter looks like a token to the strict redactor and is masked.
        let long = format!("{}{}", "Make the cart total round to cents. ".repeat(5), "Make the cart total ");
        assert_eq!(long.chars().count(), 200);
        let port = fake(vec![item("copy:h1", &long, Level::Check, vec![row("a.ts", 1, 0)])]);
        let (status, body) = call("GET", &["review-desk"], LOCAL, true, None, &port);
        assert_eq!(status, 200, "{body}");
        let title = body["items"][0]["title"].as_str().unwrap();
        assert_eq!(title.chars().count(), 60, "{title}");
    }

    #[test]
    fn a_seeded_secret_in_a_title_is_masked_on_the_phone() {
        let port = fake(vec![item(
            "copy:h1",
            "Fix the key sk-ant-api03-SeededSecret0123456789 in config",
            Level::Check,
            vec![row("a.ts", 1, 0)],
        )]);
        let (_, body) = call("GET", &["review-desk"], LOCAL, true, None, &port);
        let title = body["items"][0]["title"].as_str().unwrap();
        assert!(!title.contains("SeededSecret"), "{title}");
        assert!(!body.to_string().contains("SeededSecret"), "{body}");
    }

    #[test]
    fn an_empty_desk_is_zero_items() {
        let port = fake(Vec::new());
        let (status, body) = call("GET", &["review-desk"], LOCAL, true, None, &port);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, json!({ "count": 0, "high": 0, "items": [] }));
    }

    #[test]
    fn an_mcp_session_is_refused_with_its_own_code() {
        let port = two_items();
        refused(call("GET", &["review-desk"], LOCAL, true, Some("mcp-session-1"), &port), 403, "REVIEW_DESK_NOT_FOR_AGENTS");
    }

    #[test]
    fn every_method_but_get_is_405() {
        let port = two_items();
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            refused(call(method, &["review-desk"], LOCAL, true, None, &port), 405, "METHOD_NOT_ALLOWED");
        }
    }

    #[test]
    fn a_forwarder_or_a_foreign_host_or_origin_on_loopback_is_not_trusted() {
        let port = two_items();
        for headers in [FORWARDED, "Host: evil.example.com\r\n", "Host: 127.0.0.1:17321\r\nOrigin: https://evil.example.com\r\n"] {
            assert_eq!(call("GET", &["review-desk"], headers, true, None, &port).0, 401, "{headers}");
        }
    }

    #[test]
    fn a_paired_phone_reads_with_watch_and_is_refused_without_it() {
        let port = two_items();

        let none = pair(Caps::none());
        refused(call("GET", &["review-desk"], &phone_headers(&none), false, None, &port), 403, "DEVICE_NOT_ALLOWED");

        let watch = pair(Caps::watch_and_answer());
        let (status, body) = call("GET", &["review-desk"], &phone_headers(&watch), false, None, &port);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["count"], 2);
    }

    #[test]
    fn the_route_is_watch_by_default() {
        assert_eq!(route_need("GET", &["review-desk"]), Need::Cap(Cap::Watch));
        assert!(!Caps::none().satisfies(route_need("GET", &["review-desk"])));
        assert!(Caps::watch_and_answer().satisfies(route_need("GET", &["review-desk"])));
    }

    #[test]
    fn other_routes_are_left_alone() {
        let port = two_items();
        for segments in [&["review"][..], &["review-desk", "keep"][..], &["review-desks"][..], &["review-desk", "1"][..], &["night"][..]] {
            assert!(handle_request("GET", segments, LOCAL, true, None, &port).is_none(), "{segments:?}");
        }
    }
}
