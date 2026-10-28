//! Builds the whole plan of one turn (spec 3.2 to 3.6): the time window of the turn, its terminal
//! replay (or its steps), the redaction of every screen, the request and the result facts. This is the
//! one place where the clip pieces meet, and it has no Tauri types, so the whole path is unit-testable.
//!
//! The plan is tried in this order, and the first one that fits `MAX_PLAN_BYTES` is kept:
//! 1. the terminal replay with 100 ms keyframes,
//! 2. the same replay with 200 ms keyframes,
//! 3. the activity scene (the steps as a list, no terminal frames).
//!
//! Only the person's message, tool steps and the turn's facts go in. Agent text and tool output are
//! never read here.

use std::sync::Arc;

use super::model::{ClipPlan, Replay, Step, MAX_FRAMES, MAX_PLAN_BYTES, REPLAY_MAX_MS};
use super::screens::{replay_every, Replayed, KEYFRAME_MS, SLOW_KEYFRAME_MS};
use super::scrub::{clean_line, scrub};
use super::steps;
use super::window::{find_turn, window_for, Turn, NOTHING};
use crate::flight::ring::{Ring, Snapshot};
use crate::redact::RedactContext;
use crate::session_chat::model::{ChatItem, TurnOutcome};
use crate::turn_summary::TurnSummary;

/// Said on the sheet when the steps are shown instead of the terminal record.
pub const NOT_AVAILABLE: &str = "The terminal record of this turn is not available. Showing the steps instead.";
/// Said when the ring dropped the first part of the turn. The replay still runs from what is kept.
pub const TRUNCATED: &str = "The first part of this turn was not recorded.";
/// Said when the terminal is wider than `WIDE_COLS`, so the replayed text is small.
pub const WIDE: &str = "The terminal is wide, so the text is small.";
/// Refusal when even the steps-only plan is over `MAX_PLAN_BYTES`. The caps make this unreachable.
pub const TOO_BIG: &str = "This turn is too big to clip.";

/// Most notes on one plan.
const MAX_NOTES: usize = 3;
/// Longest agent label, in characters.
const AGENT_CHARS: usize = 40;
/// Wider than this many columns, the replayed text is small.
const WIDE_COLS: u16 = 133;
/// Clip time kept after the last step of the activity scene, so its last line stays a moment.
const ACTIVITY_TAIL_MS: u32 = 1_000;

/// Everything the plan is built from.
pub struct PlanInput {
    /// From `Plans::issue`, one per plan.
    pub token: String,
    /// The agent's display label, for example "Claude Code".
    pub agent_label: String,
    /// The chat items of the pane.
    pub items: Vec<ChatItem>,
    /// The `User` item that starts the turn. `None` is the last turn.
    pub user_item_id: Option<String>,
    /// The pane's turn summaries (`turn_summary::tracker().summaries`).
    pub summaries: Vec<TurnSummary>,
    /// The pane's terminal record (`flight::get`), when the app still has it.
    pub ring: Option<Arc<Ring>>,
    /// Known secret values and the home folder, for redaction.
    pub ctx: RedactContext,
}

/// Builds the plan of the turn. It takes the ring's snapshot and redacts every screen, so call it on a
/// blocking thread. An error is one plain sentence for the sheet.
pub fn build(input: PlanInput) -> Result<ClipPlan, String> {
    let snap = input.ring.as_ref().map(|ring| ring.snapshot());
    build_from(&input, snap.as_ref(), MAX_PLAN_BYTES)
}

/// `build` with the snapshot and the byte limit given, so tests can reach the fallbacks.
fn build_from(input: &PlanInput, snap: Option<&Snapshot>, limit: usize) -> Result<ClipPlan, String> {
    let turn = find_turn(&input.items, input.user_item_id.as_deref())?;
    let (start, end, summary) = window_for(&turn, &input.summaries)?;
    let summary = summary.as_ref();
    let ctx = &input.ctx;
    let base = ClipPlan {
        token: input.token.clone(),
        agent: clean_line(&input.agent_label, AGENT_CHARS, ctx),
        request: steps::request_text(&turn, ctx),
        turn_ms: end.saturating_sub(start),
        outcome: outcome_of(&turn, summary),
        replay: Replay::None,
        steps: Vec::new(),
        result: steps::result_facts(summary, ctx),
        notes: Vec::new(),
        hidden: 0,
    };

    // The terminal replay: 100 ms keyframes, then 200 ms when the first one is too big.
    for every in [KEYFRAME_MS, SLOW_KEYFRAME_MS] {
        let Some(replayed) = snap.and_then(|snap| replay_every(snap, start, end, every)) else { break };
        if !replayed.covered {
            // The ring starts too late for this turn. A slower keyframe will not change that.
            break;
        }
        if replayed.frames.len() > MAX_FRAMES {
            continue;
        }
        let plan = terminal_plan(&base, &turn, (start, end), &replayed, ctx);
        if fits(&plan, limit) {
            return Ok(plan);
        }
    }

    let plan = activity_plan(&base, &turn, (start, end), summary.is_some(), ctx);
    if fits(&plan, limit) {
        Ok(plan)
    } else {
        Err(TOO_BIG.to_string())
    }
}

/// The plan with the terminal replay `replayed`, redacted, and the steps on the same clock.
fn terminal_plan(
    base: &ClipPlan,
    turn: &Turn,
    window: (u64, u64),
    replayed: &Replayed,
    ctx: &RedactContext,
) -> ClipPlan {
    let scrubbed = scrub(&replayed.frames, replayed.cols, ctx);
    let mut plan = base.clone();
    if replayed.truncated {
        plan.notes.push(TRUNCATED.to_string());
    }
    if replayed.cols > WIDE_COLS {
        plan.notes.push(WIDE.to_string());
    }
    plan.steps = steps::steps(turn, window, Some(&replayed.time_map), ctx);
    plan.hidden = scrubbed.hidden;
    plan.replay = Replay::Terminal {
        cols: replayed.cols,
        rows: replayed.rows,
        row_from: scrubbed.row_from,
        row_to: scrubbed.row_to,
        speed_x: replayed.speed_x,
        length_ms: replayed.length_ms,
        row_table: scrubbed.row_table,
        frames: scrubbed.frames,
    };
    plan.notes.truncate(MAX_NOTES);
    plan
}

/// The plan with the steps as a list (the activity scene). With no steps and no summary there is
/// nothing to show, so the replay is `None` and the sheet says so.
fn activity_plan(base: &ClipPlan, turn: &Turn, window: (u64, u64), has_summary: bool, ctx: &RedactContext) -> ClipPlan {
    let mut plan = base.clone();
    let steps = steps::steps(turn, window, None, ctx);
    if steps.is_empty() && !has_summary {
        plan.notes = vec![NOTHING.to_string()];
        return plan;
    }
    if !steps.is_empty() {
        plan.notes.push(NOT_AVAILABLE.to_string());
    }
    plan.replay = Replay::Activity { length_ms: activity_length(&steps) };
    plan.steps = steps;
    plan.notes.truncate(MAX_NOTES);
    plan
}

/// Length of the activity scene: up to the last step plus a moment, at most the replay limit. Zero
/// without steps (the clip is then the title, the result and the end card).
fn activity_length(steps: &[Step]) -> u32 {
    steps.last().map_or(0, |last| (last.at_ms + ACTIVITY_TAIL_MS).min(REPLAY_MAX_MS))
}

/// "completed", "interrupted" or "failed". A turn with no `TurnEnd` was cut short by a later message.
fn outcome_of(turn: &Turn, summary: Option<&TurnSummary>) -> String {
    let ended = turn.items.iter().find_map(|item| match item {
        ChatItem::TurnEnd { outcome, .. } => Some(outcome),
        _ => None,
    });
    let word = match ended {
        Some(TurnOutcome::Completed) => "completed",
        Some(TurnOutcome::Failed) => "failed",
        Some(TurnOutcome::Interrupted) => "interrupted",
        None if summary.is_some_and(|s| s.end.kind == "error") => "failed",
        None => "interrupted",
    };
    word.to_string()
}

/// True when the plan's JSON is at most `limit` bytes.
fn fits(plan: &ClipPlan, limit: usize) -> bool {
    serde_json::to_vec(plan).is_ok_and(|json| json.len() <= limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::window::STILL_RUNNING;
    use crate::flight::ring::{Event, Kind};
    use crate::flight::InputTag;
    use crate::session_chat::ToolDetail;
    use crate::turn_summary::model::{EndState, FileStats};
    use chrono::{DateTime, SecondsFormat, Utc};

    /// Unix time of a hand-built snapshot's start, in ms.
    const START: u64 = 1_000_000;

    /// A key shaped like the real ones. It is built here, so no real secret sits in the file.
    fn key(letters: &str) -> String {
        format!("sk-ant-api03-{}", letters.repeat(20))
    }

    /// Unix ms as RFC 3339, the form chat items carry.
    fn iso(ms: u64) -> String {
        DateTime::<Utc>::from_timestamp_millis(ms as i64)
            .expect("a time in range")
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    fn ctx() -> RedactContext {
        RedactContext { env_values: vec!["hunter2-env-value".into()], home: Some("C:\\Users\\you".into()) }
    }

    /// A finished turn: the request, one `Read` call with its ok result, and the end when `end` is set.
    fn turn_items(start: u64, request: &str, end: Option<(TurnOutcome, u64)>) -> Vec<ChatItem> {
        let mut items = vec![
            ChatItem::User { id: "u1".into(), at: Some(iso(start)), text: request.into() },
            ChatItem::ToolCall {
                id: "c1".into(),
                at: Some(iso(start + 200)),
                call_id: "k1".into(),
                name: "Read".into(),
                summary: "Read the env file".into(),
                detail: ToolDetail { paths: vec!["C:\\Users\\you\\proj\\.env".into()], ..Default::default() },
            },
            ChatItem::ToolResult { id: "r1".into(), at: Some(iso(start + 300)), call_id: "k1".into(), ok: true, output: String::new() },
        ];
        if let Some((outcome, at)) = end {
            items.push(ChatItem::TurnEnd { id: "e1".into(), at: Some(iso(at)), outcome, duration_ms: Some(at - start) });
        }
        items
    }

    fn finished(start: u64, end: u64) -> Vec<ChatItem> {
        turn_items(start, "Fix the login", Some((TurnOutcome::Completed, end)))
    }

    /// The summary of the turn `[start, end]`, with one changed file.
    fn summary(start: u64, end: u64) -> TurnSummary {
        TurnSummary {
            pane_id: "pane-1".into(),
            agent: "Claude Code".into(),
            agent_id: None,
            account: None,
            turn: 1,
            started_at_ms: start,
            ended_at_ms: end,
            duration_ms: end - start,
            files: Some(FileStats { count: 1, added: 3, deleted: 1, paths: vec!["C:\\Users\\you\\proj\\.env".into()] }),
            approvals: Vec::new(),
            end: EndState { kind: "idle".into(), prompt: None },
            receipt: None,
            sentence: "Read the env file.".into(),
        }
    }

    fn input(items: Vec<ChatItem>, summaries: Vec<TurnSummary>, ring: Option<Arc<Ring>>) -> PlanInput {
        PlanInput {
            token: "tok-1".into(),
            agent_label: "Claude Code".into(),
            items,
            user_item_id: None,
            summaries,
            ring,
            ctx: ctx(),
        }
    }

    /// A snapshot built by hand: each output is `(ms after START, bytes)`, on an 80 by 24 grid.
    fn snap_of(outputs: &[(u64, Vec<u8>)], truncated: bool) -> Snapshot {
        let mut bytes = Vec::new();
        let mut events = Vec::new();
        for (ms, data) in outputs {
            let off = bytes.len() as u64;
            bytes.extend_from_slice(data);
            events.push(Event { t_us: ms * 1000, off, len: data.len() as u32, aux: 0, kind: Kind::Output });
        }
        Snapshot { start_unix_ms: START as i64, base_cols: 80, base_rows: 24, events, bytes, first_off: 0, truncated }
    }

    fn json_of(plan: &ClipPlan) -> String {
        serde_json::to_string(plan).expect("serializes")
    }

    #[test]
    fn a_key_in_the_ring_and_in_the_request_never_reaches_the_plan() {
        let ring = Arc::new(Ring::new(60, 8));
        let start = ring.snapshot().start_unix_ms as u64;
        let key = key("A1");
        let (head, tail) = key.split_at(20);
        // A key the person typed is input, never replayed.
        ring.input(InputTag::User, format!("{key}\r").as_bytes());
        ring.output(format!("$ cat .env\r\nANTHROPIC_API_KEY={key}\r\n").as_bytes());
        // A key split by a colour change between its letters.
        ring.output(format!("split={head}\x1b[1m{tail}\x1b[0m\r\n").as_bytes());
        ring.output(b"open C:\\Users\\you\\proj\\.env\r\n");
        let items = turn_items(start, &format!("Use {key} for the build"), Some((TurnOutcome::Completed, start + 3_000)));
        let plan = build(input(items, vec![summary(start, start + 3_000)], Some(ring))).expect("the turn is finished");

        let json = json_of(&plan);
        assert!(!json.contains("sk-ant"), "a key reached the plan: {json}");
        assert!(!json.contains(&"A1".repeat(8)), "a key fragment reached the plan: {json}");
        assert!(!json.contains("Users"), "a home folder reached the plan: {json}");
        assert!(!json.contains("hunter2"), "a known value reached the plan: {json}");
        // The terminal replay ran, so the screen rows are in the plan (masked, not missing).
        assert!(json.contains("ANTHROPIC_API_KEY"), "the replay did not run: {json}");
        assert!(matches!(plan.replay, Replay::Terminal { .. }));
        assert!(plan.hidden >= 1);
        assert!(plan.request.contains("[REDACTED]"));
        assert_eq!(plan.token, "tok-1");
        assert_eq!(plan.agent, "Claude Code");
        assert_eq!(plan.outcome, "completed");
        assert_eq!(plan.turn_ms, 3_000);
    }

    #[test]
    fn a_clean_screen_hides_nothing_and_adds_no_note() {
        let ring = Arc::new(Ring::new(40, 4));
        let start = ring.snapshot().start_unix_ms as u64;
        ring.output(b"cargo test\r\nok\r\n");
        let plan = build(input(finished(start, start + 3_000), vec![summary(start, start + 3_000)], Some(ring)))
            .expect("the turn is finished");
        assert_eq!(plan.hidden, 0);
        assert!(plan.notes.is_empty());
        assert!(matches!(plan.replay, Replay::Terminal { .. }));
    }

    #[test]
    fn each_changed_row_with_different_text_counts_once() {
        let ring = Arc::new(Ring::new(60, 8));
        let start = ring.snapshot().start_unix_ms as u64;
        let (a, b) = (key("A1"), key("B2"));
        // Two identical rows count once, a third row with another secret counts again.
        ring.output(format!("A={a}\r\nA={a}\r\nB={b}\r\n").as_bytes());
        let plan = build(input(finished(start, start + 3_000), vec![summary(start, start + 3_000)], Some(ring)))
            .expect("the turn is finished");
        assert_eq!(plan.hidden, 2);
    }

    #[test]
    fn without_the_ring_the_steps_are_shown_with_a_note() {
        let start = 1_700_000_000_000;
        let plan = build_from(
            &input(finished(start, start + 3_000), vec![summary(start, start + 3_000)], None),
            None,
            MAX_PLAN_BYTES,
        )
        .expect("builds");
        assert!(matches!(plan.replay, Replay::Activity { length_ms: 1_000 }));
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].at_ms, 0);
        assert_eq!(plan.steps[0].ok, Some(true));
        assert_eq!(plan.notes, vec![NOT_AVAILABLE.to_string()]);
        assert_eq!(plan.hidden, 0);
    }

    #[test]
    fn a_ring_that_dropped_its_start_keeps_the_replay_and_adds_a_note() {
        // Output from 1 s on; the ring dropped older bytes, but it still covers the turn.
        let snap = snap_of(&[(1_000, b"build ok\r\n".to_vec())], true);
        let plan = build_from(
            &input(finished(START, START + 3_000), vec![summary(START, START + 5_000)], None),
            Some(&snap),
            MAX_PLAN_BYTES,
        )
        .expect("builds");
        assert!(matches!(plan.replay, Replay::Terminal { .. }), "{plan:?}");
        assert_eq!(plan.notes, vec![TRUNCATED.to_string()]);
    }

    #[test]
    fn a_ring_that_starts_too_late_for_the_turn_gives_the_steps() {
        // The first output is 4 s into a 5 s turn: the ring covers less than half of it.
        let snap = snap_of(&[(4_000, b"late\r\n".to_vec())], true);
        let plan = build_from(
            &input(finished(START, START + 3_000), vec![summary(START, START + 5_000)], None),
            Some(&snap),
            MAX_PLAN_BYTES,
        )
        .expect("builds");
        assert!(matches!(plan.replay, Replay::Activity { .. }), "{plan:?}");
        assert_eq!(plan.notes, vec![NOT_AVAILABLE.to_string()]);
    }

    #[test]
    fn an_unfinished_turn_is_refused() {
        let items = turn_items(START, "Fix it", None);
        let err = build(input(items, Vec::new(), None)).expect_err("the turn is still running");
        assert_eq!(err, STILL_RUNNING);
    }

    #[test]
    fn a_turn_with_nothing_to_show_has_no_replay() {
        // Timed items and no summary, no ring, no tool calls: the window is the items' own times.
        let items = vec![
            ChatItem::User { id: "u1".into(), at: Some(iso(START)), text: "Hello".into() },
            ChatItem::TurnEnd { id: "e1".into(), at: Some(iso(START + 500)), outcome: TurnOutcome::Completed, duration_ms: None },
        ];
        let plan = build(input(items, Vec::new(), None)).expect("builds");
        assert!(matches!(plan.replay, Replay::None));
        assert_eq!(plan.notes, vec![NOTHING.to_string()]);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn the_request_is_one_line_of_at_most_140_characters() {
        let long = format!("{} ", "please fix the login page".repeat(20));
        let plan = build(input(turn_items(START, &long, Some((TurnOutcome::Completed, START + 3_000))), Vec::new(), None))
            .expect("builds");
        assert!(plan.request.chars().count() <= 140, "{}", plan.request.len());
        assert!(!plan.request.contains('\n'));
    }

    #[test]
    fn the_outcome_follows_the_turn_end() {
        let failed = turn_items(START, "Run it", Some((TurnOutcome::Failed, START + 3_000)));
        assert_eq!(build(input(failed, Vec::new(), None)).expect("builds").outcome, "failed");
        // A later message ends a turn that has no end item: it was cut short.
        let mut cut = turn_items(START, "Run it", None);
        cut.push(ChatItem::User { id: "u2".into(), at: Some(iso(START + 4_000)), text: "Stop".into() });
        let plan_input = PlanInput { user_item_id: Some("u1".into()), ..input(cut, Vec::new(), None) };
        let plan = build_from(&plan_input, None, MAX_PLAN_BYTES).expect("builds");
        assert_eq!(plan.outcome, "interrupted");
    }

    #[test]
    fn a_plan_over_the_limit_with_no_steps_is_refused() {
        let items = finished(START, START + 3_000);
        let err = build_from(&input(items, vec![summary(START, START + 3_000)], None), None, 10)
            .expect_err("the plan cannot fit in 10 bytes");
        assert_eq!(err, TOO_BIG);
    }

    /// A hash of `x` (splitmix64), so the letters of the redraws do not repeat in a pattern.
    fn mix(x: u64) -> u64 {
        let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Full-screen redraws, 100 ms apart, every cell a new letter in a colour: about 11 KB a frame, about
    /// 2 MB in all, and every row of every frame is different text. A space every seventh cell keeps
    /// the letters from reading as one opaque run (strict redaction masks those).
    fn redraws(count: u64) -> Vec<(u64, Vec<u8>)> {
        (0..count)
            .map(|n| {
                let mut frame = String::from("\x1b[H");
                for row in 0..24u64 {
                    for col in 0..80u64 {
                        let letter = if col % 7 == 6 {
                            ' '
                        } else {
                            char::from(b'a' + (mix(n * 100_000 + row * 100 + col) % 26) as u8)
                        };
                        // Red and yellow alternate: they differ in dimness, so each cell is its own run.
                        frame.push_str(&format!("\x1b[{}m{letter}", 31 + (col % 2) * 2));
                    }
                    frame.push_str("\r\n");
                }
                (n * 100, frame.into_bytes())
            })
            .collect()
    }

    #[test]
    fn a_two_megabyte_redraw_stays_under_the_plan_limit() {
        let snap = snap_of(&redraws(180), false);
        let end = START + 18_000;
        let plan_input = input(finished(START, end), vec![summary(START, end)], None);

        // With no limit the plan uses 100 ms keyframes, and it is over the real limit.
        let full = build_from(&plan_input, Some(&snap), usize::MAX).expect("builds");
        let full_size = json_of(&full).len();
        assert!(full_size > MAX_PLAN_BYTES, "the redraws are too small to test the limit: {full_size}");

        // One byte under that size, the plan falls back to 200 ms keyframes.
        let slow = build_from(&plan_input, Some(&snap), full_size - 1).expect("builds");
        let Replay::Terminal { frames, length_ms, .. } = &slow.replay else {
            panic!("expected the terminal replay at 200 ms, got {:?}", slow.replay);
        };
        assert!(frames.iter().all(|f| f.at_ms % 200 == 0 || f.at_ms == *length_ms));
        assert!(json_of(&slow).len() < full_size);

        // With the real limit, the plan is always within it.
        let fitted = build_from(&plan_input, Some(&snap), MAX_PLAN_BYTES).expect("builds");
        assert!(json_of(&fitted).len() <= MAX_PLAN_BYTES);
    }
}
