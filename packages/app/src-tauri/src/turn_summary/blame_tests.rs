//! Tests for `turn_summary::blame`: latest turn wins, unknown files get nothing, paths are normalised.

use super::blame::{blame, normalise, FileBlame};
use super::model::{assemble, FileStats, Facts, TurnSummary};
use std::collections::HashMap;

/// A finished turn of `pane` that listed `paths`, ending at `ended_at_ms`.
fn turn(pane: &str, agent: &str, turn: u32, ended_at_ms: u64, paths: &[&str]) -> TurnSummary {
    assemble(Facts {
        pane_id: pane.into(),
        agent: agent.into(),
        agent_id: None,
        account: None,
        turn,
        started_at_ms: ended_at_ms.saturating_sub(1_000),
        ended_at_ms,
        files: Some(FileStats {
            count: paths.len() as u32,
            added: 1,
            deleted: 0,
            paths: paths.iter().map(|p| p.to_string()).collect(),
        }),
        approvals: vec![],
        end_status: "idle".into(),
        end_prompt: None,
        receipt: None,
    })
}

fn titles(map: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = map.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |pane| map.get(pane).cloned()
}

fn no_titles(_: &str) -> Option<String> {
    None
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn latest_turn_wins_across_panes_and_within_one() {
    let summaries = vec![
        turn("p1", "Claude Code", 1, 1_000, &["src/a.ts"]),
        turn("p2", "Codex", 1, 3_000, &["src/a.ts"]),
        // Same pane, later turn, but it ended before p2's turn: p2 still wins.
        turn("p1", "Claude Code", 2, 2_000, &["src/a.ts"]),
    ];
    let found = blame(&summaries, &names(&["src/a.ts"]), &no_titles);
    assert_eq!(found["src/a.ts"], FileBlame { agent: "Codex".into(), pane_title: None, at_ms: 3_000 });

    // A later turn of the first pane takes the file back.
    let mut more = summaries.clone();
    more.push(turn("p1", "Claude Code", 3, 4_000, &["src/a.ts"]));
    let found = blame(&more, &names(&["src/a.ts"]), &no_titles);
    assert_eq!(found["src/a.ts"].agent, "Claude Code");
    assert_eq!(found["src/a.ts"].at_ms, 4_000);
}

#[test]
fn the_higher_turn_wins_when_two_turns_end_at_the_same_time() {
    let summaries = vec![turn("p1", "Codex", 4, 5_000, &["a.txt"]), turn("p1", "Codex", 5, 5_000, &["a.txt"])];
    let found = blame(&summaries, &names(&["a.txt"]), &no_titles);
    assert_eq!(found["a.txt"].at_ms, 5_000);
}

#[test]
fn a_file_no_summary_lists_gets_no_entry() {
    let summaries = vec![turn("p1", "Codex", 1, 1_000, &["src/a.ts"])];
    let found = blame(&summaries, &names(&["src/other.ts", "src/a.ts"]), &no_titles);
    assert!(!found.contains_key("src/other.ts"));
    assert!(found.contains_key("src/a.ts"));
    // No summaries at all: nothing, never a guess.
    assert!(blame(&[], &names(&["src/a.ts"]), &no_titles).is_empty());
}

#[test]
fn a_summary_without_file_facts_names_no_file() {
    let mut unknown = turn("p1", "Codex", 1, 1_000, &["a.txt"]);
    unknown.files = None;
    assert!(blame(&[unknown], &names(&["a.txt"]), &no_titles).is_empty());
}

#[test]
fn paths_are_folder_relative_and_normalised() {
    let summaries = vec![turn("p1", "Codex", 1, 1_000, &["src/a.ts"])];
    // Windows separators, "./" and doubled slashes all name the same file.
    for raw in ["src\\a.ts", "./src/a.ts", "src//a.ts", "  src/a.ts  "] {
        let found = blame(&summaries, &names(&[raw]), &no_titles);
        assert!(found.contains_key("src/a.ts"), "{raw} should match");
    }
    // Absolute paths and paths that leave the folder never match.
    let outside = vec![turn("p1", "Codex", 1, 1_000, &["/etc/passwd", "../x.txt", "C:/x.txt"])];
    for raw in ["/etc/passwd", "../x.txt", "C:/x.txt", "C:\\x.txt", "", "."] {
        assert!(blame(&outside, &names(&[raw]), &no_titles).is_empty(), "{raw:?} must not match");
    }
    // A summary's own stored paths are normalised too.
    let stored_backslash = vec![turn("p1", "Codex", 1, 1_000, &["src\\b.ts"])];
    assert!(blame(&stored_backslash, &names(&["src/b.ts"]), &no_titles).contains_key("src/b.ts"));
}

#[test]
fn normalise_rules() {
    assert_eq!(normalise("a\\b\\c.txt").as_deref(), Some("a/b/c.txt"));
    assert_eq!(normalise("./a/./b").as_deref(), Some("a/b"));
    assert_eq!(normalise("a/../b"), None);
    assert_eq!(normalise("/a"), None);
    assert_eq!(normalise("D:/a"), None);
    assert_eq!(normalise("   "), None);
}

#[test]
fn pane_title_is_included_when_the_pane_is_titled() {
    let summaries = vec![turn("p1", "Codex", 1, 1_000, &["a.txt"]), turn("p2", "Claude Code", 1, 2_000, &["b.txt"])];
    let titled = titles(&[("p1", "helper"), ("p2", "   ")]);
    let found = blame(&summaries, &names(&["a.txt", "b.txt"]), &titled);
    assert_eq!(found["a.txt"].pane_title.as_deref(), Some("helper"));
    // A blank title counts as no title.
    assert_eq!(found["b.txt"].pane_title, None);
}
