//! Privacy and shape tests for `live_room::view` (SPEC-n32 LR-2; privacy matrix of 4 #3, card rule of 4 #4).
//! Fixtures are fake: the planted secrets are built here, and the "real-looking" user folder is a made-up name.
//! The folder is built with `concat!` so no literal `Users\<name>` sits in the source (scripts/check-no-personal-paths).
//! Every check reads the serialized JSON, the same bytes a viewer frame would carry.

use serde_json::json;

use super::view::{clean, pane_view, to_live, LiveItem, LiveState, WAITING_LINE};
use crate::pty::status::PaneStatus;
use crate::redact::RedactContext;
use crate::session_chat::detail::{PlanItem, PlanStatus, ToolDetail};
use crate::session_chat::model::{ChatItem, TurnOutcome};
use crate::tui::{Activity, ActivityKind};

const ENV_SECRET: &str = "pl4nted-env-secret-4242";
const EMAIL: &str = "jane.doe@example.test";
const USER_HOME: &str = concat!("C:\\Users\\", "Jane Doe Smith");
const USER_PATH: &str = concat!("C:\\Users\\", "Jane Doe Smith", "\\proj\\src\\a.rs");

fn api_key() -> String {
    format!("sk-ant-api03-{}", "A1".repeat(20))
}

fn opaque() -> String {
    "Qw3rT9yUi0".repeat(4)
}

fn ctx() -> RedactContext {
    RedactContext {
        env_values: vec![ENV_SECRET.to_string()],
        home: Some(USER_HOME.to_string()),
    }
}

/// One text that carries every planted secret shape at once.
fn dirty() -> String {
    format!("key {} mail {} env {} run {} at {} end", api_key(), EMAIL, ENV_SECRET, opaque(), USER_PATH)
}

/// Asserts that no planted value, no e-mail, no folder name and no `Users` segment is in `json`.
fn assert_no_secret(json: &str) {
    for planted in [ENV_SECRET.to_string(), api_key(), EMAIL.to_string(), opaque()] {
        assert!(!json.contains(&planted), "secret leaked: {planted} in {json}");
    }
    for word in ["Jane", "Smith", "Users", "example.test"] {
        assert!(!json.contains(word), "personal text leaked: {word} in {json}");
    }
}

fn user(text: &str) -> ChatItem {
    ChatItem::User { id: "u".into(), at: None, text: text.into() }
}

fn assistant(text: &str) -> ChatItem {
    ChatItem::Assistant { id: "a".into(), at: None, text: text.into(), model: None, final_answer: false }
}

fn live(item: &ChatItem, next: &mut u64) -> Option<LiveItem> {
    to_live(item, "p1", next, &ctx())
}

#[test]
fn privacy_matrix_covers_every_shared_kind() {
    let d = dirty();
    let items = vec![
        user(&d),
        assistant(&d),
        ChatItem::ToolCall {
            id: "t".into(),
            at: None,
            call_id: "c".into(),
            name: "Bash".into(),
            summary: d.clone(),
            detail: ToolDetail {
                input: Some(json!({ "command": d.clone(), "api_key": api_key() })),
                paths: vec![USER_PATH.into()],
                writes: true,
                helper: false,
            },
        },
        ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: false, output: d.clone() },
        ChatItem::TurnEnd { id: "e".into(), at: None, outcome: TurnOutcome::Failed, duration_ms: Some(81_234) },
        ChatItem::Plan {
            id: "p".into(),
            at: None,
            items: vec![PlanItem { text: d.clone(), status: Some(PlanStatus::InProgress), task_id: None }],
            replace: true,
        },
        ChatItem::Notice { id: "n".into(), at: None, notice_kind: "error".into(), text: d.clone(), url: None },
        ChatItem::Notice {
            id: "q".into(),
            at: None,
            notice_kind: "pr".into(),
            text: d.clone(),
            url: Some(format!("https://example.test/pull/{}", opaque())),
        },
    ];
    let mut next = 1;
    let rows: Vec<String> = items
        .iter()
        .map(|item| serde_json::to_string(&live(item, &mut next).expect("shared kind")).unwrap())
        .collect();
    let tags: Vec<&str> = rows
        .iter()
        .map(|row| {
            let tag = row.split("\"t\":\"").nth(1).unwrap();
            tag.split('"').next().unwrap()
        })
        .collect();
    assert_eq!(tags, ["user", "agent", "tool", "result", "end", "plan", "note", "note"]);
    for row in &rows {
        assert_no_secret(row);
    }
    // Positive check: the text is still there, cut to the file name.
    assert!(rows[0].contains("a.rs"), "{}", rows[0]);
    assert_eq!(rows[7], r#"{"t":"note","id":8,"pane":"p1","text":"Opened a pull request"}"#);
}

#[test]
fn tool_input_and_result_output_are_never_sent() {
    let mut next = 1;
    let call = ChatItem::ToolCall {
        id: "t".into(),
        at: None,
        call_id: "c".into(),
        name: "Bash".into(),
        summary: "npm test".into(),
        detail: ToolDetail {
            input: Some(json!({ "command": "npm test", "api_key": api_key() })),
            paths: vec![USER_PATH.into(), "b.rs".into()],
            writes: true,
            helper: false,
        },
    };
    let row = serde_json::to_string(&live(&call, &mut next).unwrap()).unwrap();
    assert_eq!(
        row,
        r#"{"t":"tool","id":1,"pane":"p1","name":"Bash","summary":"npm test","files":2,"writes":true}"#
    );
    assert_no_secret(&row);

    let result = ChatItem::ToolResult {
        id: "r".into(),
        at: None,
        call_id: "c".into(),
        ok: true,
        output: format!("{}\nsecond line\n", api_key()),
    };
    let row = serde_json::to_string(&live(&result, &mut next).unwrap()).unwrap();
    assert_eq!(row, r#"{"t":"result","id":2,"pane":"p1","ok":true,"lines":2}"#);
}

/// Spaced words: a run of one letter would be an opaque run and masked whole by the strict redactor.
fn words(count_chars: usize) -> String {
    "word ".repeat(count_chars / 5 + 1).chars().take(count_chars).collect()
}

#[test]
fn user_text_is_capped_at_500_chars_with_an_ellipsis() {
    let mut next = 1;
    let text = words(501);
    let long = live(&user(&text), &mut next).unwrap();
    assert_eq!(long, LiveItem::User { id: 1, pane: "p1".into(), text: format!("{}…", words(500)) });
    let exact = live(&user(&words(500)), &mut next).unwrap();
    assert_eq!(exact, LiveItem::User { id: 2, pane: "p1".into(), text: words(500) });
}

#[test]
fn agent_text_is_capped_at_1500_chars() {
    let mut next = 1;
    let row = live(&assistant(&words(1501)), &mut next).unwrap();
    match row {
        LiveItem::Agent { text, .. } => assert_eq!(text, format!("{}…", words(1500))),
        other => panic!("not an agent row: {other:?}"),
    }
}

#[test]
fn e_mail_addresses_are_masked_in_every_text_field() {
    assert_eq!(clean(&format!("write to {EMAIL}"), 200, &ctx()), "write to <email>");
    assert!(!clean(EMAIL, 60, &ctx()).contains("example"));
}

#[test]
fn folder_paths_are_cut_to_the_file_name() {
    let mut next = 1;
    let row = live(&user(&format!("Open {USER_PATH} now")), &mut next).unwrap();
    assert_eq!(row, LiveItem::User { id: 1, pane: "p1".into(), text: "Open a.rs now".into() });
    assert_eq!(clean(USER_PATH, 60, &ctx()), "a.rs");
}

#[test]
fn assistant_text_stays_plain_text() {
    let mut next = 1;
    let markup = "<img src=x onerror=alert(1)> **bold**";
    let row = live(&assistant(markup), &mut next).unwrap();
    assert_eq!(row, LiveItem::Agent { id: 1, pane: "p1".into(), text: markup.into() });
}

#[test]
fn turn_end_words_and_duration() {
    let mut next = 1;
    let cases = [(TurnOutcome::Completed, "completed"), (TurnOutcome::Interrupted, "interrupted")];
    for (outcome, word) in cases {
        let item = ChatItem::TurnEnd { id: "e".into(), at: None, outcome, duration_ms: None };
        assert_eq!(
            live(&item, &mut next).unwrap(),
            LiveItem::End { id: next - 1, pane: "p1".into(), outcome: word.into(), ms: None }
        );
    }
}

#[test]
fn plan_keeps_at_most_20_tasks_and_maps_status_words() {
    let items: Vec<PlanItem> = (0..25)
        .map(|i| PlanItem { text: format!("task {i}"), status: Some(PlanStatus::Completed), task_id: None })
        .collect();
    let item = ChatItem::Plan { id: "p".into(), at: None, items, replace: true };
    let mut next = 1;
    match live(&item, &mut next).unwrap() {
        LiveItem::Plan { tasks, replace, .. } => {
            assert_eq!(tasks.len(), 20);
            assert!(replace);
            assert_eq!(tasks[0].status.as_deref(), Some("done"));
        }
        other => panic!("not a plan row: {other:?}"),
    }
}

#[test]
fn pr_notice_drops_the_url_and_other_notice_kinds_are_not_shared() {
    let mut next = 1;
    let pr = ChatItem::Notice {
        id: "n".into(),
        at: None,
        notice_kind: "pr".into(),
        text: "PR 42".into(),
        url: Some(format!("https://example.test/{}", api_key())),
    };
    let row = serde_json::to_string(&live(&pr, &mut next).unwrap()).unwrap();
    assert!(row.contains("Opened a pull request"));
    assert_no_secret(&row);

    let other = ChatItem::Notice {
        id: "n".into(),
        at: None,
        notice_kind: "something-new".into(),
        text: "hello".into(),
        url: None,
    };
    assert_eq!(live(&other, &mut next), None);
}

#[test]
fn usage_session_and_empty_texts_are_not_shared() {
    let mut next = 1;
    let usage = ChatItem::Usage { id: "u".into(), at: None, input_tokens: 1, output_tokens: 2, context_window: None };
    let session = ChatItem::Session { id: "s".into(), at: None, model: None, cli_version: None };
    assert_eq!(live(&usage, &mut next), None);
    assert_eq!(live(&session, &mut next), None);
    assert_eq!(live(&user(""), &mut next), None);
    assert_eq!(live(&user("   \n "), &mut next), None);
    assert_eq!(live(&assistant(""), &mut next), None);
    let empty_note = ChatItem::Notice { id: "n".into(), at: None, notice_kind: "compacted".into(), text: "".into(), url: None };
    assert_eq!(live(&empty_note, &mut next), None);
    assert_eq!(next, 1, "skipped items must not use up ids");
}

#[test]
fn ids_count_up_from_the_passed_counter() {
    let mut next = 10;
    let items = [
        user("one"),
        ChatItem::Usage { id: "u".into(), at: None, input_tokens: 1, output_tokens: 1, context_window: None },
        assistant("two"),
        ChatItem::TurnEnd { id: "e".into(), at: None, outcome: TurnOutcome::Failed, duration_ms: Some(5) },
    ];
    let ids: Vec<u64> = items
        .iter()
        .filter_map(|item| live(item, &mut next))
        .map(|row| match row {
            LiveItem::User { id, .. } | LiveItem::Agent { id, .. } | LiveItem::End { id, .. } => id,
            other => panic!("unexpected row {other:?}"),
        })
        .collect();
    assert_eq!(ids, vec![10, 11, 12]);
    assert_eq!(next, 13);
}

#[test]
fn waiting_pane_shows_only_the_fixed_line() {
    let card = Activity::new(ActivityKind::Waiting, "Run rm -rf secret-folder?");
    let working = Activity::new(ActivityKind::Working, "Running rm -rf secret-folder");

    let cases = [
        pane_view("p1", "Codex", None, Some(PaneStatus::WaitingInput), Some(&working), false, &ctx()),
        pane_view("p1", "Codex", None, Some(PaneStatus::Running), Some(&card), false, &ctx()),
        pane_view("p1", "Codex", None, Some(PaneStatus::Idle), None, true, &ctx()),
    ];
    for pane in cases {
        assert_eq!(pane.state, LiveState::Waiting);
        assert_eq!(pane.line.as_deref(), Some(WAITING_LINE));
        let json = serde_json::to_string(&pane).unwrap();
        assert!(!json.contains("rm -rf"), "{json}");
        assert!(!json.contains("secret-folder"), "{json}");
    }
}

#[test]
fn pane_states_and_lines() {
    let ctx = ctx();
    let gone = pane_view("p2", "Codex", None, None, None, false, &ctx);
    assert_eq!((gone.state, gone.line), (LiveState::Gone, None));

    let failed = pane_view("p2", "Codex", None, Some(PaneStatus::Error), None, false, &ctx);
    assert_eq!((failed.state, failed.line), (LiveState::Error, None));

    let error_activity = Activity::new(ActivityKind::Error, "Build failed");
    let errored = pane_view("p2", "Codex", None, Some(PaneStatus::Running), Some(&error_activity), false, &ctx);
    assert_eq!((errored.state, errored.line), (LiveState::Error, None));

    let working = Activity::new(ActivityKind::Working, "Running npm test");
    let busy = pane_view("p2", "Codex", None, Some(PaneStatus::Running), Some(&working), false, &ctx);
    assert_eq!(busy.state, LiveState::Working);
    assert_eq!(busy.line.as_deref(), Some("Running npm test"));

    let idle_text = Activity::new(ActivityKind::Idle, "Idle");
    let idle = pane_view("p2", "Codex", None, Some(PaneStatus::Idle), Some(&idle_text), false, &ctx);
    assert_eq!((idle.state, idle.line.as_deref()), (LiveState::Idle, Some("Idle")));
}

#[test]
fn pane_label_and_title_are_cleaned_and_capped() {
    let title = format!("Fix {USER_PATH} with {}", api_key());
    let pane = pane_view("p3", "Codex", Some(&title), Some(PaneStatus::Idle), None, false, &ctx());
    assert_eq!(pane.key, "p3");
    assert_eq!(pane.label, "Codex");
    assert!(pane.title.starts_with("Fix a.rs with"), "{}", pane.title);
    assert_no_secret(&serde_json::to_string(&pane).unwrap());
    assert!(pane.title.chars().count() <= 61);

    let activity = Activity::new(ActivityKind::Working, format!("Running {}", dirty()));
    let working = pane_view("p3", "Codex", None, Some(PaneStatus::Running), Some(&activity), false, &ctx());
    assert_no_secret(&serde_json::to_string(&working).unwrap());
}

#[test]
fn clean_applies_redaction_before_the_cap() {
    let text = format!("{} tail", api_key());
    let out = clean(&text, 60, &ctx());
    assert!(!out.contains(&api_key()), "{out}");
    assert!(out.ends_with("tail"), "{out}");
}
