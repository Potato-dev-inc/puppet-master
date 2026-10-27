//! The Chat tab of an agent that writes no session file Puppet Master can read: the chat is read off the pane's
//! screen (`session_chat::screen`), on a real ConPTY with `pm-fake-tui` as a made-up agent ("chat", drawn with the
//! nimbus layout). Zero tokens. The person types in the terminal; the chat has to follow.
//!
//! Nothing is tuned to one CLI: the agent, its words and its regexes are made up.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{ChatProbe, Session, World};
use serde_json::Value;

/// A screen-read agent as a wizard writes it for the made-up layout, with `[tui.delivery]` naming the row where the
/// transcript echoes a sent message (`> <message>`).
const CHAT_AGENT: &str = r##"
schema_version = 1

[agent]
id = "chat"
label = "Chat"
experimental = true

[binary]
names = ["chat"]

[tui]
argv = []
idle_after_ms = 1500
quiet_idle_after_ms = 800

[tui.busy]
working_regex = ['ctrl\+c to stop']
idle_regex = ['\|\s*>\s*type a task']

[tui.chrome]
line_regex = [
  '^nimbus\s+[\d.]+',
  '^[+|\-\s]+$',
  '^\|\s*>\s*type a task',
  '^/help for commands',
  '^model:\s',
  '^done in \d+s$',
  '^\W?\s*generating\.\.\.',
]

[tui.thinking]
line_regex = ['^[~*]\s*(thinking|generating)', '^thought for \d+s']

[tui.tool_activity]
line_regex = ['^tool:\s']

[tui.delivery]
input_echo_regex = '^\|\s*>\s*\S'
transcript_echo_regex = '^>\s+\S'
"##;

const TWO_TURNS: &str = r#"
agent = "nimbus"
boot_ms = 100
[[turn]]
on_prompt = "first"
steps = [
  { do = "think", ms = 900 },
  { do = "tool", label = "ls", ms = 400 },
  { do = "think", ms = 600 },
  { do = "say", text = "Hello there, friend." },
]
[[turn]]
on_prompt = "second"
steps = [
  { do = "think", ms = 700 },
  { do = "say", text = "The quick brown fox." },
]
"#;

fn kinds(items: &[Value]) -> Vec<String> {
    items.iter().map(|item| item["kind"].as_str().unwrap_or_default().to_string()).collect()
}

fn text_of(items: &[Value], id: &str) -> String {
    let item = items.iter().find(|item| item["id"] == id).unwrap_or_else(|| panic!("no item {id} in {items:#?}"));
    item["text"].as_str().or(item["summary"].as_str()).unwrap_or_default().to_string()
}

/// Types `text` as the person does and waits until the chat shows the end of turn `n`.
fn ask(session: &Session, pane: &str, chat: &ChatProbe, text: &str, n: usize) {
    session.type_as_user(pane, text, true).unwrap_or_else(|e| panic!("typing failed: {e}"));
    let end = format!("screen:{n}:end");
    let found = session.until(60, || chat.ids().contains(&end).then_some(()));
    assert!(
        found.is_some(),
        "the chat never showed the end of turn {n}\n-- items: {:#?}\n-- screen:\n{}",
        chat.items(),
        session.history(pane)
    );
}

#[test]
fn a_chat_read_off_the_screen_follows_two_turns_typed_in_the_terminal() {
    let session = World::get().session("chat-from-screen");
    session.screen_agent("fake_chat", CHAT_AGENT);
    session.script(TWO_TURNS);
    let pane = session.spawn_pane("fake_chat").expect("a pane");
    session.until(30, || session.screen(&pane).contains("Type a task").then_some(())).expect("the agent drew its input box");

    // Open the Chat tab before anything was said: nothing to show, nothing invented from the banner or the input box.
    let chat = session.open_screen_chat(&pane, "fake_chat");
    assert!(chat.items().is_empty(), "{:#?}", chat.items());

    ask(&session, &pane, &chat, "Say the first thing please", 1);
    let first = chat.items();
    assert_eq!(kinds(&first), ["user", "toolCall", "assistant", "turnEnd"], "{first:#?}\n-- screen:\n{}", session.history(&pane));
    assert_eq!(chat.ids(), ["screen:1:user", "screen:1:tool:0", "screen:1:reply", "screen:1:end"]);
    assert_eq!(text_of(&first, "screen:1:user"), "Say the first thing please");
    assert_eq!(text_of(&first, "screen:1:tool:0"), "tool: ls");
    assert_eq!(text_of(&first, "screen:1:reply"), "reply: Hello there, friend.");
    let reply = first.iter().find(|item| item["id"] == "screen:1:reply").unwrap();
    assert_eq!(reply["finalAnswer"], true, "the turn is over, so the reply is final");
    let end = first.iter().find(|item| item["id"] == "screen:1:end").unwrap();
    assert_eq!(end["outcome"], "completed");
    // None of the agent's chrome reached the chat.
    let all = serde_json::to_string(&first).unwrap();
    for chrome in ["Generating", "ctrl+c to stop", "Type a task", "done in", "NIMBUS", "Thought"] {
        assert!(!all.contains(chrome), "{chrome:?} leaked into the chat: {all}");
    }

    ask(&session, &pane, &chat, "Say the second thing please", 2);
    let both = chat.items();
    assert_eq!(
        chat.ids(),
        ["screen:1:user", "screen:1:tool:0", "screen:1:reply", "screen:1:end", "screen:2:user", "screen:2:reply", "screen:2:end"],
        "{both:#?}"
    );
    assert_eq!(&both[..4], &first[..], "turn 1 did not change when turn 2 was added");
    assert_eq!(text_of(&both, "screen:2:user"), "Say the second thing please");
    assert_eq!(text_of(&both, "screen:2:reply"), "reply: The quick brown fox.");
    // Items only ever grew or changed in place: the viewer was never told to start over.
    let (batches, resets) = chat.sent();
    assert!(batches >= 6, "the chat followed the turns as they went ({batches} batches)");
    assert_eq!(resets, 0, "a reset was sent: {both:#?}");
}
