//! The OpenCode transcript rules on real captures of OpenCode 1.19 (`fixtures/screen_*.txt`), through the chat view's
//! screen reader.

use super::*;
use crate::session_chat::screen::ScreenChat;
use crate::session_chat::ChatItem;

const WIDE: &str = include_str!("fixtures/screen_turn_wide.txt");
const NARROW: &str = include_str!("fixtures/screen_turn_narrow.txt");
const HOME: &str = include_str!("fixtures/screen_home.txt");

/// A capture as the pane's text: no header line, LF rows.
fn screen(capture: &str) -> String {
    capture.replace("\r\n", "\n").lines().filter(|row| !row.starts_with("# capture")).collect::<Vec<_>>().join("\n")
}

fn chat_of(text: &str) -> Vec<ChatItem> {
    let mut chat = ScreenChat::new("opencode");
    chat.observe(Some(text), "idle");
    chat.observe(None, "idle");
    chat.observe(None, "idle");
    chat.shown().to_vec()
}

fn lines(items: &[ChatItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ChatItem::User { text, .. } => format!("user:{text}"),
            ChatItem::ToolCall { summary, .. } => format!("tool:{summary}"),
            ChatItem::Assistant { text, final_answer, .. } => format!("assistant{}:{text}", if *final_answer { "!" } else { "" }),
            ChatItem::TurnEnd { .. } => "end".into(),
            ChatItem::Session { model, .. } => format!("session:{}", model.clone().unwrap_or_default()),
            other => panic!("{other:?}"),
        })
        .collect()
}

const TURN: [&str; 5] = [
    "user:Read notes.txt and reply with only its first word.",
    "tool:→ Read notes.txt",
    "assistant!:v2",
    "end",
    "",
];

#[test]
fn a_wide_pane_with_the_sidebar_reads_as_one_turn() {
    let items = chat_of(&screen(WIDE));
    assert_eq!(lines(&items), ["session:DeepSeek V4.1 Flash", TURN[0], TURN[1], TURN[2], TURN[3]]);
}

#[test]
fn a_narrow_pane_with_only_the_scrollbar_reads_the_same() {
    let items = chat_of(&screen(NARROW));
    assert_eq!(lines(&items), ["session:DeepSeek V4.1 Flash", TURN[0], TURN[1], TURN[2], TURN[3]]);
}

#[test]
fn the_transcript_leaves_out_the_sidebar_the_input_box_and_the_footer() {
    let text = transcript_text(&screen(WIDE));
    for gone in ["Context", "tokens", "MCP", "LSP", "ctrl+p", "OpenCode 1.19.0", "OpenCode Go", "╹▀", "First word of notes.txt"] {
        assert!(!text.contains(gone), "{gone:?} is not conversation:\n{text}");
    }
    for kept in ["┃  Read notes.txt and reply with only its first word.", "→ Read notes.txt", "v2", "▣  Build · DeepSeek V4.1 Flash · 12.7s"] {
        assert!(text.contains(kept), "{kept:?} is conversation:\n{text}");
    }
    assert!(!text.contains('█'), "the scrollbar is not text:\n{text}");
}

#[test]
fn a_fresh_pane_has_no_conversation() {
    assert_eq!(transcript_text(&screen(HOME)), "");
    assert!(chat_of(&screen(HOME)).is_empty());
}

#[test]
fn rows_are_classified_the_way_the_captures_show_them() {
    assert!(is_tool_activity_line("→ read notes.txt") && is_tool_activity_line("✱ glob \"*.txt\" (2 matches)") && is_tool_activity_line("$ npm test"));
    assert!(is_tool_activity_line("~ reading file...") && is_tool_activity_line("⠋ read notes.txt"));
    assert!(!is_tool_activity_line("v2") && !is_tool_activity_line("the file says hello"));
    assert!(is_summary_line("▣  build · deepseek v4.1 flash · 12.7s"));
    assert!(is_chrome_line("┃") && is_chrome_line("■■⬝⬝  esc interrupt") && is_chrome_line("tab agents  ctrl+p commands"));
    assert!(!is_chrome_line("v2") && !is_chrome_line("┃  hello"));
    assert_eq!(user_message_row("  ┃  Read notes.txt  ").as_deref(), Some("Read notes.txt"));
    assert_eq!(user_message_row("  ┃"), None);
    assert_eq!(user_message_row("v2"), None);
    assert!(is_thinking_line("thinking: weighing the options") && is_thinking_line("+ thought: 1.3s"));
}

#[test]
fn the_model_is_the_one_on_the_newest_summary_row() {
    assert_eq!(footer_model("▣  Build · Old Model · 3s\n…\n▣  Build · DeepSeek V4.1 Flash · 12.7s").as_deref(), Some("DeepSeek V4.1 Flash"));
    assert_eq!(footer_model("▣  Plan · Some Model").as_deref(), Some("Some Model"));
    assert_eq!(footer_model("no summary row here"), None);
}

#[test]
fn a_second_turn_under_the_first_is_a_second_turn_and_the_input_box_is_never_one() {
    // The transcript of the capture with a second exchange printed under it, drawn the way OpenCode draws one.
    let mut rows: Vec<String> = screen(NARROW).lines().map(str::to_string).collect();
    let at = rows.iter().position(|row| row.contains("▣  Build")).expect("the summary row");
    let bar = |text: &str| format!("  ┃  {text:<92}█");
    let second = [
        format!("{:<99}█", ""),
        bar(""),
        bar("Now read it again"),
        bar(""),
        format!("{:<99}█", "     → Read notes.txt"),
        format!("{:<99}█", ""),
        format!("{:<99}█", "     v2 again"),
        format!("{:<99}█", ""),
        format!("{:<99}█", "     ▣  Build · DeepSeek V4.1 Flash · 3.1s"),
    ];
    for (i, row) in second.into_iter().enumerate() {
        rows.insert(at + 1 + i, row);
    }
    let items = chat_of(&rows.join("\n"));
    assert_eq!(
        lines(&items),
        [
            "session:DeepSeek V4.1 Flash",
            TURN[0],
            TURN[1],
            "assistant!:v2",
            "end",
            "user:Now read it again",
            "tool:→ Read notes.txt",
            "assistant!:v2 again",
            "end",
        ]
    );
}
