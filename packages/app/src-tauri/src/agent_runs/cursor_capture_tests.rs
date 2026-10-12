//! Visible Cursor pane: what a turn's captured result is, asserted on the returned answer itself.
//!
//! Screens are built in the shape of the real captures (two-space indent, a banner, the prompt as
//! a transcript row, the reply back at the same column, the input box and the footer below).
//! Nothing here infers an answer from the prompt: every expectation is the reply text that was
//! put on the fake screen, and "nothing on screen" must stay "no result".

use super::completion::{
    clean_extracted_result_after, observe_tui_turn, TuiTurnDecision, TuiTurnWatch,
    NOT_SUBMITTED_TICKS,
};
use crate::operations::{
    self, AcceptanceStatus, DelegateWorkRequest, OperationStatus, ResultCapture, StateSource,
    WorkerPersist,
};
use uuid::Uuid;

const AGENT: &str = "cursor_agent";

const BANNER: &str = "  Cursor Agent\n  v2026.10.01-e373342\n  Tip: Use /plan to plan execution.\n";
const INPUT: &str = " \u{2584}\u{2584}\u{2584}\u{2584}\u{2584}\u{2584}\u{2584}\u{2584}\n  \u{2192} Add a follow-up\n \u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\u{2580}\n";

fn indent(text: &str) -> String {
    text.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The screen after `turns` (prompt, reply) pairs. `footer` is the model/status row.
fn screen_after(turns: &[(&str, &str)], footer: &str) -> String {
    let mut out = String::from(BANNER);
    for (prompt, reply) in turns {
        out.push('\n');
        out.push_str(&indent(prompt));
        out.push_str("\n\n");
        if !reply.is_empty() {
            out.push_str(&indent(reply));
            out.push_str("\n\n");
        }
    }
    out.push_str(INPUT);
    out.push_str(&format!("  {footer}\n  ~\\proj\n"));
    out
}

fn ticks(watch: &mut TuiTurnWatch, status: &str, screen: &str, n: u32) -> Vec<TuiTurnDecision> {
    (0..n)
        .map(|_| observe_tui_turn(watch, status, screen, false, AGENT))
        .collect()
}

fn completed(decisions: &[TuiTurnDecision]) -> Option<String> {
    decisions.iter().find_map(|decision| match decision {
        TuiTurnDecision::Complete { result, .. } => Some(result.clone()),
        _ => None,
    })
}

/// One follow-up turn the way the supervisor sees it: baseline is the previous idle screen, the
/// pane is seen running with only the prompt echoed, then idle with the reply on screen.
fn follow_up(history: &[(&str, &str)], prompt: &str, reply: &str) -> Option<String> {
    let previous = history.last().map(|(p, _)| *p).unwrap_or_default();
    let baseline = screen_after(history, "Composer 2.5 \u{b7} 3.0%");
    let mut watch = TuiTurnWatch::from_baseline(baseline)
        .with_task(prompt)
        .with_previous_task(previous);
    let echo_only = screen_after(
        &[history, &[(prompt, "")]].concat(),
        "Composer 2.5 \u{b7} 3.0%",
    );
    let _ = observe_tui_turn(&mut watch, "running", &echo_only, false, AGENT);
    let done = screen_after(
        &[history, &[(prompt, reply)]].concat(),
        "Composer 2.5 \u{b7} 3.4%",
    );
    completed(&ticks(&mut watch, "idle", &done, 4))
}

const FIRST: (&str, &str) = ("Reply with only PONG, no tools.", "PONG");

#[test]
fn short_and_punctuation_only_replies_are_returned_as_the_answer() {
    let prompt = "Reply with only the requested value, no tools, no explanation.";
    for reply in [
        "[]", "{}", "()", "\"\"", "ok", "42", "no", "\u{e9}", "\u{65e5}\u{672c}\u{8a9e}", "\u{1f642}",
        "caf\u{e9} \u{2713}", "[1, 2]", "null", "0",
    ] {
        assert_eq!(
            follow_up(&[FIRST], prompt, reply).as_deref(),
            Some(reply),
            "reply {reply:?} was not captured"
        );
    }
}

#[test]
fn a_multiline_reply_keeps_every_line_in_order() {
    let prompt = "Answer on three lines, nothing else:\n6 times 7\nThe capital of France\nThe word bench backwards";
    assert_eq!(
        follow_up(&[FIRST], prompt, "42\nParis\nhcneb").as_deref(),
        Some("42\nParis\nhcneb")
    );
    assert_eq!(
        follow_up(&[FIRST], prompt, "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\n\u{4e16}\u{754c}\n\u{1f30d}").as_deref(),
        Some("\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\n\u{4e16}\u{754c}\n\u{1f30d}")
    );
}

#[test]
fn consecutive_follow_ups_each_return_their_own_answer() {
    let second = ("Reply with only [], no tools, no explanation.", "[]");
    let third_prompt = "Answer on three lines:\n6 times 7\nThe capital of France\nThe word bench backwards";
    let mut history = vec![FIRST];
    assert_eq!(follow_up(&history, second.0, second.1).as_deref(), Some("[]"));
    history.push(second);
    // The previous answers are on screen but are not this turn's answer.
    assert_eq!(
        follow_up(&history, third_prompt, "42\nParis\nhcneb").as_deref(),
        Some("42\nParis\nhcneb")
    );
    history.push((third_prompt, "42\nParis\nhcneb"));
    // The same short answer twice in a row is still a fresh result.
    assert_eq!(
        follow_up(&history, "Reply with only [] again.", "[]").as_deref(),
        Some("[]")
    );
}

#[test]
fn a_slow_reply_is_waited_for_not_given_up_on() {
    // Cursor shows the echoed prompt and an idle-looking footer for several seconds before the
    // first token (model latency). One busy tick was seen, so this used to end after 1s as
    // "idle with nothing to capture" and the later reply was lost.
    let prompt = "Reply with only [], no tools, no explanation.";
    let baseline = screen_after(&[FIRST], "Composer 2.5 \u{b7} 3.0%");
    let mut watch = TuiTurnWatch::from_baseline(baseline)
        .with_task(prompt)
        .with_previous_task(FIRST.0);
    let echo_only = screen_after(&[FIRST, (prompt, "")], "Composer 2.5 \u{b7} 3.0%");
    let _ = observe_tui_turn(&mut watch, "running", &echo_only, false, AGENT);
    let waiting = ticks(&mut watch, "idle", &echo_only, 20);
    assert!(
        waiting.iter().all(|d| *d == TuiTurnDecision::Continue),
        "gave up while the reply was still pending: {waiting:?}"
    );
    let done = screen_after(&[FIRST, (prompt, "[]")], "Composer 2.5 \u{b7} 3.4%");
    assert_eq!(completed(&ticks(&mut watch, "idle", &done, 3)).as_deref(), Some("[]"));
}

#[test]
fn nothing_captured_stays_null_with_an_honest_end_state() {
    // The pane really never printed a reply: the run ends ambiguous, never with invented text.
    let prompt = "Reply with only [], no tools, no explanation.";
    let baseline = screen_after(&[FIRST], "Composer 2.5 \u{b7} 3.0%");
    let mut watch = TuiTurnWatch::from_baseline(baseline)
        .with_task(prompt)
        .with_previous_task(FIRST.0);
    let echo_only = screen_after(&[FIRST, (prompt, "")], "Composer 2.5 \u{b7} 3.0%");
    let _ = observe_tui_turn(&mut watch, "running", &echo_only, false, AGENT);
    let decisions = ticks(&mut watch, "idle", &echo_only, 200);
    assert!(completed(&decisions).is_none(), "{decisions:?}");
    assert_eq!(decisions.last(), Some(&TuiTurnDecision::AmbiguousIdle));
}

#[test]
fn an_echoed_prompt_alone_never_satisfies_capture() {
    // The prompt contains the text a result_contains check looks for. Its echo on screen must
    // not become the result, wrapped or not, however long the pane sits idle.
    let prompt = "Reply with only [], no tools, no explanation.";
    let baseline = screen_after(&[FIRST], "Composer 2.5 \u{b7} 3.0%");
    let wrapped = format!(
        "{}\n\n  Reply with only [], no tools,\n  no explanation.\n\n{INPUT}  Composer 2.5\n",
        baseline.split(INPUT).next().unwrap()
    );
    for echo in [screen_after(&[FIRST, (prompt, "")], "Composer 2.5 \u{b7} 3.0%"), wrapped] {
        let mut watch = TuiTurnWatch::from_baseline(baseline.clone())
            .with_task(prompt)
            .with_previous_task(FIRST.0);
        let decisions = ticks(&mut watch, "idle", &echo, NOT_SUBMITTED_TICKS as u32);
        assert!(completed(&decisions).is_none(), "{decisions:?}");
    }
    assert!(clean_extracted_result_after(prompt, prompt, FIRST.0, AGENT).is_none());
    assert!(clean_extracted_result_after("Reply with only [], no tools,\nno explanation.", prompt, "", AGENT).is_none());

    // End to end: with the check configured, an ambiguous end fails acceptance with no result.
    let project = std::env::temp_dir().join(format!("pm-cap-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let op = running_turn(&project_path, vec![contains("[]")]);
    let done = operations::mark_operation_finished(
        &project_path,
        &op.operation_id,
        None,
        ResultCapture::Missing,
        false,
    )
    .unwrap();
    assert_eq!(done.status, OperationStatus::Failed);
    assert!(done.result.is_none());
    assert_eq!(done.error.as_ref().map(|e| e.code.as_str()), Some("ACCEPTANCE_FAILED"));
    let _ = std::fs::remove_dir_all(project);
}

// ---- debris -------------------------------------------------------------------------------

fn clean(evidence: &str) -> Option<String> {
    clean_extracted_result_after(evidence, "", "", AGENT)
}

#[test]
fn composer_footer_and_status_rows_are_not_part_of_the_answer() {
    for footer in [
        "Composer 2.5 \u{b7} 7.2%",
        "Composer 2.5 \u{b7} 1 file edited",
        "Composer 2.5 \u{b7} 12.4% \u{b7} 3 files edited",
        "Composer 2.5 \u{b7} 2 files edited \u{b7} +14 -3",
        "GPT-5 \u{b7} 1 file edited",
    ] {
        assert_eq!(
            clean(&format!("All 12 tests pass.\n{footer}")).as_deref(),
            Some("All 12 tests pass."),
            "{footer}"
        );
    }
    assert!(clean("Composer 2.5 \u{b7} 1 file edited").is_none());
}

#[test]
fn an_answer_that_mentions_composer_is_kept() {
    for reply in [
        "Composer 2.5 is the model I am running on.",
        "I used Composer to draft it \u{b7} then reviewed it.",
        "Composer",
        "Composer 2.5 \u{b7} fast and cheap, that is my one-line review.",
        "The files edited were todo.py and notes.md.",
        "1 file edited",
    ] {
        assert_eq!(clean(reply).as_deref(), Some(reply), "{reply}");
    }
}

#[test]
fn command_echoes_and_tool_output_are_separated_from_the_final_text() {
    let evidence = "Let me check the file first.\n$ cd \"c:\\proj\" && python todo.py list 3s\n    1 [ ] buy-milk\n    2 [ ] call-bob\nEdited todo.py +3 -1\nComposer 2.5 \u{b7} 1 file edited\nThere are two open items.";
    assert_eq!(clean(evidence).as_deref(), Some("There are two open items."));
}

#[test]
fn repeated_commentary_from_redraws_appears_once() {
    let line = "I will now update the helper in todo.py to handle empty input.";
    let evidence = format!("{line}\nChecking the tests next.\n{line}\nDone: the helper handles empty input.");
    let cleaned = clean(&evidence).unwrap();
    assert_eq!(cleaned.matches(line).count(), 1, "{cleaned}");
    assert!(cleaned.ends_with("Done: the helper handles empty input."));
    // Short lines are real data and may legitimately repeat.
    assert_eq!(clean("ha\nso\nha").as_deref(), Some("ha\nso\nha"));
}

// ---- provenance ---------------------------------------------------------------------------

fn contains(text: &str) -> super::checks::Check {
    super::checks::Check::ResultContains { text: text.into() }
}

fn running_turn(project: &str, checks: Vec<super::checks::Check>) -> operations::OperationSnapshot {
    let request = DelegateWorkRequest {
        project_path: project.into(),
        task: "Reply with only [], no tools, no explanation.".into(),
        agent_type: "cursor_agent".into(),
        pane_id: Some("pane-1".into()),
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: true,
        placement: None,
        owner_session_id: Some("cap-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some(format!("cap-{}", Uuid::new_v4().simple())),
        turn_index: 0,
        worker: WorkerPersist::default(),
        context_policy: None,
        checks,
    };
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(project, &queued.operation_id, |snapshot| {
        snapshot.pane_id = Some("pane-1".into());
        Ok(())
    })
    .unwrap()
}

#[test]
fn passing_checks_do_not_turn_a_screen_scrape_into_an_authoritative_result() {
    let project = std::env::temp_dir().join(format!("pm-cap-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let op = running_turn(&project_path, vec![contains("[]")]);
    let done = operations::mark_operation_finished(
        &project_path,
        &op.operation_id,
        Some("[]".into()),
        ResultCapture::Inferred,
        false,
    )
    .unwrap();
    assert_eq!(done.status, OperationStatus::Completed);
    assert_eq!(done.acceptance_status, AcceptanceStatus::Passed);
    assert_eq!(done.result_capture, Some(ResultCapture::Inferred));
    assert!(!done.verified, "a passed check on scraped text is not a verified result");
    assert_eq!(done.source, StateSource::Inferred);
    let receipt = super::runtime::AgentRunView::from(done).receipt.expect("receipt");
    assert_eq!(receipt.result_capture, "inferred");
    assert!(
        receipt
            .not_verified
            .iter()
            .any(|item| item == "result read from terminal screen"),
        "{:?}",
        receipt.not_verified
    );
    let _ = std::fs::remove_dir_all(project);
}

/// A first-turn screen whose footer cwd was hard-wrapped by the TUI at `width` columns.
fn wrapped_footer_screen(prompt: &str, reply: &str, cwd: &str, width: usize) -> String {
    let mut out = String::from(BANNER);
    out.push('\n');
    out.push_str(&indent(prompt));
    out.push_str("\n\n");
    if !reply.is_empty() {
        out.push_str(&indent(reply));
        out.push_str("\n\n");
    }
    out.push_str(INPUT);
    out.push_str("  Composer 2.5 \u{b7} 3.4%\n");
    let chars: Vec<char> = cwd.chars().collect();
    for chunk in chars.chunks(width) {
        out.push_str("  ");
        out.extend(chunk);
        out.push('\n');
    }
    out
}

#[test]
fn a_footer_cwd_wrapped_over_several_rows_is_not_part_of_the_answer() {
    let cwd = r"~\AppData\Local\Temp\claude\C--Users-Ren-pc-Desktop-work2-puppet-master\7f91bd8b\scratchpad\live-2";
    for width in [20, 33, 48, 60, 80] {
        let baseline = screen_after(&[], "Composer 2.5");
        let mut watch = TuiTurnWatch::from_baseline(baseline).with_task(FIRST.0);
        let echo = wrapped_footer_screen(FIRST.0, "", cwd, width);
        let _ = observe_tui_turn(&mut watch, "running", &echo, false, AGENT);
        let done = wrapped_footer_screen(FIRST.0, "PONG", cwd, width);
        assert_eq!(
            completed(&ticks(&mut watch, "idle", &done, 4)).as_deref(),
            Some("PONG"),
            "width {width}"
        );
    }
}

#[test]
fn an_answer_that_looks_like_a_path_survives_when_it_is_not_the_footer() {
    let cwd = r"~\AppData\Local\Temp\claude\scratchpad\live-2";
    for reply in [r"d\live-2", r"C:\proj\src\main.rs", "src/lib.rs"] {
        let mut watch = TuiTurnWatch::from_baseline(screen_after(&[], "Composer 2.5")).with_task(FIRST.0);
        let _ = observe_tui_turn(&mut watch, "running", &wrapped_footer_screen(FIRST.0, "", cwd, 24), false, AGENT);
        let done = wrapped_footer_screen(FIRST.0, reply, cwd, 24);
        assert_eq!(completed(&ticks(&mut watch, "idle", &done, 4)).as_deref(), Some(reply));
    }
}

#[test]
fn debris_without_a_turn_start_is_never_a_completed_result() {
    // Live (2026-10-04): the task prompt was swallowed by a trust dialog, the pane returned to an
    // empty composer, and the run completed with footer debris as its result. With a non-blank
    // baseline, no busy tick and no echo of the task, nothing proves the turn started.
    let prompt = "Reply with only PONG, no tools.";
    let baseline = screen_after(&[], "Composer 2.5");
    for debris in [r"d\live-2", "Update available: v2026.11.01", "Run npm i -g cursor-agent to upgrade"] {
        let mut watch = TuiTurnWatch::from_baseline(baseline.clone()).with_task(prompt);
        let screen = baseline.replacen(INPUT, &format!("  {debris}\n\n{INPUT}"), 1);
        let decisions = ticks(&mut watch, "idle", &screen, NOT_SUBMITTED_TICKS as u32 + 2);
        assert!(completed(&decisions).is_none(), "{debris:?}: {decisions:?}");
        assert!(decisions.contains(&TuiTurnDecision::NotSubmitted), "{decisions:?}");
    }
}

#[test]
fn a_very_short_task_echo_still_counts_as_a_turn_start() {
    // "hi" is too short for the 8-character echo prefix; its echo is still new on screen.
    let baseline = screen_after(&[], "Composer 2.5");
    let mut watch = TuiTurnWatch::from_baseline(baseline).with_task("hi");
    let done = screen_after(&[("hi", "Hello!")], "Composer 2.5 \u{b7} 3.4%");
    let result = completed(&ticks(&mut watch, "idle", &done, 4)).expect("a turn that echoed its task completes");
    assert!(result.ends_with("Hello!"), "{result}");
}

// ---- replies longer than the screen ---------------------------------------------------------

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn numbered(count: usize) -> String {
    (1..=count).map(|n| format!("line {n:02}")).collect::<Vec<_>>().join("\n")
}

/// What the supervisor would capture from the visible rows alone.
fn visible_result(visible: &str, baseline: &str, task: &str) -> String {
    super::completion::tui_turn_evidence(visible, baseline, AGENT)
        .and_then(|raw| clean_extracted_result_after(&raw, task, "", AGENT))
        .expect("visible evidence")
}

#[test]
fn a_reply_longer_than_the_screen_is_read_back_from_the_scrollback() {
    // Live (2026-10-04): a 60-line reply came back as lines 37-60.
    let prompt = "Print the numbers one per line, 60 of them.";
    let reply = numbered(60);
    let baseline = screen_after(&[], "Composer 2.5");
    let full = screen_after(&[(prompt, &reply)], "Composer 2.5 \u{b7} 3.4%");
    let visible = last_lines(&full, 24);
    let partial = visible_result(&visible, &baseline, prompt);
    assert!(!partial.contains("line 01"), "the screen alone cannot hold the reply");

    let widened = super::completion::widen_reply(&visible, &full, prompt, "", &partial, AGENT);
    assert!(!widened.truncated);
    assert_eq!(widened.text, reply);
}

#[test]
fn a_reply_whose_start_cannot_be_located_is_flagged_truncated_not_passed_off_as_whole() {
    let prompt = "Print the numbers one per line, 60 of them.";
    let reply = numbered(60);
    let baseline = screen_after(&[], "Composer 2.5");
    let full = screen_after(&[(prompt, &reply)], "Composer 2.5 \u{b7} 3.4%");
    let visible = last_lines(&full, 24);
    let partial = visible_result(&visible, &baseline, prompt);
    // No scrollback available (alternate screen, or history shorter than the reply).
    let widened = super::completion::widen_reply(&visible, &visible, prompt, "", &partial, AGENT);
    assert!(widened.truncated);
    assert_eq!(widened.text, partial);
}

#[test]
fn a_reply_that_fits_the_screen_is_untouched() {
    let prompt = "Print the numbers one per line, 3 of them.";
    let baseline = screen_after(&[], "Composer 2.5");
    let full = screen_after(&[(prompt, "line 01\nline 02\nline 03")], "Composer 2.5 \u{b7} 3.4%");
    let partial = visible_result(&full, &baseline, prompt);
    let widened = super::completion::widen_reply(&full, &full, prompt, "", &partial, AGENT);
    assert!(!widened.truncated);
    assert_eq!(widened.text, "line 01\nline 02\nline 03");
}

#[test]
fn a_repeated_prompt_widens_from_the_latest_turn_only() {
    let prompt = "Print the numbers one per line, 60 of them.";
    let first = numbered(10);
    let second = numbered(60);
    let baseline = screen_after(&[(prompt, &first)], "Composer 2.5");
    let full = screen_after(&[(prompt, &first), (prompt, &second)], "Composer 2.5 \u{b7} 3.4%");
    let visible = last_lines(&full, 24);
    let partial = visible_result(&visible, &baseline, prompt);
    let widened = super::completion::widen_reply(&visible, &full, prompt, prompt, &partial, AGENT);
    assert!(!widened.truncated);
    assert_eq!(widened.text, second);
}
