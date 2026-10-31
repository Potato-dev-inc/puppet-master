//! The screen reader on made-up screens of made-up agents: nothing here names a real CLI. The probe profiles are
//! declared the way a wizard-made definition is (chrome, thinking, tool rows, the transcript echo of a message).

use std::sync::Arc;

use super::*;
use crate::session_chat::model::ChatItem;

pub(in crate::session_chat) const ECHO: &str = "chat-screen-probe";
/// The same agent without a tool table: its tool rows are ordinary text.
const NO_TOOLS: &str = "chat-screen-probe-no-tools";
/// An agent whose transcript does not echo what was typed.
const NO_ECHO: &str = "chat-screen-probe-no-echo";

const BODY: &str = r##"
[binary]
names = ["probe"]

[tui]
argv = []

[tui.chrome]
line_regex = ['^probe cli', '^[+|\-\s]+$', '^\|\s*>\s*type a task', '^/help for commands', '^done in \d+s$', '^model:']

[tui.thinking]
line_regex = ['^~\s*thinking']

@@TOOLS@@

[tui.footer_model]
regex = 'model:\s*(?P<model>[\w.-]+)'

@@DELIVERY@@
"##;

fn definition(id: &str, tools: bool, echo: bool) -> String {
    let tools = if tools { "[tui.tool_activity]\nline_regex = ['^tool:\\s']" } else { "" };
    let delivery = if echo { "[tui.delivery]\ninput_echo_regex = '^\\|\\s*>\\s*\\S'\ntranscript_echo_regex = '^>\\s+\\S'" } else { "" };
    let head = format!("schema_version = 1\n\n[agent]\nid = \"{id}\"\nlabel = \"Probe\"\nexperimental = true\n");
    head + &BODY.replace("@@TOOLS@@", tools).replace("@@DELIVERY@@", delivery)
}

/// Registered once for the whole test run (the tests run side by side and share the registry).
pub(in crate::session_chat) fn register_probes() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for (id, tools, echo) in [(ECHO, true, true), (NO_TOOLS, false, true), (NO_ECHO, true, false)] {
            let parsed = crate::tui::declarative::parse_definition(&definition(id, tools, echo)).expect("parses");
            let profile = Arc::new(crate::tui::declarative::DeclarativeProfile::new(parsed).expect("builds"));
            crate::tui::registry::registry().add_transient(profile).expect("registered");
        }
    });
}

fn chat() -> ScreenChat {
    register_probes();
    ScreenChat::new(ECHO)
}

const BANNER: &str = "Probe CLI 1.4  model: small-1\n";
const INPUT: &str = "+--------------------+\n| > Type a task...   |\n+--------------------+\n/help for commands\n";

/// A screen: the conversation rows between the banner and the input box.
fn screen(conversation: &str) -> String {
    format!("{BANNER}{conversation}\n{INPUT}")
}

const FIRST_ASKED: &str = "> list the files here\n~ thinking 1s";
const FIRST_TOOL: &str = "> list the files here\ntool: ls\n  a.txt\n  b.txt\n~ thinking 3s";
const FIRST_DONE: &str = "> list the files here\ntool: ls\n  a.txt\n  b.txt\ntwo files, a.txt and b.txt.\ndone in 2s";
const SECOND_ASKED: &str = "\n> now read a.txt\n~ thinking 1s";
const SECOND_DONE: &str = "\n> now read a.txt\ntool: cat a.txt\n  hello\nit says hello.\ndone in 1s";

/// What a viewer holds: items put in place by id, in order, the way the UI merges them.
#[derive(Default)]
struct Viewer {
    items: Vec<ChatItem>,
    resets: usize,
    batches: Vec<Vec<String>>,
}

impl Viewer {
    fn take(&mut self, change: Change) {
        match change {
            Change::Nothing => {}
            Change::Append(items) | Change::Upsert(items) => {
                self.batches.push(items.iter().map(|item| item.id().to_string()).collect());
                for item in items {
                    match self.items.iter().position(|known| known.id() == item.id()) {
                        Some(at) => self.items[at] = item,
                        None => self.items.push(item),
                    }
                }
            }
            Change::Reset(items) => {
                self.resets += 1;
                self.batches.push(items.iter().map(|item| item.id().to_string()).collect());
                self.items = items;
            }
        }
    }

    fn look(&mut self, chat: &mut ScreenChat, text: &str, status: &str) {
        self.take(chat.observe(Some(text), status));
    }

    /// Each item as `kind:text`.
    fn lines(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|item| match item {
                ChatItem::User { text, .. } => format!("user:{text}"),
                ChatItem::Assistant { text, final_answer, .. } => format!("assistant{}:{text}", if *final_answer { "!" } else { "" }),
                ChatItem::ToolCall { summary, .. } => format!("tool:{summary}"),
                ChatItem::TurnEnd { .. } => "end".to_string(),
                ChatItem::Session { model, .. } => format!("session:{}", model.clone().unwrap_or_default()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn ids(&self) -> Vec<String> {
        self.items.iter().map(|item| item.id().to_string()).collect()
    }
}

/// Quiet polls on the same text, as the watcher makes them while nothing changes.
fn settle(viewer: &mut Viewer, chat: &mut ScreenChat) {
    for _ in 0..3 {
        viewer.take(chat.observe(None, "idle"));
    }
}

#[test]
fn two_turns_become_user_tool_reply_and_end_items() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    settle(&mut viewer, &mut chat);
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}{SECOND_DONE}")), "idle");
    settle(&mut viewer, &mut chat);
    assert_eq!(
        viewer.lines(),
        [
            "session:small-1",
            "user:list the files here",
            "tool:tool: ls",
            "assistant!:two files, a.txt and b.txt.",
            "end",
            "user:now read a.txt",
            "tool:tool: cat a.txt",
            "assistant!:it says hello.",
            "end",
        ]
    );
    assert_eq!(
        viewer.ids(),
        [
            "screen:session",
            "screen:1:user",
            "screen:1:tool:0",
            "screen:1:reply",
            "screen:1:end",
            "screen:2:user",
            "screen:2:tool:0",
            "screen:2:reply",
            "screen:2:end",
        ]
    );
}

#[test]
fn thinking_chrome_and_tool_output_are_not_chat_text() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_TOOL), "running");
    let text = viewer.lines().join("\n");
    for dropped in ["thinking", "Type a task", "/help", "a.txt", "+---", "done in"] {
        assert!(!text.contains(dropped), "{dropped:?} leaked into the chat:\n{text}");
    }
    assert_eq!(viewer.lines(), ["session:small-1", "user:list the files here", "tool:tool: ls"]);
}

#[test]
fn a_reply_that_grows_is_sent_again_under_the_same_id_and_replaces_its_old_copy() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen("> say something\nThe"), "running");
    viewer.look(&mut chat, &screen("> say something\nThe quick brown"), "running");
    viewer.look(&mut chat, &screen("> say something\nThe quick brown fox\njumps over it."), "running");
    assert_eq!(viewer.lines(), ["session:small-1", "user:say something", "assistant:The quick brown fox\njumps over it."]);
    assert_eq!(viewer.resets, 0, "growing text is a replace, not a reset");
    // The second and third batches carry only the reply.
    assert_eq!(viewer.batches[1], ["screen:1:reply"]);
    assert_eq!(viewer.batches[2], ["screen:1:reply"]);
}

#[test]
fn a_turn_ends_only_after_the_pane_was_idle_on_the_same_text_twice() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "running");
    assert!(!viewer.lines().contains(&"end".to_string()));
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    assert!(!viewer.lines().contains(&"end".to_string()), "one idle look is not enough");
    viewer.take(chat.observe(None, "idle"));
    assert_eq!(viewer.lines().last().map(String::as_str), Some("end"));
    assert!(viewer.lines().iter().any(|line| line.starts_with("assistant!:")), "final once the turn ended");
    // The same text under a pane that looks busy for a moment (a redraw) keeps the turn ended.
    viewer.take(chat.observe(None, "running"));
    assert_eq!(viewer.lines().last().map(String::as_str), Some("end"));
    assert_eq!(viewer.resets, 0);
    // Work goes on in the turn (another tool row): it is open again, and the viewer is told.
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}\ntool: pwd")), "running");
    assert!(!viewer.lines().contains(&"end".to_string()), "{:?}", viewer.lines());
    assert_eq!(viewer.resets, 1, "an item went away, so everything is sent again");
}

#[test]
fn a_screen_that_is_cleared_resets_the_chat_and_a_new_conversation_starts_at_turn_one() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    settle(&mut viewer, &mut chat);
    assert!(viewer.lines().len() > 3);
    viewer.look(&mut chat, &screen(""), "idle");
    settle(&mut viewer, &mut chat);
    assert_eq!(viewer.lines(), ["session:small-1"], "the model stays, the conversation is gone");
    assert_eq!(viewer.resets, 1);
    viewer.look(&mut chat, &screen("> a fresh start\nhello again"), "idle");
    assert_eq!(viewer.ids()[1], "screen:1:user");
    assert_eq!(viewer.lines()[1], "user:a fresh start");
}

#[test]
fn history_that_scrolled_off_stays_and_new_turns_are_numbered_on() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    settle(&mut viewer, &mut chat);
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}{SECOND_ASKED}")), "running");
    // The pane has no scrollback: the first turn is gone from the text once the second has pushed it up.
    viewer.look(&mut chat, &screen(SECOND_DONE.trim_start()), "idle");
    settle(&mut viewer, &mut chat);
    let lines = viewer.lines();
    assert!(lines.contains(&"user:list the files here".to_string()), "{lines:?}");
    assert!(lines.contains(&"user:now read a.txt".to_string()), "{lines:?}");
    assert_eq!(viewer.resets, 0);
    assert_eq!(viewer.ids().iter().filter(|id| id.ends_with(":user")).collect::<Vec<_>>(), ["screen:1:user", "screen:2:user"]);
}

#[test]
fn a_reply_whose_message_scrolled_off_the_top_keeps_the_longest_text_seen() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen("> write four lines\none\ntwo\nthree\nfour"), "running");
    // Only the last rows are on the text now; the message row is above the top.
    viewer.look(&mut chat, &screen("three\nfour\nfive"), "running");
    assert_eq!(viewer.resets, 0);
    assert_eq!(viewer.lines()[1], "user:write four lines");
    let reply = viewer.lines().into_iter().find(|line| line.starts_with("assistant")).expect("a reply");
    assert!(reply.contains("one") && reply.contains("four"), "the longer reading stays: {reply}");
}

#[test]
fn a_draft_in_the_input_box_is_not_a_message_until_the_agent_works() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen("> half a thought"), "idle");
    settle(&mut viewer, &mut chat);
    assert_eq!(viewer.lines(), ["session:small-1"]);
    viewer.look(&mut chat, &screen("> half a thought\n~ thinking 1s"), "running");
    assert_eq!(viewer.lines(), ["session:small-1", "user:half a thought"]);
}

#[test]
fn a_message_wrapped_over_two_rows_is_one_user_item() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen("> please list every file in the\n> folder and say how many\nfive."), "idle");
    assert_eq!(viewer.lines()[1], "user:please list every file in the folder and say how many");
    assert_eq!(viewer.lines()[2], "assistant:five.");
}

#[test]
fn a_profile_with_no_tool_table_shows_tool_rows_as_text_and_makes_no_tool_items() {
    register_probes();
    let mut chat = ScreenChat::new(NO_TOOLS);
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    assert!(viewer.items.iter().all(|item| !matches!(item, ChatItem::ToolCall { .. })));
    let reply = viewer.lines().into_iter().find(|line| line.starts_with("assistant")).expect("a reply");
    assert!(reply.contains("two files, a.txt and b.txt."), "{reply}");
}

#[test]
fn a_profile_that_echoes_nothing_cuts_turns_by_the_panes_status() {
    register_probes();
    let mut chat = ScreenChat::new(NO_ECHO);
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(""), "idle");
    assert_eq!(viewer.lines(), ["session:small-1"]);
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}")), "running");
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}")), "idle");
    settle(&mut viewer, &mut chat);
    // The echo row of the person's message is just text here; the reply is what follows the tool row.
    assert_eq!(viewer.lines()[0], "session:small-1");
    assert_eq!(viewer.lines()[1], "tool:tool: ls");
    assert!(viewer.lines()[2].starts_with("assistant!:"), "{:?}", viewer.lines());
    assert_eq!(viewer.lines()[3], "end");
    // A second busy period is a second turn; the first one is not read again from the new text.
    let both = screen(&format!("{FIRST_DONE}\n> again\ntool: pwd\nhere."));
    viewer.look(&mut chat, &both, "running");
    viewer.look(&mut chat, &both, "idle");
    settle(&mut viewer, &mut chat);
    let replies: Vec<String> = viewer.lines().into_iter().filter(|line| line.starts_with("assistant")).collect();
    assert_eq!(replies.len(), 2, "{:?}", viewer.lines());
    assert!(replies[1].ends_with("here."), "{replies:?}");
    assert!(!replies[1].contains("a.txt"), "the first turn's text is not in the second {replies:?}");
    assert_eq!(viewer.ids().iter().filter(|id| id.ends_with(":end")).count(), 2);
}

#[test]
fn an_unchanged_screen_sends_nothing() {
    let mut chat = chat();
    assert!(matches!(chat.observe(Some(&screen(FIRST_DONE)), "idle"), Change::Upsert(_)));
    for _ in 0..4 {
        chat.observe(None, "idle");
    }
    assert_eq!(chat.observe(None, "idle"), Change::Nothing);
    assert_eq!(chat.observe(Some(&screen(FIRST_DONE)), "idle"), Change::Nothing, "the same text again");
}

#[test]
fn a_screen_with_nothing_but_chrome_has_no_turns() {
    register_probes();
    let mut chat = chat();
    assert_eq!(chat.observe(Some(""), "idle"), Change::Nothing);
    // Nothing on screen but chrome: no items, no turns.
    assert!(matches!(chat.observe(Some(&screen("")), "idle"), Change::Upsert(items) if items.len() == 1));
}

#[test]
fn a_message_typed_and_deleted_leaves_nothing_behind() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "running");
    settle(&mut viewer, &mut chat);
    viewer.look(&mut chat, &screen(&format!("{FIRST_DONE}\n> typing next")), "running");
    assert!(viewer.lines().contains(&"user:typing next".to_string()));
    viewer.look(&mut chat, &screen(FIRST_DONE), "running");
    assert!(!viewer.lines().contains(&"user:typing next".to_string()), "{:?}", viewer.lines());
}

#[test]
fn profile_helpers_in_the_probe_behave_as_described() {
    register_probes();
    let profile = crate::tui::profile_for(ECHO);
    assert!(profile.reads_user_messages());
    assert_eq!(profile.user_message_row("> hello there").as_deref(), Some("hello there"));
    assert_eq!(profile.user_message_row("hello"), None);
    assert!(!crate::tui::profile_for(NO_ECHO).reads_user_messages());
}

#[test]
fn a_message_drawn_wrong_for_one_frame_is_put_right_under_the_same_id() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    // The layout was still settling: the side column's text sat in the message row.
    viewer.look(&mut chat, &screen("> say hello to the room          New session 2026\nreply: hello"), "running");
    assert!(viewer.lines()[1].starts_with("user:say hello to the room") && viewer.lines()[1].ends_with("New session 2026"), "{:?}", viewer.lines());
    viewer.look(&mut chat, &screen("> say hello to the room\nreply: hello"), "running");
    assert_eq!(viewer.lines()[1], "user:say hello to the room");
    assert_eq!(viewer.ids().iter().filter(|id| id.ends_with(":user")).count(), 1);
    assert_eq!(viewer.resets, 0);
}

#[test]
fn a_redraw_caught_half_way_is_not_a_cleared_screen() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    settle(&mut viewer, &mut chat);
    let before = viewer.lines();
    // The screen was wiped and is being drawn again: one look sees nothing, the next sees it all.
    viewer.look(&mut chat, &screen(""), "running");
    viewer.look(&mut chat, &screen(FIRST_DONE), "idle");
    settle(&mut viewer, &mut chat);
    assert_eq!(viewer.lines(), before);
    assert_eq!(viewer.resets, 0, "nothing was taken away from the viewer");
}

#[test]
fn a_reply_longer_than_the_cap_keeps_its_newest_rows_and_says_the_earlier_ones_were_trimmed() {
    let mut chat = chat();
    let mut viewer = Viewer::default();
    let rows: String = (1..=3000).map(|n| format!("output row {n}\n")).collect();
    viewer.look(&mut chat, &screen(&format!("> print rows\n{rows}all rows printed\ndone in 9s")), "running");
    let reply = viewer.lines().into_iter().find(|line| line.starts_with("assistant")).expect("a reply");
    assert!(reply.ends_with("all rows printed"), "the end of the reply stays: {}", &reply[reply.len().saturating_sub(60)..]);
    assert!(reply.contains("output row 3000"), "the newest rows stay");
    assert!(!reply.contains("output row 1\n"), "the oldest rows are the ones dropped");
    assert!(reply.starts_with("assistant:[earlier output trimmed]\n"), "the cut is said out loud: {}", &reply[..60]);
    assert!(reply.chars().count() <= MAX_REPLY_CHARS + 80, "still bounded");
    // The cut falls between rows, never inside one.
    let first_row = reply.lines().nth(1).unwrap();
    assert!(first_row.starts_with("output row "), "no half row: {first_row}");
}
