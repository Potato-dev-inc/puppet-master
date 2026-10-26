//! A reply that is a slice of the task text, printed under the agent's tool rows (found by a real wizard run: the task
//! was "List the files in this folder, then reply with exactly: folder listed, code X." and the reply row was
//! `folder listed, code X.`). The reading cuts the turn at the last tool row, so the task's echo row, which is above
//! the tools, was never in the text that was cleaned; the reply row was then judged to be an echo of the task, with
//! nothing earlier to prove it was a second copy, and dropped. The turn was declared ambiguous with the answer on screen.
//!
//! Generic: nothing here names an agent; the made-up profile only has the tables a drafted profile has.

use std::sync::Arc;

use crate::agent_runs::completion::{observe_tui_turn, TuiTurnDecision, TuiTurnWatch};

const AGENT: &str = "tools-reply-probe";

const PROFILE: &str = r##"
schema_version = 1

[agent]
id = "tools-reply-probe"
label = "Tools reply probe"
experimental = true

[binary]
names = ["tools-reply-probe"]

[tui]
argv = []

[tui.chrome]
line_regex = [
  '^[\u2580-\u259f\s]+',
  'cli \d+\.\d+',
  '^~/',
  '^\? for shortcuts',
  '^esc to cancel',
  '^>\s*$',
]

[tui.thinking]
line_regex = ['^\W?\s*thought for [\d.]+s \(']

[tui.summary]
line_regex = ['^thought for \d+s, \d+ tokens']

[tui.tool_activity]
line_regex = ['^\W?\s*(ran|explored) \d+ (commands?|files?)']
"##;

const TASK: &str = "List the files in this folder, then reply with exactly: folder listed, code plum-7421.";
const FIRST: &str = "Reply with exactly the word: pear-1180";

/// Registered once for the whole test run (the tests of this file run side by side and share it).
fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let profile = Arc::new(
            crate::tui::declarative::DeclarativeProfile::new(crate::tui::declarative::parse_definition(PROFILE).expect("parses")).expect("builds"),
        );
        crate::tui::registry::registry().add_transient(profile).expect("registered");
    });
}

fn rule() -> String {
    "\u{2500}".repeat(44)
}

fn banner() -> String {
    "\n      \u{2584}\u{2580}\u{2580}\u{2584}        Probe CLI 1.4.7\n     \u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}       Small Model (High)\n   \u{2584}\u{2580}\u{2580}    \u{2580}\u{2580}\u{2584}     ~/tmp/test-project\n".to_string()
}

fn footer() -> String {
    format!("{}\n>\n{}\n? for shortcuts                                Small Model \u{b7} high\n", rule(), rule())
}

/// The pane right after the second task was sent and before anything was printed under it.
fn echo_only() -> String {
    format!("{}\n{}\n> {FIRST}\n\n  pear-1180\n\n{}\n> {TASK}\n\n{}", banner(), rule(), rule(), footer())
}

/// The real screen at the failure: tool rows under the echo, then the reply row, which is a slice of the task.
fn answered() -> String {
    format!(
        "{}\n{}\n> {FIRST}\n\n  pear-1180\n\n{}\n> {TASK}\n\n\
         \u{25cb} Thought for 9.6s (The request includes two distinct actions: listing files and providing a specific phrase. The p...)\n\
         \u{25cf} Ran 7 commands (+3 \u{b7} Get-Process | Where-... \u{b7} Get-ChildItem -Path ...)\n\
         \u{25cf} Explored 1 file (notes.ts) \n\
         \u{25cf} Ran 3 commands, killed 1 task (Select-String -Path \"C:\\Users\\user\\...)\n\
         \u{25cf} Explored 1 file (main.rs) (ctrl+o to expand)\n\n\
         \u{25b8} Thought for 4s, 380 tokens\n\n  folder listed, code plum-7421.\n\n{}",
        banner(),
        rule(),
        rule(),
        footer()
    )
}

fn watch() -> TuiTurnWatch {
    TuiTurnWatch::from_baseline(format!("{}\n{}\n> {FIRST}\n\n  pear-1180\n\n{}", banner(), rule(), footer()))
        .with_task(TASK)
        .with_previous_task(FIRST)
        .with_verified_delivery(true)
}

#[test]
fn a_reply_that_repeats_the_task_text_under_the_tool_rows_is_read_as_the_reply() {
    register();
    let mut watch = watch();
    let _ = observe_tui_turn(&mut watch, "running", &echo_only(), false, AGENT);
    let screen = answered();
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..4 {
        last = observe_tui_turn(&mut watch, "idle", &screen, false, AGENT);
        if matches!(last, TuiTurnDecision::Complete { .. }) {
            break;
        }
    }
    match last {
        TuiTurnDecision::Complete { result, .. } => assert_eq!(result, "folder listed, code plum-7421."),
        other => panic!("the reply is on the screen, got {other:?}"),
    }
}

/// The same screen with the first echo row only: no tool rows, the reply is still told from the echo.
#[test]
fn the_echo_row_itself_is_still_not_a_reply() {
    register();
    let mut watch = watch();
    let _ = observe_tui_turn(&mut watch, "running", &echo_only(), false, AGENT);
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..6 {
        last = observe_tui_turn(&mut watch, "idle", &echo_only(), false, AGENT);
    }
    assert!(!matches!(last, TuiTurnDecision::Complete { .. }), "{last:?}");
}


/// The same turn without its answer yet: silent tool work.
fn working_silently() -> String {
    format!("{}\n{}\n> {FIRST}\n\n  pear-1180\n\n{}\n> {TASK}\n\n\u{25cb} Thought for 2.1s (Listing the files...)\n\n{}", banner(), rule(), rule(), footer())
}

fn question() -> String {
    format!("{}\nRun this command?\n> 1. Yes, run command\n  2. No, cancel\n", working_silently())
}

/// A turn that stopped on a question and went on after it was answered runs its command in silence: that is not a turn
/// that ended without a reply. (The same run: the pane read idle with no reply under long tool work.)
#[test]
fn silent_work_after_an_answered_question_is_not_declared_ambiguous_on_the_short_wait() {
    register();
    let silent = working_silently();
    let short = crate::agent_runs::completion::AWAITING_REPLY_TICKS as usize + 5;
    // without a question the short wait ends the turn
    let mut plain = watch();
    let _ = observe_tui_turn(&mut plain, "running", &echo_only(), false, AGENT);
    let decisions: Vec<_> = (0..short).map(|_| observe_tui_turn(&mut plain, "idle", &silent, false, AGENT)).collect();
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::AmbiguousIdle));
    // with one, the turn is still being waited for
    let mut asked = watch();
    let _ = observe_tui_turn(&mut asked, "running", &echo_only(), false, AGENT);
    assert_eq!(observe_tui_turn(&mut asked, "waiting_input", &question(), true, AGENT), TuiTurnDecision::WaitingInput);
    let decisions: Vec<_> = (0..short).map(|_| observe_tui_turn(&mut asked, "idle", &silent, false, AGENT)).collect();
    assert!(decisions.iter().all(|d| *d == TuiTurnDecision::Continue), "{decisions:?}");
    // and the reply is still read when it comes
    let mut last = TuiTurnDecision::Continue;
    for _ in 0..3 {
        last = observe_tui_turn(&mut asked, "idle", &answered(), false, AGENT);
        if matches!(last, TuiTurnDecision::Complete { .. }) {
            break;
        }
    }
    assert!(matches!(last, TuiTurnDecision::Complete { .. }), "{last:?}");
}
