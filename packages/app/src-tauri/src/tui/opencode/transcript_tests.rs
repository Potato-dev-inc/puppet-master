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

#[test]
fn the_model_is_not_followed_by_the_sidebar_on_the_same_row() {
    let row = format!("     ▣  Build · DeepSeek V4.1 Flash{:<60}█    $0.00 spent", "");
    assert_eq!(footer_model(&row).as_deref(), Some("DeepSeek V4.1 Flash"));
}

const WORKING: &str = include_str!("fixtures/screen_working_wide.txt");
const REPLY: &str = include_str!("fixtures/screen_reply_wide.txt");

#[test]
fn a_turn_in_flight_shows_the_message_and_the_tool_rows_and_no_end() {
    let mut chat = ScreenChat::new("opencode");
    chat.observe(Some(&screen(WORKING)), "running");
    assert_eq!(
        lines(chat.shown()),
        [
            "session:DeepSeek V4.1 Flash",
            "user:Read sum.js and test.js, then answer in exactly three short lines: what sum.js does, what test.js does, and the word DONE.",
            "tool:→ Read sum.js",
            "tool:→ Read test.js",
        ]
    );
}

#[test]
fn a_three_line_answer_is_one_reply_with_its_lines() {
    let items = chat_of(&screen(REPLY));
    let lines = lines(&items);
    assert_eq!(lines.len(), 6, "{lines:#?}");
    assert_eq!(
        lines[4],
        "assistant!:sum.js exports a sum function that adds all numbers in an array.\ntest.js asserts sum(1,2,3) is 6 and sum([]) is 0, then logs \"all tests passed\".\nDONE"
    );
    assert_eq!(lines[5], "end");
}

#[test]
fn the_chat_follows_a_turn_from_its_first_look_to_its_answer_without_a_reset() {
    use crate::session_chat::watch::Change;
    let mut chat = ScreenChat::new("opencode");
    let mut resets = 0;
    for (capture, status) in [(WORKING, "running"), (REPLY, "running"), (REPLY, "idle")] {
        if matches!(chat.observe(Some(&screen(capture)), status), Change::Reset(_)) {
            resets += 1;
        }
    }
    chat.observe(None, "idle");
    chat.observe(None, "idle");
    assert_eq!(resets, 0);
    assert_eq!(chat.shown().len(), 6);
}

const BASH: &str = include_str!("fixtures/screen_bash_wide.txt");

#[test]
fn the_box_a_shell_command_is_drawn_in_is_a_tool_row_and_not_a_second_message() {
    let items = chat_of(&screen(BASH));
    assert_eq!(
        lines(&items),
        [
            "session:DeepSeek V4.1 Flash",
            "user:Run the shell command: node -v   Then reply with only the version number.",
            "tool:$ node -v",
            "assistant!:v22.20.0",
            "end",
        ]
    );
}

#[test]
fn a_thought_in_its_bar_block_is_not_conversation_and_a_message_that_starts_like_one_still_is() {
    let thought = "  ┃  Thinking: weighing the two files\n  ┃  and how to answer\n\n     the answer\n";
    assert_eq!(transcript_text(thought).trim(), "the answer");
    let note = "  ┃  Thinking about lunch, what should I eat?\n\n     soup\n";
    // "Thinking about" is a message: only the label with its colon is a thought.
    assert!(transcript_text(note).contains("Thinking about lunch"), "{}", transcript_text(note));
}

#[test]
fn a_shell_box_with_a_title_row_keeps_only_the_command_as_a_tool_row() {
    let boxed = "  ┃  # Show the node version\n  ┃\n  ┃  $ node -v\n  ┃  v22.20.0\n\n     v22.20.0\n";
    assert_eq!(transcript_text(boxed).trim(), "$ node -v\n\n     v22.20.0");
}
