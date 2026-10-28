//! `GET /chats/search?q=<text>&limit=<n>`: search across every live chat, for the phone's search screen. The answer
//! is the desktop's (`{ enabled, hits }`, see `chat_index`): what was said, redacted, with the agent, the folder name
//! (never the path), the pane and the snippet's marks. `enabled` is false when the person turned the index off;
//! then `hits` is empty and nothing was searched.
//!
//! Old chats (the imported rows of "Bring my old chats") are never searched here: the spec keeps them on the desktop.
//! The route calls `ChatIndex::search_live`, and a guard test (`session_chat/history/guard.rs`, `all_chats_search`)
//! keeps every-row searches out of the bridge.
//!
//! Reading it needs the device cap `watch` (`mobile_pairing/caps.rs`), like a pane's chat. The gate
//! (`authorize_caller`) refuses a request with no paired token before this runs.

use serde_json::json;
use tauri::Manager;

use super::{Ctx, RouteResult};
use crate::bridge::http::query_param;
use crate::chat_index::{self, ChatIndex};
use crate::commands::SessionChatState;

/// Most hits one search answers. A larger `limit` is cut to this.
pub const MAX_LIMIT: usize = 50;
/// Hits when the phone asks for no `limit` (or one that is not a number).
pub const DEFAULT_LIMIT: usize = 30;
/// Longest query, in characters. A longer one is refused, not cut: a cut query would match other things.
pub const MAX_QUERY_CHARS: usize = 200;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("GET", ["chats", "search"]) => {}
        _ => return None,
    }
    let index = cx.app.state::<SessionChatState>().index(cx.registry);
    let query = query_param(cx.query, "q").unwrap_or_default();
    let limit = query_param(cx.query, "limit");
    Some(reply(index.as_deref(), &query, limit.as_deref()))
}

/// The answer for a query and a `limit` as the URL gave them. `index` is `None` when the index could not be opened.
pub(super) fn reply(index: Option<&ChatIndex>, query: &str, limit: Option<&str>) -> RouteResult {
    if query.chars().count() > MAX_QUERY_CHARS {
        let message = format!("The search is longer than {MAX_QUERY_CHARS} characters.");
        return Err((400, json!({ "code": "QUERY_TOO_LONG", "error": message, "message": message })));
    }
    let limit = clamp_limit(limit);
    let Some(index) = index else {
        return Ok((200, json!({ "enabled": chat_index::enabled(), "hits": [] })));
    };
    // Live rows only: an old chat (imported with the desktop's "Bring my old chats") never reaches a phone.
    match index.search_live(query, limit) {
        Ok(answer) => serde_json::to_value(answer)
            .map(|value| (200, value))
            .map_err(|error| (500, json!({ "code": "INTERNAL", "error": error.to_string(), "message": error.to_string() }))),
        Err(error) => Err((500, json!({ "code": "INTERNAL", "error": error, "message": error }))),
    }
}

/// The `limit` the URL asked for, kept within `1..=MAX_LIMIT`. Missing or not a number means `DEFAULT_LIMIT`.
pub fn clamp_limit(raw: Option<&str>) -> usize {
    match raw.and_then(|value| value.trim().parse::<i64>().ok()) {
        Some(n) => n.clamp(1, MAX_LIMIT as i64) as usize,
        None => DEFAULT_LIMIT,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::chat_index::{entry_of_with, ContextLookup, ImportedChatRow, PaneContext, Strength, MAX_ITEMS};
    use crate::session_chat::{ChatItem, ItemObserver};

    struct Fixed;

    impl ContextLookup for Fixed {
        fn context(&self, _pane_id: &str) -> Option<PaneContext> {
            Some(PaneContext::of("agent-x", r"C:\Users\you\projects\shop", Some("sess-1")))
        }
    }

    /// An index in a new file in the temp folder, on or off, with one user message and one reply kept.
    fn index_with(on: bool) -> Arc<ChatIndex> {
        let path = std::env::temp_dir().join(format!("pm-chats-route-{}.sqlite", uuid::Uuid::new_v4()));
        let store = chat_index::open_store(&path, MAX_ITEMS).expect("a new index file");
        let index = ChatIndex::start_switched(store, Arc::new(Fixed), Arc::new(AtomicBool::new(on)));
        index.observe(
            "pane-7",
            &[
                ChatItem::User { id: "1:0".into(), at: None, text: "the license check lives in checks.rs".into() },
                ChatItem::Assistant { id: "2:0".into(), at: None, text: "Found the license key in checks.rs".into(), model: None, final_answer: true },
            ],
        );
        let end = Instant::now() + Duration::from_secs(5);
        while on && index.count() < 2 {
            assert!(Instant::now() < end, "the writer did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        index
    }

    fn body(result: RouteResult) -> serde_json::Value {
        result.unwrap_or_else(|(status, value)| panic!("status {status}: {value}")).1
    }

    /// One old chat (an imported row, strictly redacted, as the importer writes it) on another folder and session.
    /// Its text says "license" too, and "pelican", which no live chat says.
    fn add_old_chat(index: &ChatIndex) {
        let item = ChatItem::Assistant { id: "1:0".into(), at: None, text: "pelican license key in checks.rs".into(), model: None, final_answer: true };
        let ctx = PaneContext::of("agent-x", r"C:\Users\you\projects\old-shop", Some("old-sess-1"));
        // 2000 is the importer's cap for an imported text (`MAX_IMPORTED_TEXT_CHARS`).
        let entry = entry_of_with("import:agent-x", &ctx, &item, 500, Strength::Strict, true, 2000).expect("an entry");
        let row = ImportedChatRow {
            agent: "agent-x".into(),
            session: "old-sess-1".into(),
            folder: "old-shop".into(),
            cwd: "~/projects/old-shop".into(),
            title: "old license question".into(),
            updated_ms: 500,
            stamp: "1:500".into(),
            items: 1,
            cut_short: false,
            imported_ms: 600,
        };
        index.import_chat(&row, &[entry]).expect("the old chat is written");
    }

    #[test]
    fn the_limit_is_kept_between_1_and_50_and_defaults_to_30() {
        assert_eq!(clamp_limit(None), 30);
        assert_eq!(clamp_limit(Some("abc")), 30);
        assert_eq!(clamp_limit(Some("")), 30);
        assert_eq!(clamp_limit(Some("10")), 10);
        assert_eq!(clamp_limit(Some(" 12 ")), 12);
        assert_eq!(clamp_limit(Some("500")), MAX_LIMIT);
        assert_eq!(clamp_limit(Some("51")), 50);
        assert_eq!(clamp_limit(Some("0")), 1);
        assert_eq!(clamp_limit(Some("-7")), 1);
    }

    #[test]
    fn a_hit_names_the_agent_and_the_folder_name_never_the_path() {
        let index = index_with(true);
        let answer = body(reply(Some(&*index), "license", Some("50")));
        assert_eq!(answer["enabled"], true);
        let hits = answer["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 2, "{answer}");
        for hit in hits {
            assert_eq!(hit["paneId"], "pane-7");
            assert_eq!(hit["folder"], "shop");
            assert_eq!(hit["agent"], "agent-x");
            assert!(!hit.to_string().contains("projects"), "{hit}");
            assert!(!hit.to_string().contains(r"C:\Users"), "{hit}");
        }
    }

    #[test]
    fn an_old_chat_never_reaches_the_phone_search() {
        let index = index_with(true);
        add_old_chat(&index);
        // The desktop search sees the old chat: three rows say "license", one of them imported.
        let desktop = index.search_hits("license", 50).expect("the desktop search");
        assert_eq!(desktop.len(), 3, "{desktop:?}");
        assert_eq!(desktop.iter().filter(|hit| hit.imported).count(), 1, "{desktop:?}");
        // The phone gets the two live hits only: the same word, no imported row, no old folder or session.
        let answer = body(reply(Some(&*index), "license", Some("50")));
        let hits = answer["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 2, "{answer}");
        for hit in hits {
            assert_eq!(hit["imported"], false, "{answer}");
            assert_eq!(hit["paneId"], "pane-7", "{answer}");
        }
        for leaked in ["old-shop", "old-sess-1", "import:", "pelican"] {
            assert!(!answer.to_string().contains(leaked), "{leaked} leaked to the phone: {answer}");
        }
        // A word that only the old chat says finds nothing on the phone at all.
        let only_old = body(reply(Some(&*index), "pelican", None));
        assert_eq!(only_old, json!({ "enabled": true, "hits": [] }));
        assert_eq!(index.search_hits("pelican", 50).expect("the desktop search").len(), 1);
    }

    #[test]
    fn the_limit_caps_the_number_of_hits() {
        let index = index_with(true);
        assert_eq!(body(reply(Some(&*index), "license", Some("1")))["hits"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_query_over_200_characters_is_refused_and_200_is_fine() {
        let index = index_with(true);
        let long = "a".repeat(MAX_QUERY_CHARS + 1);
        let (status, value) = reply(Some(&*index), &long, None).unwrap_err();
        assert_eq!(status, 400);
        assert_eq!(value["code"], "QUERY_TOO_LONG");
        let edge = "a".repeat(MAX_QUERY_CHARS);
        assert_eq!(body(reply(Some(&*index), &edge, None))["enabled"], true);
    }

    #[test]
    fn when_the_index_is_off_the_answer_says_so_with_no_hits() {
        let index = index_with(false);
        let answer = body(reply(Some(&*index), "license", None));
        assert_eq!(answer, json!({ "enabled": false, "hits": [] }));
    }

    #[test]
    fn when_the_index_could_not_be_opened_search_is_off_too() {
        let answer = body(reply(None, "license", None));
        assert_eq!(answer["hits"], json!([]));
        assert!(answer["enabled"].is_boolean());
    }

    #[test]
    fn an_agent_over_mcp_is_refused_the_search_before_the_route_runs() {
        // The agents' MCP surface has no search of every chat: the gate refuses an MCP session on this path.
        let error = crate::mcp_sessions::authorize(Some("mcp-session-1"), "GET", "/chats/search?q=license", &json!({})).unwrap_err();
        assert!(error.message.contains("not exposed"), "{}", error.message);
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let index = index_with(true);
        assert_eq!(body(reply(Some(&*index), "  ", None)), json!({ "enabled": true, "hits": [] }));
    }
}
