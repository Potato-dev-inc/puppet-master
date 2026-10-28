use super::*;

use crate::redact::RedactContext;
use crate::session_chat::detail::{PlanItem, PlanStatus, ToolDetail};
use crate::session_chat::export_text::{escape, strip_paths};
use crate::session_chat::model::{ChatItem, TurnOutcome};
use serde_json::json;

/// A token-shaped string; nothing real.
const FAKE_TOKEN: &str = "sk-ant-api03-xxxxxxxxxxxxxxxxxxxx";

fn ctx() -> RedactContext {
    RedactContext { env_values: Vec::new(), home: None }
}

fn meta() -> Meta {
    Meta { agent: "Test Agent".into(), folder: "puppet-master".into(), date: "2026-10-28".into() }
}

fn user(n: usize, text: &str) -> ChatItem {
    ChatItem::User { id: format!("u{n}"), at: None, text: text.into() }
}

fn agent(n: usize, text: &str) -> ChatItem {
    ChatItem::Assistant { id: format!("a{n}"), at: None, text: text.into(), model: None, final_answer: true }
}

fn call(n: usize, name: &str, summary: &str) -> ChatItem {
    ChatItem::ToolCall { id: format!("t{n}"), at: None, call_id: format!("c{n}"), name: name.into(), summary: summary.into(), detail: ToolDetail::default() }
}

fn render(items: &[ChatItem]) -> String {
    page(items, &meta(), TurnRange::default(), &ctx())
}

#[test]
fn page_is_one_document_with_no_script_and_no_link_to_anything_outside() {
    let items = vec![user(1, "hello"), agent(1, "hi there"), call(1, "Bash", "git status")];
    let html = render(&items);
    let lower = html.to_ascii_lowercase();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<meta charset=\"utf-8\">"));
    assert!(!lower.contains("<script"));
    for banned in ["href=", "src=", "http", "url(", "@import", "<link", "<iframe", "<img", "<object", "<embed"] {
        assert!(!lower.contains(banned), "page must not contain {banned}");
    }
}

#[test]
fn chat_text_that_looks_like_markup_shows_as_text() {
    let items = vec![user(1, "<img src=x onerror=alert(1)>"), agent(1, "<script>alert(1)</script>")];
    let html = render(&items);
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<img"));
    assert!(!html.to_ascii_lowercase().contains("<script"));
}

#[test]
fn a_fake_token_is_masked_everywhere_it_appears() {
    let items = vec![
        user(1, &format!("my key is {FAKE_TOKEN} please")),
        call(1, "Bash", &format!("curl -H {FAKE_TOKEN}")),
        ChatItem::ToolResult { id: "r1".into(), at: None, call_id: "c1".into(), ok: true, output: format!("token={FAKE_TOKEN}") },
    ];
    let html = render(&items);
    assert!(!html.contains("sk-ant-api03"), "the token must not reach the page");
    assert!(html.contains("[REDACTED]"));
}

#[test]
fn absolute_paths_shrink_to_the_last_name() {
    let items = vec![agent(
        1,
        r"Edited C:\Users\you\Desktop\work\puppet-master\src\main.rs and /home/you/projects/app/lib.rs.",
    )];
    let html = render(&items);
    assert!(html.contains("Edited main.rs and lib.rs."));
    assert!(!html.contains("Users"));
    assert!(!html.contains("Desktop"));
    assert!(!html.contains("home/you"));
}

#[test]
fn strip_paths_keeps_text_that_is_not_an_absolute_path() {
    assert_eq!(strip_paths("/api"), "/api");
    assert_eq!(strip_paths("1/2 and/or"), "1/2 and/or");
    assert_eq!(strip_paths("https://example.test/a/b"), "https://example.test/a/b");
    assert_eq!(strip_paths(r"see C:\a\b.rs, ok"), "see b.rs, ok");
    assert_eq!(strip_paths(r#""C:/x/y""#), r#""y""#);
    assert_eq!(strip_paths("C:\\"), "C:\\");
}

#[test]
fn special_characters_are_escaped() {
    assert_eq!(escape(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
}

#[test]
fn folder_is_shown_by_name_only() {
    assert_eq!(folder_name(r"C:\Users\you\work\app\"), "app");
    assert_eq!(folder_name("/home/you/proj"), "proj");
    assert_eq!(folder_name(""), "");
}

#[test]
fn header_names_the_agent_folder_and_date_and_the_footer_names_the_app() {
    let html = render(&[user(1, "hi")]);
    assert!(html.contains("<h1>Test Agent</h1>"));
    assert!(html.contains("<span>puppet-master</span><span>2026-10-28</span>"));
    assert!(html.contains("<title>Test Agent · puppet-master</title>"));
    assert!(html.contains("<footer class=\"foot\">Saved from Puppet Master</footer>"));
}

#[test]
fn a_tool_call_is_one_line_of_name_and_target() {
    let html = render(&[call(1, "Bash", "git status")]);
    assert!(html.contains("<div class=\"tool\"><span class=\"name\">Bash</span> <span>git status</span></div>"));
}

#[test]
fn an_edit_shows_its_change_as_plus_and_minus_lines() {
    let mut item = call(1, "Edit", "src/a.rs");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"file_path": "src/a.rs", "old_string": "let a = 1;\nlet b = 2;", "new_string": "let a = 3;"}));
    }
    let html = render(&[item]);
    assert!(html.contains("<span class=\"del\">- let a = 1;</span>"));
    assert!(html.contains("<span class=\"del\">- let b = 2;</span>"));
    assert!(html.contains("<span class=\"add\">+ let a = 3;</span>"));
}

#[test]
fn a_long_diff_side_is_cut_at_forty_lines() {
    let old: String = (1..=45).map(|n| format!("old-{n}\n")).collect();
    let mut item = call(1, "Edit", "a.rs");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"old_string": old, "new_string": "x"}));
    }
    let html = render(&[item]);
    assert!(html.contains("old-40</span>"));
    assert!(!html.contains("old-41"));
    assert!(html.contains("… 5 more lines"));
}

#[test]
fn tool_output_is_cut_at_forty_lines() {
    let output: String = (1..=50).map(|n| format!("row-{n}\n")).collect();
    let items = vec![ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output }];
    let html = render(&items);
    assert!(html.contains("row-40\n") || html.contains("row-40<"));
    assert!(!html.contains("row-41"));
    assert!(html.contains("… 10 more lines"));
}

#[test]
fn a_failed_output_is_marked() {
    let items = vec![ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: false, output: "boom".into() }];
    assert!(render(&items).contains("<pre class=\"output failed\">boom</pre>"));
}

#[test]
fn a_turn_end_says_how_it_ended_and_how_long_it_took() {
    let items = vec![ChatItem::TurnEnd { id: "e".into(), at: None, outcome: TurnOutcome::Interrupted, duration_ms: Some(65_000) }];
    assert!(render(&items).contains("Interrupted · Worked for 1m 5s"));
}

#[test]
fn a_task_list_shows_each_task_with_its_status_mark() {
    let items = vec![ChatItem::Plan {
        id: "p".into(),
        at: None,
        items: vec![
            PlanItem { text: "Write tests".into(), status: Some(PlanStatus::Completed), task_id: None },
            PlanItem { text: "Run them".into(), status: Some(PlanStatus::InProgress), task_id: None },
        ],
        replace: true,
    }];
    let html = render(&items);
    assert!(html.contains("[x]</span> Write tests"));
    assert!(html.contains("[~]</span> Run them"));
}

#[test]
fn a_notice_shows_its_text_but_not_its_link() {
    let items = vec![ChatItem::Notice {
        id: "n".into(),
        at: None,
        notice_kind: "pr".into(),
        text: "Pull request #1".into(),
        url: Some("https://example.test/1".into()),
    }];
    let html = render(&items);
    assert!(html.contains("Pull request #1"));
    assert!(!html.contains("example.test"));
}

#[test]
fn usage_and_session_lines_are_not_shown() {
    let items = vec![
        ChatItem::Usage { id: "u".into(), at: None, input_tokens: 1, output_tokens: 2, context_window: None },
        ChatItem::Session { id: "s".into(), at: None, model: None, cli_version: Some("9.8.7".into()) },
    ];
    let html = render(&items);
    assert!(!html.contains("9.8.7"));
    assert!(html.contains("Nothing to show."));
}

#[test]
fn turns_are_numbered_by_the_persons_messages() {
    let items = vec![agent(0, "before"), user(1, "one"), agent(1, "a"), user(2, "two"), agent(2, "b")];
    assert_eq!(turn_numbers(&items), vec![0, 1, 1, 2, 2]);
}

#[test]
fn a_turn_range_keeps_only_its_turns() {
    let items = vec![user(1, "first ask"), agent(1, "first answer"), user(2, "second ask"), agent(2, "second answer"), user(3, "third ask")];
    let html = page(&items, &meta(), TurnRange { from: Some(2), to: Some(2) }, &ctx());
    assert!(html.contains("second ask"));
    assert!(html.contains("second answer"));
    assert!(!html.contains("first ask"));
    assert!(!html.contains("third ask"));
    assert!(html.contains("<p class=\"range\">Turns 2 to 2</p>"));
}

#[test]
fn an_open_range_runs_to_the_end_of_the_chat() {
    let items = vec![user(1, "first ask"), user(2, "second ask"), user(3, "third ask")];
    let html = page(&items, &meta(), TurnRange { from: Some(2), to: None }, &ctx());
    assert!(!html.contains("first ask"));
    assert!(html.contains("second ask"));
    assert!(html.contains("third ask"));
    assert!(html.contains("Turns 2 to 3"));
}

#[test]
fn a_range_with_no_turns_says_nothing_to_show() {
    let items = vec![user(1, "only")];
    let html = page(&items, &meta(), TurnRange { from: Some(5), to: Some(6) }, &ctx());
    assert!(html.contains("Nothing to show."));
    assert!(!html.contains("only"));
}

#[test]
fn a_range_whose_first_turn_is_after_its_last_is_refused() {
    assert!(TurnRange { from: Some(3), to: Some(2) }.check().is_err());
    assert!(TurnRange { from: Some(2), to: Some(2) }.check().is_ok());
    assert!(TurnRange::default().check().is_ok());
}

#[test]
fn the_page_follows_the_system_colour_scheme() {
    let html = render(&[user(1, "hi")]);
    assert!(html.contains("color-scheme:light dark"));
    assert!(html.contains("@media (prefers-color-scheme:dark)"));
}
