//! The request, the tool steps and the result facts of a clip. Only safe text goes in: the person's
//! message, tool names, one-line summaries, last file names, counts and the turn's one-line sentence.
//!
//! Every string goes through `plain` first (strict redaction, e-mail masking, paths cut to their last
//! names), then to one line and a length limit. Assistant text and tool output are never read here,
//! and no tool input leaves this module (only the flags and paths of `ToolDetail`).

use std::collections::HashMap;

use super::model::{FilesFact, ResultFacts, Step, MAX_STEPS};
use super::screens::TimeMap;
use super::window::{item_ms, Turn};
use crate::redact::RedactContext;
use crate::session_chat::export_text::{last_name, plain};
use crate::session_chat::model::ChatItem;
use crate::turn_summary::model::{receipt_part, TurnSummary};

/// Longest request, in characters.
const REQUEST_CHARS: usize = 140;
/// Longest tool name and tool summary, in characters.
const NAME_CHARS: usize = 40;
const SUMMARY_CHARS: usize = 60;
/// Longest file name, in characters.
const FILE_CHARS: usize = 40;
/// Longest checks line and result sentence, in characters.
const CHECKS_CHARS: usize = 60;
const SENTENCE_CHARS: usize = 160;
/// Most file names in the result.
const MAX_NAMES: usize = 6;
/// Activity scene: one step every this many clip milliseconds.
const ACTIVITY_STEP_MS: u32 = 900;
/// The replay keeps output from this long before and after the window (spec 3.4). Steps outside it
/// are not on screen, so they are left out of a terminal replay.
const SLACK_BEFORE_MS: u64 = 300;
const SLACK_AFTER_MS: u64 = 700;

/// The tool steps of a turn, in order, at most `MAX_STEPS`.
///
/// `map` gives clip time from the replay (`TimeMap::clip_ms`). Without it (the activity scene) step `i`
/// is at `i * 900` ms. A step without a time takes the time of the step before it.
pub fn steps(turn: &Turn, window: (u64, u64), map: Option<&TimeMap>, ctx: &RedactContext) -> Vec<Step> {
    let results = results_by_call(turn.items);
    let from = window.0.saturating_sub(SLACK_BEFORE_MS);
    let to = window.1.saturating_add(SLACK_AFTER_MS);
    let mut out: Vec<Step> = Vec::new();
    for item in turn.items {
        let ChatItem::ToolCall { call_id, name, summary, detail, .. } = item else {
            continue;
        };
        if out.len() == MAX_STEPS {
            break;
        }
        let at_ms = match map {
            None => out.len() as u32 * ACTIVITY_STEP_MS,
            Some(map) => match item_ms(item) {
                Some(unix) if unix < from || unix > to => continue,
                Some(unix) => map.clip_ms(unix),
                None => out.last().map_or(0, |step| step.at_ms),
            },
        };
        out.push(Step {
            at_ms,
            name: one_line(name, NAME_CHARS, ctx),
            summary: one_line(summary, SUMMARY_CHARS, ctx),
            paths: detail.paths.iter().map(|path| file_name(path, ctx)).collect(),
            writes: detail.writes,
            helper: detail.helper,
            ok: results.get(call_id.as_str()).copied(),
        });
    }
    out
}

/// The person's message as one redacted line, at most `REQUEST_CHARS` characters. Empty when the turn
/// has no `User` item.
pub fn request_text(turn: &Turn, ctx: &RedactContext) -> String {
    match turn.user {
        Some(ChatItem::User { text, .. }) => one_line(text, REQUEST_CHARS, ctx),
        _ => String::new(),
    }
}

/// The result scene's facts: file totals, the checks line and the one-line sentence of the turn.
/// With no summary, only the empty sentence is set.
pub fn result_facts(summary: Option<&TurnSummary>, ctx: &RedactContext) -> ResultFacts {
    let Some(summary) = summary else {
        return ResultFacts { files: None, checks: None, sentence: String::new() };
    };
    let files = summary.files.as_ref().map(|files| FilesFact {
        count: files.count,
        added: files.added,
        deleted: files.deleted,
        names: files.paths.iter().take(MAX_NAMES).map(|path| file_name(path, ctx)).collect(),
    });
    let checks = summary
        .receipt
        .as_ref()
        .and_then(receipt_part)
        .map(|text| one_line(&text, CHECKS_CHARS, ctx));
    ResultFacts { files, checks, sentence: one_line(&summary.sentence, SENTENCE_CHARS, ctx) }
}

/// Whether each tool call's result was ok, by call id. A call with no result is not in the map.
fn results_by_call(items: &[ChatItem]) -> HashMap<&str, bool> {
    items
        .iter()
        .filter_map(|item| match item {
            ChatItem::ToolResult { call_id, ok, .. } => Some((call_id.as_str(), *ok)),
            _ => None,
        })
        .collect()
}

/// A file's last name, redacted, at most `FILE_CHARS` characters.
fn file_name(path: &str, ctx: &RedactContext) -> String {
    let cleaned = plain(path, ctx);
    one_line(&last_name(&cleaned), FILE_CHARS, ctx)
}

/// Redacts the whole text first (so a secret that spans lines is still found), then joins it into one
/// line and cuts it to at most `max` characters. A cut text ends with "…" and is still at most `max`.
fn one_line(text: &str, max: usize, ctx: &RedactContext) -> String {
    let flat = plain(text, ctx).split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::screens::replay;
    use crate::flight::ring::{Event, Kind, Snapshot};
    use crate::session_chat::detail::ToolDetail;
    use crate::session_chat::model::TurnOutcome;
    use crate::turn_summary::model::{EndState, FileStats, ReceiptFacts};

    /// Unix time of the ring's start, in ms.
    const START: u64 = 1_000_000;

    fn ctx() -> RedactContext {
        RedactContext { env_values: Vec::new(), home: Some("C:\\Users\\you".into()) }
    }

    /// A fake key of the right shape, built here so no real secret sits in the source.
    fn fake_key() -> String {
        format!("sk-ant-api03-{}", "A1".repeat(20))
    }

    fn user(text: &str) -> ChatItem {
        ChatItem::User { id: "u1".into(), at: None, text: text.into() }
    }

    fn call(n: usize, name: &str, summary: &str, detail: ToolDetail) -> ChatItem {
        ChatItem::ToolCall {
            id: format!("c{n}"),
            at: None,
            call_id: format!("call-{n}"),
            name: name.into(),
            summary: summary.into(),
            detail,
        }
    }

    fn result(n: usize, ok: bool) -> ChatItem {
        ChatItem::ToolResult { id: format!("r{n}"), at: None, call_id: format!("call-{n}"), ok, output: String::new() }
    }

    fn turn(items: &[ChatItem]) -> Turn<'_> {
        Turn { items, user: items.first(), finished: true, newest: true }
    }

    fn summary(files: Option<FileStats>, receipt: Option<ReceiptFacts>, sentence: &str) -> TurnSummary {
        TurnSummary {
            pane_id: "pane-1".into(),
            agent: "Test Agent".into(),
            agent_id: None,
            account: None,
            turn: 1,
            started_at_ms: START,
            ended_at_ms: START + 5_000,
            duration_ms: 5_000,
            files,
            approvals: Vec::new(),
            end: EndState { kind: "idle".into(), prompt: None },
            receipt,
            sentence: sentence.into(),
        }
    }

    #[test]
    fn a_matching_tool_result_gives_ok() {
        let items = vec![
            user("fix it"),
            call(1, "Bash", "cargo test", ToolDetail::default()),
            call(2, "Read", "a.rs", ToolDetail::default()),
            call(3, "Edit", "b.rs", ToolDetail::default()),
            result(1, false),
            result(3, true),
        ];
        let steps = steps(&turn(&items), (0, 0), None, &ctx());
        assert_eq!(steps.iter().map(|s| s.ok).collect::<Vec<_>>(), vec![Some(false), None, Some(true)]);
    }

    #[test]
    fn secrets_in_the_request_and_a_summary_are_masked() {
        let key = fake_key();
        let items = vec![
            user(&format!("use {key} for the deploy")),
            call(1, "Bash", &format!("export TOKEN={key}"), ToolDetail::default()),
        ];
        let steps = steps(&turn(&items), (0, 0), None, &ctx());
        let request = request_text(&turn(&items), &ctx());
        assert!(!request.contains(&key), "request kept the key: {request}");
        assert!(!request.contains("A1A1A1"), "request kept the key body: {request}");
        assert!(!steps[0].summary.contains("A1A1A1"), "summary kept the key body: {}", steps[0].summary);
        assert!(steps[0].summary.chars().count() <= SUMMARY_CHARS);
    }

    #[test]
    fn tool_names_pass_through() {
        let items = vec![
            user("look"),
            call(1, "Read", "a.rs", ToolDetail::default()),
            call(2, "Bash", "git status", ToolDetail::default()),
        ];
        let names: Vec<String> = steps(&turn(&items), (0, 0), None, &ctx()).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["Read", "Bash"]);
    }

    #[test]
    fn a_turn_keeps_at_most_forty_steps_in_order() {
        let mut items = vec![user("many")];
        for n in 0..50 {
            items.push(call(n, "Read", &format!("file {n}.rs"), ToolDetail::default()));
        }
        let steps = steps(&turn(&items), (0, 0), None, &ctx());
        assert_eq!(steps.len(), MAX_STEPS);
        assert_eq!(steps[0].summary, "file 0.rs");
        assert_eq!(steps[MAX_STEPS - 1].summary, "file 39.rs");
        // The activity scene: one step every 900 ms.
        assert_eq!(steps[2].at_ms, 1800);
    }

    #[test]
    fn paths_become_last_names() {
        let detail = ToolDetail {
            input: None,
            paths: vec!["C:\\Users\\you\\proj\\src\\a.rs".into(), "src/b.rs".into()],
            writes: true,
            helper: false,
        };
        let items = vec![user("edit"), call(1, "Edit", "a.rs", detail)];
        let steps = steps(&turn(&items), (0, 0), None, &ctx());
        assert_eq!(steps[0].paths, vec!["a.rs", "b.rs"]);
        assert!(steps[0].writes);
        assert!(!steps[0].helper);
    }

    #[test]
    fn an_e_mail_in_the_request_is_masked() {
        let items = vec![user("send the report to maya@example.test today")];
        let request = request_text(&turn(&items), &ctx());
        assert!(!request.contains('@'), "request kept an address: {request}");
        assert!(request.contains("report"));
    }

    #[test]
    fn the_request_is_one_line_and_at_most_140_characters() {
        let items = vec![user("first line\n\nsecond   line")];
        assert_eq!(request_text(&turn(&items), &ctx()), "first line second line");

        // Plain words, so redaction leaves them alone (a 30+ character opaque run would be masked).
        let long = vec![user(&"show the report ".repeat(20))];
        let request = request_text(&turn(&long), &ctx());
        assert_eq!(request.chars().count(), REQUEST_CHARS);
        assert!(request.ends_with('…'));
    }

    #[test]
    fn terminal_steps_follow_the_replay_time_map() {
        // Output at 1 s into a 5 s ring window: the replay starts at START.
        let snap = Snapshot {
            start_unix_ms: START as i64,
            base_cols: 20,
            base_rows: 4,
            events: vec![Event { t_us: 1_000_000, off: 0, len: 5, aux: 0, kind: Kind::Output }],
            bytes: b"hello".to_vec(),
            first_off: 0,
            truncated: false,
        };
        let replayed = replay(&snap, START, START + 5_000).expect("replays");
        let map = &replayed.time_map;
        let inside = ChatItem::ToolCall {
            id: "c1".into(),
            at: Some("1970-01-01T00:16:41Z".into()),
            call_id: "call-1".into(),
            name: "Read".into(),
            summary: "a.rs".into(),
            detail: ToolDetail::default(),
        };
        // A call on another day is far outside the window, so it is not on screen and is left out.
        let outside = ChatItem::ToolCall {
            id: "c2".into(),
            at: Some("2026-10-28T10:00:00Z".into()),
            call_id: "call-2".into(),
            name: "Bash".into(),
            summary: "far away".into(),
            detail: ToolDetail::default(),
        };
        let items = vec![user("run"), inside, outside];
        let steps = steps(&turn(&items), (START, START + 5_000), Some(map), &ctx());
        assert_eq!(steps.len(), 1, "the far call must be dropped: {steps:?}");
        assert_eq!(steps[0].at_ms, map.clip_ms(START + 1_000));
    }

    #[test]
    fn result_facts_name_the_files_checks_and_sentence() {
        let files = FileStats {
            count: 7,
            added: 86,
            deleted: 12,
            paths: (1..=7).map(|n| format!("C:\\Users\\you\\proj\\f{n}.rs")).collect(),
        };
        let receipt = ReceiptFacts { checks: "2/2 passed".into(), outcome: "passed_checks".into() };
        let facts = result_facts(Some(&summary(Some(files), Some(receipt), "Test Agent edited 7 files.")), &ctx());
        let files = facts.files.expect("files");
        assert_eq!((files.count, files.added, files.deleted), (7, 86, 12));
        assert_eq!(files.names, vec!["f1.rs", "f2.rs", "f3.rs", "f4.rs", "f5.rs", "f6.rs"]);
        assert_eq!(facts.checks.as_deref(), Some("all checks passed (2/2)"));
        assert_eq!(facts.sentence, "Test Agent edited 7 files.");
    }

    #[test]
    fn result_facts_mask_a_secret_in_the_sentence() {
        let key = fake_key();
        let s = summary(None, None, &format!("Test Agent printed {key}."));
        let facts = result_facts(Some(&s), &ctx());
        assert!(facts.files.is_none());
        assert!(facts.checks.is_none());
        assert!(!facts.sentence.contains("A1A1A1"), "sentence kept the key: {}", facts.sentence);
    }

    #[test]
    fn no_summary_gives_no_files_and_an_empty_sentence() {
        let facts = result_facts(None, &ctx());
        assert!(facts.files.is_none());
        assert!(facts.checks.is_none());
        assert_eq!(facts.sentence, "");
    }

    #[test]
    fn a_turn_end_and_other_items_are_not_steps() {
        let items = vec![
            user("go"),
            ChatItem::TurnEnd { id: "e".into(), at: None, outcome: TurnOutcome::Completed, duration_ms: None },
            call(1, "Read", "a.rs", ToolDetail::default()),
        ];
        assert_eq!(steps(&turn(&items), (0, 0), None, &ctx()).len(), 1);
    }
}
