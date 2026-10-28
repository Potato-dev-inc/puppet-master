use super::*;

use crate::redact::RedactContext;
use crate::session_chat::detail::{PlanItem, PlanStatus, ToolDetail};
use crate::session_chat::export_text::{escape, plain, strip_paths};
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
    assert_eq!(strip_paths("/api", None), "/api");
    assert_eq!(strip_paths("1/2 and/or", None), "1/2 and/or");
    assert_eq!(strip_paths("https://example.test/a/b", None), "https://example.test/a/b");
    assert_eq!(strip_paths(r"see C:\a\b.rs, ok", None), "see b.rs, ok");
    assert_eq!(strip_paths(r#""C:/x/y""#, None), r#""y""#);
    assert_eq!(strip_paths("C:\\", None), "C:\\");
}

#[test]
fn special_characters_are_escaped() {
    assert_eq!(escape(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
}

#[test]
fn folder_is_shown_by_name_only() {
    assert_eq!(folder_of(r"C:\Users\you\work\app\", None), "app");
    assert_eq!(folder_of("/home/you/proj", None), "proj");
    assert_eq!(folder_of("", None), "");
}

/// A context whose OS home folder is `home`.
fn ctx_home(home: &str) -> RedactContext {
    RedactContext { env_values: Vec::new(), home: Some(home.into()) }
}

#[test]
fn a_home_folder_with_a_space_never_shows_the_user_name() {
    assert_eq!(strip_paths(r"Edited C:\Users\Default User\proj\a.rs", None), "Edited a.rs");
    assert_eq!(strip_paths(r"Edited C:\Users\Default User\proj\a.rs", Some(r"C:\Users\Default User")), "Edited a.rs");
    assert_eq!(strip_paths(r#""C:\Users\Default User\proj\a.rs""#, None), r#""a.rs""#);
}

#[test]
fn a_bare_home_folder_becomes_a_tilde() {
    assert_eq!(strip_paths(r"C:\Users\test", None), "~");
    assert_eq!(strip_paths("cwd C:\\Users\\test\nnext", None), "cwd ~\nnext");
    assert_eq!(strip_paths(r"C:\Users\Default User", None), "~");
    assert_eq!(folder_of(r"C:\Users\test", None), "~");
    assert_eq!(folder_of(r"C:\Users\test\", None), "~");
    assert_eq!(folder_of(r"C:\Users\test\work\app", None), "app");
}

#[test]
fn a_forward_slash_home_folder_is_replaced_too() {
    assert_eq!(strip_paths("C:/Users/test/proj/a.rs", None), "a.rs");
    assert_eq!(strip_paths("/home/test/proj/a.rs", None), "a.rs");
    assert_eq!(strip_paths("/Users/test/proj/b.rs", None), "b.rs");
    assert_eq!(strip_paths("C:/Users/Default User/proj/c.rs", None), "c.rs");
    assert_eq!(strip_paths(r"C:/Users/test\proj\d.rs", None), "d.rs");
}

#[test]
fn a_mixed_case_home_folder_is_replaced() {
    assert_eq!(strip_paths(r"c:\users\x\proj\a.rs", None), "a.rs");
    assert_eq!(strip_paths(r"C:\USERS\X\proj\a.rs", None), "a.rs");
    assert_eq!(strip_paths(r"c:\users\x", None), "~");
    assert_eq!(strip_paths("/HOME/Default/proj/a.rs", None), "a.rs");
    // The OS home is matched in any case and with either slash.
    assert_eq!(strip_paths("c:/users/x/proj/a.rs", Some(r"C:\Users\X")), "a.rs");
}

#[test]
fn a_path_inside_quotes_loses_the_user_name() {
    assert_eq!(strip_paths(r#""C:\Users\test\proj\a.rs""#, None), r#""a.rs""#);
    assert_eq!(strip_paths(r"'C:\Users\test'", None), "'~'");
    let html = page(&[agent(1, r#"Opened "C:\Users\Default User\proj\a.rs" today"#)], &meta(), TurnRange::default(), &ctx_home(r"C:\Users\Default User"));
    assert!(html.contains("Opened &quot;a.rs&quot; today"));
    assert!(!html.contains("Default"));
    assert!(!html.contains("User"));
}

#[test]
fn the_user_name_is_gone_from_the_whole_page() {
    let items = vec![
        user(1, r"open C:\Users\Default User\proj\a.rs"),
        call(1, "Read", r"C:\Users\Default User"),
        ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output: r"C:/Users/test/x".into() },
    ];
    let html = page(&items, &meta(), TurnRange::default(), &ctx_home(r"C:\Users\Default User"));
    assert!(!html.contains("Default"));
    assert!(!html.contains("test"));
    assert!(!html.contains("User"));
    assert!(html.contains("open a.rs"));
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

/// A private key block made of fake base64 (nothing real).
fn fake_pem() -> String {
    let body: String = (0..12).map(|n| format!("QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo{n:02}abcdefgh\n")).collect();
    format!("-----BEGIN RSA PRIVATE KEY-----\n{body}-----END RSA PRIVATE KEY-----\n")
}

#[test]
fn a_private_key_in_tool_output_is_masked_on_every_line() {
    let output = format!("$ cat id_rsa\n{}done\n", fake_pem());
    let items = vec![ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output }];
    let html = render(&items);
    assert!(!html.contains("QUJDREVG"), "no line of the key body may reach the page");
    assert!(!html.contains("END RSA PRIVATE KEY"));
    assert!(html.contains("[REDACTED]"));
    assert!(html.contains("$ cat id_rsa") && html.contains("done"), "the text around the key stays");
}

#[test]
fn a_private_key_in_an_edit_is_masked_on_both_sides() {
    let mut item = call(1, "Edit", "key.pem");
    if let ChatItem::ToolCall { detail, .. } = &mut item {
        detail.input = Some(json!({"old_string": fake_pem(), "new_string": format!("x\n{}", fake_pem())}));
    }
    let html = render(&[item]);
    assert!(!html.contains("QUJDREVG"));
    assert!(!html.contains("PRIVATE KEY-----"));
}

#[test]
fn an_unknown_shape_secret_is_masked_in_the_page() {
    // 48 opaque characters with no known prefix: only strict mode finds it.
    let opaque = "Zk3Qp9Lm2Xv7Rt4Wc8Yb1Nd6Hf5Gj0Ks3Ua9Ie2Oo7Ty4Rq";
    let html = render(&[agent(1, &format!("the value is {opaque}")), ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output: format!("v={opaque}") }]);
    assert!(!html.contains(opaque));
}

#[test]
fn the_account_name_in_a_path_is_masked_with_no_home_known() {
    let html = render(&[agent(1, r"see C:\Users\someone\notes.txt and /home/someone/x.rs")]);
    assert!(!html.contains("someone"));
}

#[test]
fn an_email_address_in_a_message_tool_output_and_edit_never_reaches_the_page() {
    let mut edit = call(1, "Edit", "contact.txt");
    if let ChatItem::ToolCall { detail, .. } = &mut edit {
        detail.input = Some(json!({"old_string": "to: a.b@corp.io", "new_string": "to: c@d.co.uk"}));
    }
    let items = vec![
        user(1, "mail jane.doe@example.org please"),
        ChatItem::ToolResult { id: "r".into(), at: None, call_id: "c".into(), ok: true, output: "From: jane.doe@example.org".into() },
        edit,
    ];
    let html = render(&items);
    for address in ["jane.doe", "example.org", "a.b@corp.io", "corp.io", "d.co.uk"] {
        assert!(!html.contains(address), "{address} must not reach the page");
    }
    assert!(html.contains("email&gt;"), "each address is replaced by the mask");
}

#[test]
fn text_with_an_at_sign_that_is_not_an_address_stays() {
    let html = render(&[agent(1, "install @scope/pkg and foo@1.2.3 now")]);
    assert!(html.contains("@scope/pkg"));
    assert!(html.contains("foo@1.2.3"));
}

#[test]
fn plain_text_drops_the_key_and_the_user_folder() {
    let text = format!(r"key {FAKE_TOKEN} in C:\Users\you\proj\a.rs");
    let out = plain(&text, &ctx());
    assert!(!out.contains("sk-ant-api03"), "{out}");
    assert!(!out.contains("Users"), "{out}");
    assert!(!out.contains("you"), "{out}");
    assert!(out.contains("a.rs"), "{out}");
}

#[test]
fn plain_text_is_not_html_escaped() {
    let out = plain(r#"a <b> & "c" C:\Users\you\x\y.rs"#, &ctx());
    assert!(out.contains("<b>") && out.contains('&') && out.contains("\"c\""), "{out}");
}
