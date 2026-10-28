//! Point and Fix tests for the session (picks, TTL, drafts, state) and the rule that preview windows
//! get no Tauri capabilities (SPEC n32 S3).
use crate::point::box_window;
use crate::point::session::{blocked_notice, Session, REPLACED_NOTICE};
use crate::point::target::Target;
use crate::point::window::{preview_label, PREVIEW_PREFIX};
use crate::point::{Pick, PickFacts, Rect, Shot, SourceRef, Viewport, MAX_PICKS, SESSION_TTL_MS};
use std::path::PathBuf;

fn facts(label: &str) -> PickFacts {
    PickFacts {
        page: "localhost:5173/pricing".into(),
        url: "http://localhost:5173/pricing".into(),
        title: "Pricing".into(),
        tag: "button".into(),
        label: label.into(),
        selector: "main > button".into(),
        text: "Get started".into(),
        html: "<button>".into(),
        rect: Rect { x: 10.0, y: 10.0, w: 100.0, h: 40.0 },
        viewport: Viewport { w: 1280.0, h: 800.0, dpr: 1.0 },
        attrs: vec![],
        styles: vec![],
        component: Some("PricingCard".into()),
        source_hint: None,
    }
}

fn pick(session: &mut Session, with_shot: bool) -> Pick {
    let n = session.next_seq();
    Pick {
        id: format!("pick-{n}"),
        preview_id: "preview-1".into(),
        facts: facts(&format!("button.b{n}")),
        source: Some(SourceRef { rel_path: "src/Card.tsx".into(), line: Some(12) }),
        shot: with_shot.then(|| Shot {
            abs_path: PathBuf::from(format!("shots/pick-{n}.png")),
            rel_path: format!(".puppet-master/shots/pick-{n}.png"),
            width: 10,
            height: 10,
        }),
        no_shot_reason: (!with_shot).then_some("Screenshots need Windows for now."),
        created_ms: 0,
    }
}

fn target(id: &str) -> Target {
    Target {
        pane_id: id.into(),
        agent_label: "Agent".into(),
        title: "Fix pricing".into(),
        same_folder: true,
        can_send: true,
        reason: None,
    }
}

fn no_thumb(_: &Shot) -> Option<String> {
    None
}

#[test]
fn a_session_holds_at_most_three_picks_and_the_fourth_replaces_the_oldest() {
    let mut session = Session::new();
    let ids: Vec<String> = (0..MAX_PICKS)
        .map(|i| {
            let p = pick(&mut session, true);
            let id = p.id.clone();
            let added = session.add_pick(p, 1_000 + i as u64);
            assert!(added.dropped_shots.is_empty());
            id
        })
        .collect();
    assert_eq!(session.picks().len(), 3);

    let fourth = pick(&mut session, true);
    let fourth_id = fourth.id.clone();
    let added = session.add_pick(fourth, 2_000);
    assert_eq!(added.dropped_shots, vec![PathBuf::from("shots/pick-1.png")]);
    let now: Vec<&str> = session.picks().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(now, vec![ids[1].as_str(), ids[2].as_str(), fourth_id.as_str()]);
    let state = session.state_with(2_000, vec![], None, &no_thumb);
    assert_eq!(state.notice.as_deref(), Some(REPLACED_NOTICE));
}

#[test]
fn a_new_pick_clears_the_old_notice() {
    let mut session = Session::new();
    session.set_blocked("example.com", "https://example.com/");
    assert!(session.blocked_url().is_some());
    let p = pick(&mut session, false);
    session.add_pick(p, 5);
    assert_eq!(session.state_with(5, vec![], None, &no_thumb).notice, None);
    assert_eq!(session.blocked_url(), None);
}

#[test]
fn the_session_ends_after_the_ttl_and_returns_its_shots() {
    let mut session = Session::new();
    let p = pick(&mut session, true);
    session.add_pick(p, 1_000);
    assert!(!session.is_expired(1_000 + SESSION_TTL_MS), "exactly the TTL is still alive");
    assert!(session.expire(1_000 + SESSION_TTL_MS).is_none());
    assert!(session.is_expired(1_001 + SESSION_TTL_MS));
    // An expired session shows nothing even before it is swept.
    assert!(session.state_with(1_001 + SESSION_TTL_MS, vec![], None, &no_thumb).picks.is_empty());
    let shots = session.expire(1_001 + SESSION_TTL_MS).expect("expired");
    assert_eq!(shots, vec![PathBuf::from("shots/pick-1.png")]);
    assert!(session.is_empty());
    assert!(session.expire(99_999_999).is_none(), "nothing left to expire");
}

#[test]
fn a_pick_after_the_ttl_starts_a_fresh_session() {
    let mut session = Session::new();
    let old = pick(&mut session, true);
    session.add_pick(old, 0);
    let fresh = pick(&mut session, true);
    let fresh_id = fresh.id.clone();
    let added = session.add_pick(fresh, SESSION_TTL_MS + 1);
    assert_eq!(added.dropped_shots, vec![PathBuf::from("shots/pick-1.png")]);
    assert_eq!(session.picks().len(), 1);
    assert_eq!(session.picks()[0].id, fresh_id);
    assert!(!session.is_expired(SESSION_TTL_MS + 2));
}

#[test]
fn removing_a_pick_hands_it_back_and_restarts_the_clock() {
    let mut session = Session::new();
    for _ in 0..2 {
        let p = pick(&mut session, true);
        session.add_pick(p, 100);
    }
    let removed = session.remove("pick-1", 50_000).expect("found");
    assert_eq!(removed.shot.unwrap().abs_path, PathBuf::from("shots/pick-1.png"));
    assert!(session.remove("pick-1", 50_000).is_none());
    assert!(!session.is_expired(100 + SESSION_TTL_MS + 1), "removal counts as activity");
    assert_eq!(session.picks().len(), 1);
}

#[test]
fn discard_returns_every_shot_and_keeps_drafts() {
    let mut session = Session::new();
    for has_shot in [true, false, true] {
        let p = pick(&mut session, has_shot);
        session.add_pick(p, 1);
    }
    session.put_draft("pane-a", "text".into());
    let shots = session.discard();
    assert_eq!(shots, vec![PathBuf::from("shots/pick-1.png"), PathBuf::from("shots/pick-3.png")]);
    assert!(session.is_empty());
    assert_eq!(session.take_draft("pane-a").as_deref(), Some("text"));
}

#[test]
fn discard_keep_files_ends_the_session_without_naming_shots() {
    let mut session = Session::new();
    let p = pick(&mut session, true);
    session.add_pick(p, 1);
    session.set_blocked("example.com", "https://example.com/");
    session.discard_keep_files();
    assert!(session.is_empty());
    let state = session.state_with(2, vec![], None, &no_thumb);
    assert!(state.picks.is_empty() && state.notice.is_none());
    assert_eq!(session.blocked_url(), None);
}

#[test]
fn a_draft_is_taken_once_and_the_newest_wins() {
    let mut session = Session::new();
    assert_eq!(session.take_draft("pane-a"), None);
    session.put_draft("pane-a", "first".into());
    session.put_draft("pane-a", "second".into());
    session.put_draft("pane-b", "other".into());
    assert_eq!(session.take_draft("pane-a").as_deref(), Some("second"));
    assert_eq!(session.take_draft("pane-a"), None);
    assert_eq!(session.take_draft("pane-b").as_deref(), Some("other"));
}

#[test]
fn a_blocked_link_gives_a_notice_and_only_web_links_are_kept_for_the_browser() {
    let mut session = Session::new();
    session.set_blocked("example.com", "https://example.com/a?b=1");
    assert_eq!(session.blocked_url(), Some("https://example.com/a?b=1"));
    let state = session.state_with(1, vec![], None, &no_thumb);
    assert_eq!(state.notice, Some(blocked_notice("example.com")));
    assert!(state.notice.unwrap().starts_with("Blocked a link to example.com."));

    for bad in ["file:///C:/Users/you/secret.txt", "javascript:alert(1)", "ftp://example.com/", "http://exa mple.com/", "mailto:a@b.c", ""] {
        session.set_blocked("example.com", bad);
        assert_eq!(session.blocked_url(), None, "{bad}");
    }
}

#[test]
fn state_carries_the_pick_fields_the_box_shows() {
    let mut session = Session::new();
    let with = pick(&mut session, true);
    let without = pick(&mut session, false);
    session.add_pick(with, 1);
    session.add_pick(without, 2);
    let targets = vec![target("pane-a"), target("pane-b")];
    let state = session.state_with(3, targets.clone(), Some("pane-b".into()), &|shot: &Shot| {
        Some(format!("data:image/png;base64,{}", shot.width))
    });
    assert_eq!(state.targets, targets);
    assert_eq!(state.default_target.as_deref(), Some("pane-b"));
    let first = &state.picks[0];
    assert_eq!(first.id, "pick-1");
    assert_eq!(first.page, "localhost:5173/pricing");
    assert_eq!(first.label, "button.b1");
    assert_eq!(first.text, "Get started");
    assert_eq!(first.component.as_deref(), Some("PricingCard"));
    assert_eq!(first.source, Some(SourceRef { rel_path: "src/Card.tsx".into(), line: Some(12) }));
    assert_eq!(first.thumb.as_deref(), Some("data:image/png;base64,10"));
    assert_eq!(first.shot_path.as_deref(), Some(".puppet-master/shots/pick-1.png"));
    assert_eq!(first.no_shot_reason, None);
    let second = &state.picks[1];
    assert_eq!(second.thumb, None);
    assert_eq!(second.shot_path, None);
    assert_eq!(second.no_shot_reason.as_deref(), Some("Screenshots need Windows for now."));
}

#[test]
fn thumbnails_are_read_once_per_pick() {
    use std::cell::Cell;
    let mut session = Session::new();
    let p = pick(&mut session, true);
    session.add_pick(p, 1);
    let reads = Cell::new(0);
    let load = |_: &Shot| {
        reads.set(reads.get() + 1);
        Some("data:image/png;base64,AA".to_string())
    };
    for _ in 0..3 {
        session.state_with(2, vec![], None, &load);
    }
    assert_eq!(reads.get(), 1);
}

#[test]
fn the_state_json_uses_camel_case_names() {
    let mut session = Session::new();
    let p = pick(&mut session, true);
    session.add_pick(p, 1);
    let state = session.state_with(2, vec![target("pane-a")], Some("pane-a".into()), &no_thumb);
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["defaultTarget"], "pane-a");
    assert_eq!(json["targets"][0]["paneId"], "pane-a");
    assert_eq!(json["targets"][0]["canSend"], true);
    assert_eq!(json["picks"][0]["shotPath"], ".puppet-master/shots/pick-1.png");
    assert!(json["picks"][0].get("noShotReason").is_some());
}

#[test]
fn a_new_session_gets_a_new_id_and_a_second_pick_keeps_it() {
    let mut session = Session::new();
    assert_eq!(session.state_with(0, vec![], None, &no_thumb).session, None);
    let first = pick(&mut session, false);
    session.add_pick(first, 1_000);
    let id = session.state_with(1_000, vec![], None, &no_thumb).session;
    assert!(id.is_some(), "a pick starts a session");
    let second = pick(&mut session, false);
    session.add_pick(second, 2_000);
    assert_eq!(session.state_with(2_000, vec![], None, &no_thumb).session, id, "the same session keeps its id");

    session.discard_keep_files();
    assert_eq!(session.state_with(3_000, vec![], None, &no_thumb).session, None, "a discarded session is over");
    let third = pick(&mut session, false);
    session.add_pick(third, 4_000);
    let next = session.state_with(4_000, vec![], None, &no_thumb).session;
    assert!(next.is_some() && next != id, "a new session has a new id");
}

#[test]
fn expiry_means_no_session_and_the_next_pick_starts_a_new_one() {
    let mut session = Session::new();
    let p = pick(&mut session, false);
    session.add_pick(p, 0);
    let old = session.state_with(1, vec![], None, &no_thumb).session;
    let late = SESSION_TTL_MS + 1;
    assert_eq!(session.state_with(late, vec![], None, &no_thumb).session, None, "expired shows no session before the sweep");
    assert!(session.expire(late).is_some());
    assert_eq!(session.state_with(late, vec![], None, &no_thumb).session, None);
    let fresh = pick(&mut session, false);
    session.add_pick(fresh, late);
    let new = session.state_with(late, vec![], None, &no_thumb).session;
    assert!(new.is_some() && new != old, "the pick after expiry is a new session");
}

#[test]
fn removing_the_last_pick_keeps_the_session_until_it_expires() {
    let mut session = Session::new();
    let p = pick(&mut session, false);
    session.add_pick(p, 100);
    let id = session.state_with(100, vec![], None, &no_thumb).session;
    session.remove("pick-1", 200);
    assert_eq!(session.state_with(200, vec![], None, &no_thumb).session, id, "no pick left, same session");
    let q = pick(&mut session, false);
    session.add_pick(q, 200 + SESSION_TTL_MS + 1);
    let next = session.state_with(200 + SESSION_TTL_MS + 1, vec![], None, &no_thumb).session;
    assert!(next.is_some() && next != id, "an idle empty session ends too");
}

// ---------------------------------------------------------------- S3: no capabilities for previews

fn capability_files() -> Vec<(String, serde_json::Value)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("capabilities folder") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).expect("read capability");
            let json = serde_json::from_str(&text).expect("capability is JSON");
            files.push((path.file_name().unwrap().to_string_lossy().into_owned(), json));
        }
    }
    assert!(!files.is_empty(), "no capability files found");
    files
}

fn string_list(json: &serde_json::Value, key: &str) -> Vec<String> {
    json.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Does any `windows` or `webviews` pattern of any capability match `label`? (Tauri uses glob patterns.)
fn capability_matching(label: &str) -> Option<String> {
    for (file, json) in capability_files() {
        for key in ["windows", "webviews"] {
            for pattern in string_list(&json, key) {
                let glob = glob::Pattern::new(&pattern).expect("pattern");
                if glob.matches(label) {
                    return Some(format!("{file}: {key} pattern {pattern:?} matches {label:?}"));
                }
            }
        }
    }
    None
}

fn has_key(json: &serde_json::Value, key: &str) -> bool {
    match json {
        serde_json::Value::Object(map) => map.contains_key(key) || map.values().any(|v| has_key(v, key)),
        serde_json::Value::Array(items) => items.iter().any(|v| has_key(v, key)),
        _ => false,
    }
}

#[test]
fn preview_windows_match_no_capability() {
    for n in [1, 2, 3, 10, 999] {
        let label = preview_label(n);
        assert!(label.starts_with(PREVIEW_PREFIX));
        assert_eq!(capability_matching(&label), None);
    }
    assert_eq!(capability_matching(&format!("{PREVIEW_PREFIX}x")), None);
    assert_eq!(capability_matching(PREVIEW_PREFIX), None);
}

#[test]
fn no_capability_names_previews_or_opens_a_window_to_remote_pages() {
    for (file, json) in capability_files() {
        assert!(!has_key(&json, "remote"), "{file}: a `remote` key would give web pages Tauri access");
        assert!(!json.to_string().contains("preview"), "{file}: names a preview window");
        for pattern in string_list(&json, "windows") {
            assert!(pattern != "*" && pattern != "**", "{file}: window pattern {pattern:?} would match preview windows");
        }
    }
}

#[test]
fn the_capability_check_itself_catches_a_wildcard() {
    // The matcher the pin uses really does match globs: a `preview-*` or `*` pattern would be found.
    assert!(glob::Pattern::new("preview-*").unwrap().matches(&preview_label(1)));
    assert!(glob::Pattern::new("*").unwrap().matches(&preview_label(1)));
    assert!(glob::Pattern::new("detached-pane-*").unwrap().matches("detached-pane-abc"));
    assert!(!glob::Pattern::new("detached-pane-*").unwrap().matches(&preview_label(1)));
}

#[test]
fn the_box_is_never_taken_for_a_preview() {
    assert!(!box_window::LABEL.starts_with(PREVIEW_PREFIX));
    assert_eq!(box_window::URL, "index.html?pointbox=1");
}

#[test]
fn tauri_config_creates_no_preview_windows() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let windows = json["app"]["windows"].as_array().cloned().unwrap_or_default();
    assert!(windows.iter().all(|w| !w["label"].as_str().unwrap_or("").starts_with(PREVIEW_PREFIX)));
}
