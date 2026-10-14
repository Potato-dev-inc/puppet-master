//! VERIFY-R7 finding D: the Transcript's "Sending" entry was a wall of raw screen rows (the agent's
//! model footer, mode line, folder). It is one plain line now; the rows are kept as diagnostics that
//! a reader gets only with `details`. The inputs are the real delivery traces of the live runs.

use super::delivery::{delivery_summary, record_delivery, Delivery};
use super::transcript;

/// Codex 0.159.3, update notice closed first (op 3f82886d).
const CODEX_TRACE: &str = include_str!("../tui/codex/fixtures/delivery_trace_live.txt");
/// Claude Code 2.1.299 in manual mode (the edit run).
const CLAUDE_TRACE: &str = include_str!("../tui/claude/fixtures/delivery_trace_live.txt");

fn trace_of(raw: &str) -> Vec<String> {
    raw.lines().skip(1).map(str::to_string).collect()
}

fn project() -> String {
    std::env::temp_dir().join(format!("pm-delivery-{}", uuid::Uuid::new_v4())).to_string_lossy().into_owned()
}

#[test]
fn a_codex_delivery_reads_as_one_short_sentence() {
    let summary = delivery_summary(&Delivery::Started, &trace_of(CODEX_TRACE));
    assert_eq!(summary, "Sent. Closed a startup notice first. Composer cleared.");
}

#[test]
fn a_claude_delivery_carries_none_of_its_footer_rows() {
    let summary = delivery_summary(&Delivery::Started, &trace_of(CLAUDE_TRACE));
    assert_eq!(summary, "Sent. The agent started.");
    for raw in ["manual mode", "for shortcuts", "rows=", "\u{2500}", r"D:\pm-demo"] {
        assert!(!summary.contains(raw), "{raw:?}");
    }
}

#[test]
fn deliveries_that_did_not_happen_say_why_in_a_sentence() {
    let not_sent = delivery_summary(&Delivery::NotVisible, &[]);
    assert!(not_sent.starts_with("Not sent."), "{not_sent}");
    assert!(not_sent.contains("retrying is safe"));
    let gate = crate::tui::StartupGate { id: "login", summary: "Codex needs you to sign in", dismiss: None };
    let blocked = delivery_summary(&Delivery::Blocked(gate), &[]);
    assert!(blocked.starts_with("Not sent. Codex needs you to sign in;"), "{blocked}");
    assert!(delivery_summary(&Delivery::WriteFailed("pipe closed".into()), &[]).contains("pipe closed"));
    let trace = vec!["attempt 0: typing; busy=false rows=x".to_string(), "attempt 1: typing; busy=false rows=x".to_string(), "enter poll 0: Enter sent once more (composer=HasText)".to_string()];
    let again = delivery_summary(&Delivery::SubmittedUnconfirmed, &trace);
    assert!(again.contains("Typed it again") && again.contains("Enter was pressed twice") && again.contains("still watching"), "{again}");
}

#[test]
fn the_transcript_shows_the_summary_and_keeps_the_screen_rows_for_details() {
    let project = project();
    let op = "op-delivery";
    transcript::append(&project, op, "user_task", "Read README.md");
    record_delivery(&project, op, None, &Delivery::Started, &trace_of(CODEX_TRACE));
    transcript::append(&project, op, "assistant_final", "It is a tiny Node project.");

    let page = transcript::read(&project, op, 0).unwrap();
    let streams: Vec<&str> = page.chunks.iter().map(|c| c.stream.as_str()).collect();
    assert_eq!(streams, ["user_task", "delivery", "assistant_final"]);
    let sending = &page.chunks[1].text;
    assert_eq!(sending, "Sent. Closed a startup notice first. Composer cleared.");
    assert!(!page.truncated, "hiding diagnostics is not a gap");
    assert_eq!(page.next_cursor, 4, "the cursor still counts every stored entry");
    assert!(page.chunks.iter().all(|c| !c.text.contains("GPT-6-Luna") && !c.text.contains("for shortcuts")));

    let detailed = transcript::read_with(&project, op, 0, true).unwrap();
    let streams: Vec<&str> = detailed.chunks.iter().map(|c| c.stream.as_str()).collect();
    assert_eq!(streams, ["user_task", "delivery", "delivery_trace", "assistant_final"]);
    assert!(detailed.chunks[2].text.contains("GPT-6-Luna medium"), "the raw rows are still there for whoever asks");

    // Reading turns of a handle renumbers; the hidden entry leaves no gap.
    let turns = transcript::read_turns(&project, &[op.to_string()], 0).unwrap();
    assert_eq!(turns.chunks.iter().map(|c| c.sequence).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(!turns.truncated);
}

#[test]
fn a_second_delivery_is_marked_as_such() {
    let project = project();
    record_delivery(&project, "op-again", Some("prompt not submitted"), &Delivery::Started, &trace_of(CLAUDE_TRACE));
    let page = transcript::read(&project, "op-again", 0).unwrap();
    assert_eq!(page.chunks[0].text, "Sent again (prompt not submitted). Sent. The agent started.");
}
