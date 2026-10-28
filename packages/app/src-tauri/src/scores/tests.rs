//! Guards and the end-to-end test of "Scores on your code" (SC-12).
//!
//! - `not_reachable_from_agents`: no shipped file outside the scores module and its command wiring
//!   names the module, its command or a `/scores` route. Agents reach the app through the MCP tool
//!   registry and the bridge routes, so both are scanned and must be among the files checked.
//! - `report_has_no_private_text`: text planted in the facts (a token, a home path, an e-mail, a
//!   prompt, a pane id, a run handle) never reaches the serialised report.
//! - `no_network_symbols`: the `scores/` module names no http client and no socket.
//! - `build_reads_a_seeded_project_end_to_end`: real operation records and a ring file in a temp
//!   project, read back the way `gather` reads them, then `build`.
//!
//! The source scans are plain text scans, like `proc_util::guard`. Comments are skipped by the
//! route scan (a comment cannot route anything); the network scan blanks comments and string
//! contents, so only code is checked.

use super::aggregate::{Labels, DEFAULT_ACCOUNT};
use super::join::{join, ProjectSummary};
use super::model::{ScoreRow, ScoresReport};
use super::{build, build_in, Inputs};
use crate::operations::list_operations_lenient;
use crate::operations::OperationSnapshot;
use crate::project_path::comparable_project_path;
use crate::redact::scrub;
use crate::turn_summary::model::{assemble, Approval, Facts, FileStats};
use crate::turn_summary::store::{read_project, ring_path, Store};
use crate::turn_summary::TurnSummary;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const S: u64 = 1_000;
const HOUR: u64 = 60 * 60 * S;
const DAY: u64 = 24 * HOUR;
/// 2027-01-15 08:00 UTC.
const T: u64 = 1_800_000_000_000;

// ---- source scans ----

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir` as (path relative to `src/` with `/` separators, text), sorted.
fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).expect("read dir").filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let rel = path.strip_prefix(src_dir()).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.push((rel, fs::read_to_string(&path).expect("read source")));
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `needle` occurs in `line` and does not start in the middle of a longer identifier.
fn occurs_at_word_start(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| line[..at].chars().next_back().is_none_or(|c| !is_ident(c)))
}

/// `needle` occurs in `line` as a whole word (no identifier character on either side).
fn occurs_as_word(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| {
        let before = line[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let after = line[at + needle.len()..].chars().next().is_none_or(|c| !is_ident(c));
        before && after
    })
}

/// `text` with `//` and `/* */` comments and the contents of string and char literals blanked to
/// spaces, so only code is left. Line breaks are kept, so line numbers hold. Simple on purpose:
/// no nested block comments, and an escaped quote in a char literal is not handled.
fn code_only(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                blank(&mut out, c[i]);
                i += 1;
            }
            for _ in 0..2 {
                if i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
        } else if ch == 'r' && raw_hashes(&c, i).is_some() {
            let n = raw_hashes(&c, i).unwrap_or(0);
            let mut j = i + n + 2; // past `r`, the hashes and the opening quote
            while j < c.len() && !(c[j] == '"' && (1..=n).all(|k| c.get(j + k) == Some(&'#'))) {
                j += 1;
            }
            let end = (j + n + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '"' {
            blank(&mut out, ch);
            i += 1;
            while i < c.len() && c[i] != '"' {
                let escaped = c[i] == '\\';
                blank(&mut out, c[i]);
                i += 1;
                if escaped && i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
            if i < c.len() {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '\'' && c.get(i + 1) == Some(&'\\') {
            let mut j = i + 2;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            let end = (j + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '\'' && c.get(i + 2) == Some(&'\'') {
            for &x in &c[i..i + 3] {
                blank(&mut out, x);
            }
            i += 3;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// The number of `#` of a raw string opening (`r"` or `r#"`...) at `at`, when `r` starts a token.
fn raw_hashes(c: &[char], at: usize) -> Option<usize> {
    if at > 0 && is_ident(c[at - 1]) {
        return None;
    }
    let mut j = at + 1;
    while c.get(j) == Some(&'#') {
        j += 1;
    }
    (c.get(j) == Some(&'"')).then_some(j - at - 1)
}

// ---- 1. not reachable from agents ----

/// Files that may name the scores module: the command's registration and wiring, the write-only
/// undo hook, and the module itself (`scores/`, checked by its own guard). Relative to `src/`.
const MAY_NAME_SCORES: &[&str] = &["lib.rs", "commands/mod.rs", "commands/scores.rs", "checkpoints/runtime.rs"];

/// Ways to reach the report from code: the module path, its command, and a route-looking string.
const EXPOSURE_NEEDLES: &[&str] = &["crate::scores", "scores::", "mod scores", "agent_scores", "\"/scores", "\"scores\""];

/// `(line, needle)` of every exposure needle in `text`. Comment lines are skipped; string contents
/// are not, because a route is a string.
fn exposure_hits(text: &str) -> Vec<(usize, &'static str)> {
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for &needle in EXPOSURE_NEEDLES {
            if occurs_at_word_start(line, needle) {
                hits.push((index + 1, needle));
            }
        }
    }
    hits
}

#[test]
fn not_reachable_from_agents() {
    let mut files = Vec::new();
    sources(&src_dir(), &mut files);
    assert!(files.len() > 100, "the scan found only {} files; is the source tree layout different?", files.len());
    // The places an agent can reach must be among the files checked, or the guard proves nothing.
    for must in ["tool_registry/", "bridge/"] {
        assert!(files.iter().any(|(file, _)| file.starts_with(must)), "{must} is not scanned");
    }
    let mut hits = Vec::new();
    for (file, text) in &files {
        if file.starts_with("scores/") || MAY_NAME_SCORES.contains(&file.as_str()) {
            continue;
        }
        for (line, needle) in exposure_hits(text) {
            hits.push(format!("{file}:{line}  `{needle}`"));
        }
    }
    assert!(
        hits.is_empty(),
        "the scores page is reachable outside the Tauri command (agents must never see it):\n  {}\n\
         Only the files in MAY_NAME_SCORES may name the scores module.",
        hits.join("\n  ")
    );
}

/// The scanner itself: a route, a path and a bare name are seen; a comment and a longer name are not.
#[test]
fn the_route_scan_sees_routes_and_paths_and_skips_comments() {
    let fixture = r#"
// crate::scores is named in this comment only
use crate::scores::model::ScoresReport;
let path = "/scores/report";
let my_agent_scores = 1; let ok = "scores";
"#;
    let hits = exposure_hits(fixture);
    assert!(hits.contains(&(3, "crate::scores")), "{hits:?}");
    assert!(hits.contains(&(3, "scores::")), "{hits:?}");
    assert!(hits.contains(&(4, "\"/scores")), "{hits:?}");
    assert!(hits.contains(&(5, "\"scores\"")), "{hits:?}");
    assert_eq!(hits.len(), 4, "{hits:?}");
    assert!(!hits.iter().any(|(line, _)| *line == 2), "a comment line is not a route: {hits:?}");
}

// ---- 2. no private text in the report ----

const SHOP: &str = "C:/Users/you/shop";
const HOME_PATH: &str = r"C:\Users\you\secret-proj\src\main.rs";
const TOKEN: &str = "sk-test-FAKE-0001";
const EMAIL: &str = "you@example.com";

fn agent_label(id: &str) -> String {
    if id == "alpha" {
        "Alpha Code".to_string()
    } else {
        id.to_string()
    }
}

fn account_label(id: &str) -> Option<String> {
    (id == "acct-work").then(|| "Work".to_string())
}

fn labels() -> Labels<'static> {
    Labels { agent: &agent_label, account: &account_label }
}

fn no_label(_: &str) -> Option<String> {
    None
}

/// One finished turn of a run, shaped as the operation store writes it.
fn record(project: &str, id: &str, run: &str, turn: u32, pane: &str, status: &str, times: (u64, u64)) -> Value {
    let (start, end) = times;
    json!({
        "operation_id": id,
        "project_path": project,
        "task": format!("SECRET-TASK {id}"),
        "agent_type": "alpha",
        "pane_id": pane,
        "idempotency_key": format!("key-{id}"),
        "exclusive": false,
        "locks": [],
        "status": status,
        "revision": 1,
        "created_at_ms": start,
        "updated_at_ms": end,
        "source": "native",
        "result": "SECRET-RESULT done",
        "agent_run_id": run,
        "turn_index": turn,
        "started_at_ms": start,
        "finished_at_ms": end,
    })
}

/// A turn summary as the tracker builds it. Callers plant text on the fields afterwards.
fn summary(
    pane: &str,
    turn: u32,
    times: (u64, u64),
    end_status: &str,
    account: Option<&str>,
    files: Option<FileStats>,
    approvals: Vec<Approval>,
) -> TurnSummary {
    assemble(Facts {
        pane_id: pane.into(),
        agent: "Alpha Code".into(),
        agent_id: Some("alpha".into()),
        account: account.map(String::from),
        turn,
        started_at_ms: times.0,
        ended_at_ms: times.1,
        files,
        approvals,
        end_status: end_status.into(),
        end_prompt: None,
        receipt: None,
    })
}

fn files(count: u32, added: u32, deleted: u32, path: &str) -> Option<FileStats> {
    Some(FileStats { count, added, deleted, paths: vec![path.into()] })
}

#[test]
fn report_has_no_private_text() {
    let mut run = record(SHOP, "op-p1", "run-secret-handle", 1, "pane-secret-9", "completed", (T, T + 60 * S));
    run["task"] = json!(format!("SECRET-TASK use {TOKEN}"));
    run["result"] = json!(format!("SECRET-RESULT wrote {HOME_PATH} and {TOKEN}"));
    run["worker"] = json!({ "account": "acct-work" });
    let ops: Vec<OperationSnapshot> = vec![serde_json::from_value(run).expect("fixture record")];

    let approval = Approval {
        at_ms: T + 61 * S,
        prompt_id: "prompt-1".into(),
        prompt: format!("SECRET-PROMPT allow curl {EMAIL}?"),
        kind: "command".into(),
        decision: "allow".into(),
    };
    let mut s = summary("pane-secret-9", 1, (T + S, T + 62 * S), "waiting_input", Some("acct-work"), files(1, 3, 1, HOME_PATH), vec![approval]);
    s.end.prompt = Some(format!("SECRET-END paste {TOKEN}"));
    s.sentence = format!("SECRET-SENTENCE changed {HOME_PATH} and mailed {EMAIL}");

    let (turns, runs) = join(&ops, &[ProjectSummary { project: SHOP.into(), summary: s }], &[], None, &no_label);
    let mut project_names = BTreeMap::new();
    project_names.insert(comparable_project_path(Path::new(SHOP)), SHOP.to_string());
    let inputs = Inputs { turns, runs, project_names, ..Inputs::default() };

    let report = build_in(&inputs, None, None, T + HOUR, &Utc, &labels());
    let json = serde_json::to_string(&report).expect("report json");

    // The turn did reach the report, so the absence below means something.
    assert_eq!(report.rows.len(), 1, "{json}");
    assert_eq!(report.rows[0].turns, 1, "{json}");
    assert_eq!(report.rows[0].asked_you.count, 1, "{json}");
    assert!(json.contains("\"accountLabel\":\"Work\""), "the label the accounts store returns is shown: {json}");
    assert!(json.contains("\"agentLabel\":\"Alpha Code\""), "{json}");
    assert!(json.contains("\"label\":\"shop\""), "a project shows its folder name only: {json}");

    for secret in [TOKEN, "secret-proj", "Users", EMAIL, "you@", "@", "pane-secret", "run-secret", "SECRET-"] {
        assert!(!json.contains(secret), "{secret} leaked into the report: {json}");
    }
}

// ---- 3. no network symbols ----

/// Needles of an http client or a raw socket. Only code is searched (see `code_only`).
const NETWORK_NEEDLES: &[&str] = &[
    "reqwest", "ureq", "hyper", "isahc", "attohttpc", "minreq", "surf", "curl", "http", "tungstenite", "TcpStream", "TcpListener",
    "UdpSocket", "std::net", "tokio::net",
];

#[test]
fn no_network_symbols() {
    let mut files = Vec::new();
    sources(&src_dir().join("scores"), &mut files);
    for must in ["scores/join.rs", "scores/gather.rs", "scores/aggregate.rs", "scores/undo_log.rs"] {
        assert!(files.iter().any(|(file, _)| file == must), "{must} is not scanned");
    }
    let mut hits = Vec::new();
    for (file, text) in &files {
        for (index, line) in code_only(text).lines().enumerate() {
            for needle in NETWORK_NEEDLES {
                if occurs_as_word(line, needle) {
                    hits.push(format!("{file}:{}  `{needle}`", index + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "the scores module names a network symbol:\n  {}", hits.join("\n  "));
}

/// The scanner itself: an import is seen; the same words in a comment or a string are not.
#[test]
fn the_network_scan_sees_code_and_ignores_comments_and_strings() {
    let fixture = r##"
// use reqwest::Client; in a comment
let text = "ureq::get";
let raw = r#"hyper"#;
use std::net::TcpStream;
use reqwest::Client;
"##;
    let hits: Vec<(usize, &str)> = code_only(fixture)
        .lines()
        .enumerate()
        .flat_map(|(index, line)| NETWORK_NEEDLES.iter().filter(move |n| occurs_as_word(line, n)).map(move |n| (index + 1, *n)))
        .collect();
    assert_eq!(hits, [(5, "TcpStream"), (5, "std::net"), (6, "reqwest")], "{hits:?}");
}

// ---- 4. end to end over a seeded temp project ----

/// A temp project folder, removed when the test ends (the test's own folder only).
struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("pm-scores-e2e-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp project");
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn row<'a>(report: &'a ScoresReport, agent: &str, account: Option<&str>) -> &'a ScoreRow {
    report.rows.iter().find(|r| r.agent_id == agent && r.account_id.as_deref() == account).expect("a row for the agent and account")
}

#[test]
fn build_reads_a_seeded_project_end_to_end() {
    let tmp = Temp::new();
    let folder = tmp.0.clone();
    let project = folder.to_string_lossy().to_string();
    let ops_dir = folder.join(".puppet-master").join("operations");
    fs::create_dir_all(&ops_dir).expect("operations folder");
    let write_record = |value: &Value| {
        let id = value["operation_id"].as_str().expect("id").to_string();
        fs::write(ops_dir.join(format!("{id}.json")), serde_json::to_vec_pretty(value).expect("record json")).expect("write record");
    };

    // Run "merged": two turns, the latest one's copy was merged. The first turn had a passing check.
    let mut r1t1 = record(&project, "op-r1-t1", "run-merged", 1, "pane-1", "completed", (T, T + 60 * S));
    r1t1["worker"] = json!({ "account": "acct-work" });
    r1t1["checks"] = json!([{ "type": "result_contains", "text": "ok" }]);
    r1t1["check_results"] = json!([{ "check": { "type": "result_contains", "text": "ok" }, "passed": true, "detail": "found" }]);
    write_record(&r1t1);
    let mut r1t2 = record(&project, "op-r1-t2", "run-merged", 2, "pane-1", "completed", (T + 120 * S, T + 180 * S));
    r1t2["worker"] = json!({
        "account": "acct-work",
        "worktree": {
            "path": format!("{project}/.puppet-master/worktrees/run-merged"),
            "cwd": format!("{project}/.puppet-master/worktrees/run-merged"),
            "branch": "pm/run-merged", "base": "main", "base_commit": "abc123", "repo": project,
            "handle": "run-merged", "created_at_ms": T, "closed": "merged"
        }
    });
    write_record(&r1t2);

    // Run "discarded": one failed turn with no account; its copy was thrown away.
    let mut r2 = record(&project, "op-r2-t1", "run-discarded", 1, "pane-2", "failed", (T + 300 * S, T + 330 * S));
    r2["error"] = json!({ "code": "AGENT_FAILED", "message": "the agent stopped", "recoverable": false });
    r2["worker"] = json!({
        "worktree": {
            "path": format!("{project}/.puppet-master/worktrees/run-discarded"),
            "cwd": format!("{project}/.puppet-master/worktrees/run-discarded"),
            "branch": "pm/run-discarded", "base": "main", "base_commit": "def456", "repo": project,
            "handle": "run-discarded", "created_at_ms": T, "closed": "discarded"
        }
    });
    write_record(&r2);

    // Run "waiting": finished, copy still on disk, so it is not counted as merged or thrown away yet.
    let mut r3 = record(&project, "op-r3-t1", "run-waiting", 1, "pane-4", "completed", (T + 600 * S, T + 660 * S));
    r3["worker"] = json!({
        "account": "acct-work",
        "worktree": {
            "path": format!("{project}/.puppet-master/worktrees/run-waiting"),
            "cwd": format!("{project}/.puppet-master/worktrees/run-waiting"),
            "branch": "pm/run-waiting", "base": "main", "base_commit": "fed987", "repo": project,
            "handle": "run-waiting", "created_at_ms": T, "closed": null
        }
    });
    write_record(&r3);

    // A record that cannot be read is skipped and counted, not fatal.
    fs::write(ops_dir.join("damaged.json"), "{ not an operation").expect("damaged record");

    // The ring file: summaries of the four runs' turns, and one turn typed by hand (no record).
    let waiting = Approval {
        at_ms: T + 601 * S,
        prompt_id: "prompt-1".into(),
        prompt: "allow the test command?".into(),
        kind: "command".into(),
        decision: "allow".into(),
    };
    let seeded = vec![
        summary("pane-1", 1, (T + S, T + 62 * S), "idle", Some("acct-work"), files(2, 30, 4, "src/a.ts"), vec![]),
        summary("pane-1", 2, (T + 121 * S, T + 182 * S), "idle", Some("acct-work"), files(1, 5, 0, "src/b.ts"), vec![]),
        summary("pane-2", 1, (T + 301 * S, T + 332 * S), "error", None, files(1, 2, 2, "src/c.ts"), vec![]),
        summary("pane-4", 1, (T + 601 * S, T + 662 * S), "waiting_input", Some("acct-work"), files(1, 1, 0, "src/d.ts"), vec![waiting]),
        summary("pane-5", 1, (T + 900 * S, T + 960 * S), "idle", None, None, vec![]),
    ];
    let mut store = Store::default();
    for s in seeded {
        store.record(&folder, scrub(s)).expect("ring file");
    }
    assert!(ring_path(&folder).is_file(), "the ring file was written");

    // Read back the way `gather` reads a project: records (lenient) and its ring file.
    let (ops, unreadable) = list_operations_lenient(&project).expect("operation records");
    assert_eq!((ops.len(), unreadable), (4, 1), "four readable records, the damaged one counted");
    let summaries: Vec<ProjectSummary> =
        read_project(&folder).into_iter().map(|summary| ProjectSummary { project: project.clone(), summary }).collect();
    assert_eq!(summaries.len(), 5);
    let (turns, runs) = join(&ops, &summaries, &[], None, &no_label);
    assert_eq!(turns.len(), 5, "four turns with a record and one typed by hand");
    assert_eq!(runs.len(), 3, "three runs: merged, discarded, waiting");

    let mut project_names = BTreeMap::new();
    project_names.insert(comparable_project_path(&folder), project.clone());
    let inputs = Inputs { turns, runs, undo_since_ms: None, project_names, unreadable_records: unreadable };
    let now = T + HOUR;
    let report = build(&inputs, None, None, now, &labels());

    assert_eq!(report.scope, "all");
    assert_eq!(report.projects.len(), 1);
    assert_eq!(report.projects[0].turns, 5);
    assert_eq!(report.notes[0], "Counting Undo from now on.");
    assert!(report.notes.contains(&"1 turn had no file information, so it is left out of Undone and Asked you.".to_string()), "{:?}", report.notes);
    assert!(report.notes.contains(&"1 of 5 turns had checks. Checks count only when a run is started with them.".to_string()), "{:?}", report.notes);
    assert!(report.notes.contains(&"1 record could not be read. Its turns are not counted.".to_string()), "{:?}", report.notes);

    // Work account: three turns (two merged-run turns, one waiting run's turn). Two merged-run turns
    // ended OK, the waiting one too; its copy is waiting, so Merged is 1 of 1 (the merged run).
    let work = row(&report, "alpha", Some("acct-work"));
    assert_eq!(work.account_label, "Work");
    assert_eq!(work.turns, 3);
    assert_eq!((work.ended_ok.count, work.ended_ok.total), (3, 3));
    assert_eq!((work.checks_green.count, work.checks_green.total), (1, 1));
    assert_eq!((work.merged.count, work.merged.total), (1, 1), "the waiting copy is not counted");
    assert_eq!((work.asked_you.count, work.asked_you.total), (1, 3));
    assert_eq!(work.waiting_review, 1);
    assert!(work.per_merged.is_none(), "one merged run is too few for a typical change");
    assert!(!work.enough, "three turns is not enough to show a number");

    // No account: the discarded run's turn and the turn typed by hand.
    let none = row(&report, "alpha", None);
    assert_eq!(none.account_label, DEFAULT_ACCOUNT);
    assert_eq!(none.turns, 2);
    assert_eq!((none.ended_ok.count, none.ended_ok.total), (1, 2), "the failed turn is a failure");
    assert_eq!((none.merged.count, none.merged.total), (0, 1), "the one run was thrown away");
    assert_eq!((none.asked_you.count, none.asked_you.total), (0, 2));
    assert!(!none.enough);

    // A window that starts after every turn has nothing to show.
    let later = build(&inputs, None, Some(30), T + 40 * DAY, &labels());
    assert!(later.rows.is_empty(), "{:?}", later.rows);

    // The project by its folder path: the same numbers, and the folder name only.
    let one = build(&inputs, Some(project.as_str()), None, now, &labels());
    assert_eq!(one.scope, "project");
    assert_eq!(one.project.as_ref().map(|p| p.label.as_str()), folder.file_name().and_then(|n| n.to_str()));
    assert_eq!(one.rows, report.rows);

    // Nothing from the folder, the pane ids or the run handles leaves the page.
    let json = serde_json::to_string(&report).expect("report json");
    let escaped = project.replace('\\', "\\\\");
    for secret in [project.as_str(), escaped.as_str(), "pane-1", "pane-2", "run-merged", "run-discarded", "src/a.ts", "SECRET-"] {
        assert!(!json.contains(secret), "{secret} leaked into the report: {json}");
    }
}
