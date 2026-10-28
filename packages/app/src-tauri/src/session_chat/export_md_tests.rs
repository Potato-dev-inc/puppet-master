use super::*;

use crate::session_chat::detail::{PlanItem, PlanStatus, ToolDetail};
use crate::session_chat::model::TurnOutcome;
use serde_json::json;

/// A token-shaped string; nothing real.
const FAKE_TOKEN: &str = "sk-ant-api03-xxxxxxxxxxxxxxxxxxxx";

fn ctx() -> RedactContext {
    RedactContext { env_values: Vec::new(), home: None }
}

/// A context whose OS home folder is `home`.
fn ctx_home(home: &str) -> RedactContext {
    RedactContext { env_values: Vec::new(), home: Some(home.into()) }
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

fn output(text: &str) -> ChatItem {
    ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output: text.into() }
}

fn render(items: &[ChatItem]) -> String {
    markdown(items, &meta(), TurnRange::default(), &ctx())
}

#[test]
fn a_chat_is_one_markdown_file_with_a_heading_and_a_footer() {
    let md = render(&[user(1, "hello"), agent(1, "hi there")]);
    assert!(md.starts_with("# Test Agent\n\npuppet-master · 2026-10-28\n\n---\n\n**You**\n\nhello\n\n**Test Agent**\n\nhi there"));
    assert!(md.ends_with("---\n\n_Saved from Puppet Master_\n"));
}

#[test]
fn a_tool_call_is_one_line_of_name_and_target() {
    let md = render(&[call(1, "Bash", "git status")]);
    assert!(md.contains("\n\n- **Bash** git status\n\n"), "{md}");
}

#[test]
fn a_tool_call_summary_with_a_newline_stays_on_one_line() {
    let md = render(&[call(1, "Bash", "echo a\necho b")]);
    assert!(md.contains("- **Bash** echo a echo b"), "{md}");
}

#[test]
fn an_edit_shows_its_change_as_a_diff_block() {
    let mut item = call(1, "Edit", "src/a.rs");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"file_path": "src/a.rs", "old_string": "let a = 1;\nlet b = 2;", "new_string": "let a = 3;"}));
    }
    let md = render(&[item]);
    assert!(md.contains("```diff\n- let a = 1;\n- let b = 2;\n+ let a = 3;\n```"), "{md}");
}

#[test]
fn a_long_diff_side_is_cut_at_forty_lines() {
    let old: String = (1..=45).map(|n| format!("old-{n}\n")).collect();
    let mut item = call(1, "Edit", "a.rs");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"old_string": old, "new_string": "x"}));
    }
    let md = render(&[item]);
    assert!(md.contains("- old-40\n"));
    assert!(!md.contains("old-41"));
    assert!(md.contains("_… 5 more removed lines_"));
}

#[test]
fn tool_output_is_cut_at_forty_lines() {
    let text: String = (1..=50).map(|n| format!("row-{n}\n")).collect();
    let md = render(&[output(&text)]);
    assert!(md.contains("row-40\n```"), "{md}");
    assert!(!md.contains("row-41"));
    assert!(md.contains("_… 10 more lines_"));
}

#[test]
fn a_failed_output_is_marked() {
    let md = render(&[ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: false, output: "boom".into() }]);
    assert!(md.contains("_Failed._\n\n```\nboom\n```"), "{md}");
}

#[test]
fn tool_output_with_backticks_gets_a_longer_fence() {
    let md = render(&[output("a\n```\nb")]);
    assert!(md.contains("````\na\n```\nb\n````"), "{md}");
}

#[test]
fn the_private_key_is_masked_on_every_line_of_tool_output() {
    let body: String = (0..12).map(|n| format!("QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo{n:02}abcdefgh\n")).collect();
    let pem = format!("-----BEGIN RSA PRIVATE KEY-----\n{body}-----END RSA PRIVATE KEY-----\n");
    let md = render(&[output(&format!("$ cat id_rsa\n{pem}done\n"))]);
    assert!(!md.contains("QUJDREVG"), "no line of the key body may reach the file");
    assert!(!md.contains("PRIVATE KEY-----"));
    assert!(md.contains("[REDACTED]"));
    assert!(md.contains("$ cat id_rsa") && md.contains("done"), "the text around the key stays");
}

#[test]
fn a_private_key_in_an_edit_is_masked_on_both_sides() {
    let body: String = (0..12).map(|n| format!("QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo{n:02}abcdefgh\n")).collect();
    let pem = format!("-----BEGIN RSA PRIVATE KEY-----\n{body}-----END RSA PRIVATE KEY-----\n");
    let mut item = call(1, "Edit", "key.pem");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"old_string": pem, "new_string": format!("x\n{pem}")}));
    }
    let md = render(&[item]);
    assert!(!md.contains("QUJDREVG"));
    assert!(!md.contains("PRIVATE KEY-----"));
}

#[test]
fn a_fake_token_is_masked_in_messages_and_tool_lines() {
    let md = render(&[user(1, &format!("my key is {FAKE_TOKEN} please")), call(1, "Bash", &format!("curl -H {FAKE_TOKEN}"))]);
    assert!(!md.contains("sk-ant-api03"), "the token must not reach the file");
    assert!(md.contains("[REDACTED]"));
}

#[test]
fn a_home_folder_becomes_a_tilde_in_messages_tools_and_output() {
    let home = r"C:\Users\Default User";
    let items = vec![
        user(1, r"open C:\Users\Default User\proj\a.rs"),
        call(1, "Read", r"C:\Users\Default User"),
        output(r"C:/Users/Default User/x"),
    ];
    let md = markdown(&items, &meta(), TurnRange::default(), &ctx_home(home));
    assert!(md.contains("open a.rs"), "{md}");
    assert!(md.contains("- **Read** ~"), "{md}");
    assert!(!md.contains("Default"));
    assert!(!md.contains("User"));
}

#[test]
fn a_home_folder_with_no_home_known_is_still_cut_by_the_path_rule() {
    let md = render(&[agent(1, r"see C:\Users\someone\notes.txt and /home/someone/x.rs")]);
    assert!(!md.contains("someone"));
    assert!(md.contains("notes.txt") && md.contains("x.rs"));
}

#[test]
fn chat_text_that_looks_like_markup_is_neutralised() {
    let md = render(&[user(1, "<img src=x onerror=alert(1)>"), agent(1, "<script>alert(1)</script>")]);
    assert!(!md.contains('<'), "no raw < may reach the file outside code: {md}");
    assert!(md.contains("&lt;script>alert(1)&lt;/script>"));
    assert!(md.contains("&lt;img src=x onerror=alert(1)>"));
}

#[test]
fn every_markup_source_is_neutralised_not_only_messages() {
    let items = vec![
        user(1, "<b>hi</b>"),
        call(1, "<x>", "<y>"),
        ChatItem::Notice { id: "n".into(), at: None, notice_kind: "pr".into(), text: "<i>n</i>".into(), url: None },
        ChatItem::Plan { id: "p".into(), at: None, items: vec![PlanItem { text: "<u>t</u>".into(), status: None, task_id: None }], replace: true },
    ];
    let md = render(&items);
    assert!(!md.contains('<'), "{md}");
    assert!(md.contains("- **&lt;x>** &lt;y>"));
    // Tool output is code: its markup shows as text inside the block, which no renderer reads as a tag.
    let shown = render(&[output("<z>")]);
    assert!(shown.contains("```\n<z>\n```"), "{shown}");
}

#[test]
fn a_backslash_before_a_tag_does_not_let_the_tag_through() {
    // Two backslashes are one escaped backslash, so the `<` after them must still be neutralised.
    let md = render(&[agent(1, r"\\<script>alert(1)</script>")]);
    assert!(!md.contains("<script"), "{md}");
    assert!(md.contains(r"\\&lt;script>"));
}

#[test]
fn an_image_opener_is_neutralised_so_no_image_loads() {
    let md = render(&[agent(1, "![pixel](https://example.test/p.png)")]);
    assert!(!md.contains("!["), "{md}");
    assert!(md.contains("!&#91;pixel]"));
}

#[test]
fn code_in_a_fence_keeps_its_markup_as_text() {
    let md = render(&[agent(1, "Here:\n```html\n<div>x</div>\n```")]);
    assert!(md.contains("```html\n<div>x</div>\n```"), "{md}");
}

#[test]
fn code_in_a_backtick_span_keeps_its_markup_as_text() {
    let md = render(&[agent(1, "Use `<b>` here and <i>not</i>")]);
    assert!(md.contains("Use `<b>` here and &lt;i>not&lt;/i>"), "{md}");
}

#[test]
fn an_indented_fence_is_not_code_so_its_markup_is_neutralised() {
    // Four spaces make an indented code block in some renderers and plain text in others: never a fence here.
    let text = "    ```\n<b>x</b>";
    assert_eq!(neutralise(text), "    ```\n&lt;b>x&lt;/b>");
}

#[test]
fn a_fence_left_open_is_closed_at_the_end_of_its_text() {
    assert_eq!(neutralise("```\ncode"), "```\ncode\n```");
    assert_eq!(neutralise("````\ncode\n```"), "````\ncode\n```\n````");
}

#[test]
fn a_fence_closer_must_match_the_opener() {
    // A ``` opener is not closed by a ~~~ line, so the markup after it is still code.
    assert_eq!(neutralise("```\n~~~\n<b>"), "```\n~~~\n<b>\n```");
}

#[test]
fn a_turn_range_keeps_only_its_turns_under_its_heading() {
    let items = vec![user(1, "first ask"), agent(1, "first answer"), user(2, "second ask"), agent(2, "second answer"), user(3, "third ask")];
    let md = markdown(&items, &meta(), TurnRange { from: Some(2), to: Some(2) }, &ctx());
    assert!(md.contains("second ask") && md.contains("second answer"));
    assert!(!md.contains("first ask"));
    assert!(!md.contains("third ask"));
    assert!(md.contains("Turns 2 to 2"));
}

#[test]
fn a_range_with_no_turns_says_nothing_to_show() {
    let items = vec![user(1, "only")];
    let md = markdown(&items, &meta(), TurnRange { from: Some(5), to: Some(6) }, &ctx());
    assert!(md.contains("_Nothing to show._"));
    assert!(!md.contains("only"));
}

#[test]
fn a_task_list_shows_each_task_with_its_state() {
    let items = vec![ChatItem::Plan {
        id: "p".into(),
        at: None,
        items: vec![
            PlanItem { text: "Write tests".into(), status: Some(PlanStatus::Completed), task_id: None },
            PlanItem { text: "Run them".into(), status: Some(PlanStatus::InProgress), task_id: None },
        ],
        replace: true,
    }];
    let md = render(&items);
    assert!(md.contains("- [x] Write tests\n- [ ] Run them _(in progress)_"), "{md}");
}

#[test]
fn a_turn_end_says_how_it_ended_and_how_long_it_took() {
    let items = vec![ChatItem::TurnEnd { id: "e".into(), at: None, outcome: TurnOutcome::Interrupted, duration_ms: Some(65_000) }];
    assert!(render(&items).contains("_Interrupted · Worked for 1m 5s_"));
}

#[test]
fn usage_and_session_lines_are_not_shown() {
    let items = vec![
        ChatItem::Usage { id: "u".into(), at: None, input_tokens: 1, output_tokens: 2, context_window: None },
        ChatItem::Session { id: "s".into(), at: None, model: None, cli_version: Some("9.8.7".into()) },
    ];
    let md = render(&items);
    assert!(!md.contains("9.8.7"));
    assert!(md.contains("_Nothing to show._"));
}

#[test]
fn the_fence_helper_is_longer_than_any_run_inside() {
    assert_eq!(fenced("", "x"), "```\nx\n```");
    assert_eq!(fenced("diff", "a ``` b"), "````diff\na ``` b\n````");
}

#[test]
fn an_email_address_in_a_message_tool_output_and_edit_never_reaches_the_file() {
    let mut edit = call(1, "Edit", "contact.txt");
    if let ChatItem::ToolCall { detail, .. } = &mut edit {
        detail.input = Some(json!({"old_string": "to: a.b@corp.io", "new_string": "to: c@d.co.uk"}));
    }
    let items = vec![user(1, "mail jane.doe@example.org please"), output("From: jane.doe@example.org"), edit];
    let md = render(&items);
    for address in ["jane.doe", "example.org", "a.b@corp.io", "corp.io", "d.co.uk"] {
        assert!(!md.contains(address), "{address} must not reach the file");
    }
    assert!(md.contains("email>"), "each address is replaced by the mask");
}

#[test]
fn text_with_an_at_sign_that_is_not_an_address_stays() {
    let md = render(&[agent(1, "install @scope/pkg and foo@1.2.3 now")]);
    assert!(md.contains("@scope/pkg"));
    assert!(md.contains("foo@1.2.3"));
}
