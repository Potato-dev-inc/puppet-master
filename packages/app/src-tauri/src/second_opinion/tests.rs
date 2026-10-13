//! Unit tests: diff source selection, prompt assembly, findings parsing, the read-only
//! enforcement of the review run, follow-up routing. Real `git` only where a test says so.

use super::parse::parse_review;
use super::prompt::{build_prompt, PromptInput, FORMAT_RULES};
use super::route::{compose, decide, deliver, Route, Sender, Targets};
use super::service::{build_run_request, entries_with_same, poll_of};
use super::source::{build, cap_patch, count, git_diff_head, select, RawDiff, Sources, MAX_REVIEW_PATCH_BYTES};
use super::{Severity, SourceKind, VerdictLevel};
use crate::operations::OperationError;
use std::cell::RefCell;

// ---------------------------------------------------------------- fixtures

const PATCH: &str = "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,4 @@\n one\n+two\n three\n-four\n+five\n";

fn raw(patch: &str) -> Option<RawDiff> {
    Some(RawDiff { patch: patch.to_string(), truncated: false })
}

struct Fake {
    worktree: Option<RawDiff>,
    checkpoint: Option<RawDiff>,
    git: Option<RawDiff>,
    asked: RefCell<Vec<&'static str>>,
}

impl Fake {
    fn new(worktree: Option<RawDiff>, checkpoint: Option<RawDiff>, git: Option<RawDiff>) -> Self {
        Self { worktree, checkpoint, git, asked: RefCell::new(Vec::new()) }
    }
}

impl Sources for Fake {
    fn worktree(&self) -> Option<RawDiff> {
        self.asked.borrow_mut().push("worktree");
        self.worktree.clone()
    }
    fn checkpoint(&self) -> Option<RawDiff> {
        self.asked.borrow_mut().push("checkpoint");
        self.checkpoint.clone()
    }
    fn git(&self) -> Option<RawDiff> {
        self.asked.borrow_mut().push("git");
        self.git.clone()
    }
}

// ---------------------------------------------------------------- diff source

#[test]
fn the_worktree_diff_wins_and_later_sources_are_not_even_asked() {
    let fake = Fake::new(raw(PATCH), raw("diff --git a/x b/x\n"), raw("diff --git a/y b/y\n"));
    let diff = select(&fake).unwrap();
    assert_eq!(diff.source, SourceKind::Worktree);
    assert_eq!(diff.patch, PATCH);
    assert_eq!(fake.asked.borrow().as_slice(), ["worktree"]);
}

#[test]
fn the_checkpoint_diff_is_used_when_there_is_no_worktree_diff() {
    // None (not isolated) and an empty patch (isolated, nothing changed) both fall through.
    for worktree in [None, raw("  \n")] {
        let fake = Fake::new(worktree, raw(PATCH), raw("diff --git a/y b/y\n"));
        let diff = select(&fake).unwrap();
        assert_eq!(diff.source, SourceKind::Checkpoint);
        assert_eq!(fake.asked.borrow().as_slice(), ["worktree", "checkpoint"]);
    }
}

#[test]
fn plain_git_is_the_last_resort_and_nothing_at_all_is_an_error() {
    let diff = select(&Fake::new(None, None, raw(PATCH))).unwrap();
    assert_eq!(diff.source, SourceKind::Git);
    assert_eq!(diff.source.label(), "uncommitted changes in the project");
    let err = select(&Fake::new(None, raw(""), None)).unwrap_err();
    assert_eq!(err.code, "NOTHING_TO_REVIEW");
}

#[test]
fn the_selected_diff_is_counted() {
    let diff = select(&Fake::new(raw(PATCH), None, None)).unwrap();
    assert_eq!((diff.files, diff.additions, diff.deletions), (1, 2, 1));
    assert!(!diff.truncated);
}

#[test]
fn a_big_patch_is_cut_at_a_file_boundary_and_marked() {
    let file = |n: usize| format!("diff --git a/f{n} b/f{n}\n--- a/f{n}\n+++ b/f{n}\n@@ -1 +1 @@\n-old\n+{}\n", "x".repeat(300));
    let patch: String = (0..10).map(file).collect();
    let (cut, truncated) = cap_patch(&patch, 1_000);
    assert!(truncated);
    assert!(cut.len() <= 1_000);
    assert!(cut.ends_with('\n'));
    // Whole files only: every header kept is followed by its hunk.
    assert_eq!(cut.matches("diff --git").count(), cut.matches("@@ -1 +1 @@").count());
    let diff = build(SourceKind::Git, RawDiff { patch, truncated: false }, 1_000);
    assert!(diff.truncated);
    assert!(diff.files >= 1 && diff.files < 10);

    // One huge file is cut at a line end instead.
    let one = format!("diff --git a/big b/big\n@@ -0,0 +1,500 @@\n{}", "+line of code\n".repeat(500));
    let (cut, truncated) = cap_patch(&one, 500);
    assert!(truncated && cut.len() <= 500 && cut.ends_with('\n'));

    // A source that cut already keeps its flag.
    let small = build(SourceKind::Worktree, RawDiff { patch: PATCH.into(), truncated: true }, MAX_REVIEW_PATCH_BYTES);
    assert!(small.truncated);
}

#[test]
fn count_ignores_header_lines_and_binary_files() {
    let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/i.png b/i.png\nBinary files differ\n";
    assert_eq!(count(patch), (2, 1, 1));
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn git_diff_head_reads_uncommitted_changes_and_is_none_outside_a_repo() {
    let dir = std::env::temp_dir().join(format!("pm-so-git-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    // Not a repository (no .git above a fresh temp dir) or no commits: nothing to review.
    assert!(git_diff_head(&dir).is_none());
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    assert_eq!(git_diff_head(&dir).unwrap().patch, "", "clean tree: empty patch");
    std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let patch = git_diff_head(&dir).unwrap().patch;
    assert!(patch.contains("diff --git a/a.txt b/a.txt"));
    assert!(patch.contains("-two") && patch.contains("+TWO") && patch.contains("+three"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- prompt

fn diff_of(patch: &str, truncated: bool) -> super::ReviewDiff {
    build(SourceKind::Checkpoint, RawDiff { patch: patch.to_string(), truncated }, MAX_REVIEW_PATCH_BYTES)
}

#[test]
fn the_prompt_carries_task_facts_context_diff_and_the_answer_format() {
    let input = PromptInput {
        task: Some("Add retry to the fetch helper"),
        turn_summary: Some("Claude Code changed 1 file and ran 2 tests."),
        facts: vec![("Checks".into(), "2/2 passed".into()), ("Empty".into(), "  ".into())],
        context: Some("Use pnpm, not npm."),
        author: Some("Claude Code"),
    };
    let text = build_prompt(&input, &diff_of(PATCH, false));
    assert!(text.contains("Claude Code just made the change"));
    assert!(text.contains("read-only"));
    assert!(text.contains("## What the agent was asked to do\nAdd retry to the fetch helper"));
    assert!(text.contains("- Claude Code changed 1 file and ran 2 tests."));
    assert!(text.contains("- Checks: 2/2 passed"));
    assert!(!text.contains("Empty:"), "blank facts are dropped");
    assert!(text.contains("## Project context\nUse pnpm, not npm."));
    assert!(text.contains("1 file changed, +2 -1; from the last turn"));
    assert!(text.contains("```diff\ndiff --git a/src/a.ts b/src/a.ts"));
    assert!(text.ends_with(FORMAT_RULES));
    assert!(text.contains("FILE") || text.contains("path/to/file.ext:LINE | severity"));
    assert!(!text.contains("cut for size"));
}

#[test]
fn the_prompt_works_with_nothing_but_a_diff_and_says_when_it_was_cut() {
    let text = build_prompt(&PromptInput::default(), &diff_of(PATCH, true));
    assert!(text.contains("another coding agent just made"));
    assert!(!text.contains("## What the agent was asked"));
    assert!(!text.contains("## What we recorded"));
    assert!(!text.contains("## Project context"));
    assert!(text.contains("cut for size"));
}

#[test]
fn the_diff_fence_cannot_be_closed_by_the_diff_itself() {
    let patch = "diff --git a/R.md b/R.md\n@@ -0,0 +1,3 @@\n+```sh\n+ls\n+```\n";
    let text = build_prompt(&PromptInput::default(), &diff_of(patch, false));
    assert!(text.contains("````diff\n"), "fence is longer than the longest backtick run inside");
}

#[test]
fn a_long_task_is_clipped_so_the_diff_still_fits() {
    let task = "t".repeat(10_000);
    let text = build_prompt(&PromptInput { task: Some(&task), ..Default::default() }, &diff_of(PATCH, false));
    assert!(text.len() < 10_000);
    assert!(text.contains('…'));
}

// ---------------------------------------------------------------- parsing

#[test]
fn well_formed_lines_and_a_verdict() {
    let answer = "FINDINGS:\n\
src/a.ts:12 | high | Off by one: the loop skips the last item.\n\
src/b.rs:7-9 | medium | Error is swallowed.\n\
README.md | low | Typo in the title.\n\
src/c.ts:3 | info | Consider a test for this.\n\
VERDICT: changes | The off-by-one must be fixed first.\n";
    let review = parse_review(answer);
    assert_eq!(review.findings.len(), 4);
    let f = &review.findings[0];
    assert_eq!((f.id.as_str(), f.file.as_str(), f.line, f.severity), ("f1", "src/a.ts", Some(12), Severity::High));
    assert_eq!(f.text, "Off by one: the loop skips the last item.");
    let g = &review.findings[1];
    assert_eq!((g.line, g.end_line, g.severity), (Some(7), Some(9), Severity::Medium));
    assert_eq!(review.findings[2].line, None, "a file-level finding has no line");
    assert_eq!(review.findings[3].severity, Severity::Info);
    let verdict = review.verdict.unwrap();
    assert_eq!(verdict.level, VerdictLevel::Changes);
    assert_eq!(verdict.text, "The off-by-one must be fixed first.");
    assert!(review.unstructured.is_none());
}

#[test]
fn agents_that_drift_from_the_format_are_still_read() {
    let answer = "Here is my review:\n\n\
```\n\
1. `src/a.ts:12` | **High** | Null is not checked.\n\
```\n\
- src/b.ts#L30 | Warning | Unused import.\n\
| src/c.ts:4 | nit | Rename this. |\n\
| --- | --- | --- |\n\
* C:\\work\\app\\d.ts:8 | critical | Path traversal.\n\
**Verdict:** approve - ship it after the first one.\n";
    let review = parse_review(answer);
    let got: Vec<(&str, Option<u32>, Severity)> = review.findings.iter().map(|f| (f.file.as_str(), f.line, f.severity)).collect();
    assert_eq!(
        got,
        vec![
            ("src/a.ts", Some(12), Severity::High),
            ("src/b.ts", Some(30), Severity::Medium),
            ("src/c.ts", Some(4), Severity::Low),
            ("C:\\work\\app\\d.ts", Some(8), Severity::High),
        ]
    );
    let verdict = review.verdict.unwrap();
    assert_eq!(verdict.level, VerdictLevel::Approve);
    assert_eq!(verdict.text, "ship it after the first one.");
}

#[test]
fn a_json_block_is_the_fallback() {
    let answer = "Sure.\n```json\n{\"findings\":[{\"file\":\"src/a.ts\",\"line\":\"L12\",\"severity\":\"major\",\"finding\":\"Leaks the handle.\"},\
{\"path\":\"b.rs\",\"line\":3,\"end_line\":5,\"message\":\"Racy.\"},{\"file\":\"\",\"finding\":\"dropped\"}],\
\"verdict\":{\"level\":\"concerns\",\"text\":\"Two real issues.\"}}\n```\n";
    let review = parse_review(answer);
    assert_eq!(review.findings.len(), 2);
    assert_eq!(review.findings[0].line, Some(12));
    assert_eq!(review.findings[0].severity, Severity::High);
    assert_eq!(review.findings[1].severity, Severity::Medium, "missing severity defaults to medium");
    assert_eq!(review.findings[1].end_line, Some(5));
    let verdict = review.verdict.unwrap();
    assert_eq!((verdict.level, verdict.text.as_str()), (VerdictLevel::Concerns, "Two real issues."));

    // Bare JSON array, verdict as a string.
    let review = parse_review("[{\"file\":\"x.rs\",\"line\":1,\"severity\":\"low\",\"text\":\"t\"}]");
    assert_eq!(review.findings.len(), 1);
    let review = parse_review("{\"findings\":[],\"verdict\":\"approve: all good\"}");
    assert!(review.findings.is_empty());
    assert_eq!(review.verdict.unwrap().level, VerdictLevel::Approve);
    assert!(review.unstructured.is_none());
}

#[test]
fn garbage_is_tolerated_and_kept_as_plain_text() {
    let review = parse_review("I looked at it and it seems fine to me, though I did not run it.");
    assert!(review.findings.is_empty() && review.verdict.is_none());
    assert_eq!(review.unstructured.as_deref(), Some("I looked at it and it seems fine to me, though I did not run it."));

    // Broken lines among good ones: only the good one counts.
    let review = parse_review("a | b\n | | |\nsrc/a.ts:x | high | no line number\nsrc/ok.ts:5 | high | real one\nverdicts are great\n:::|:::|:::\n");
    assert_eq!(review.findings.len(), 2, "{review:?}");
    assert_eq!(review.findings[1].file, "src/ok.ts");
    assert!(review.verdict.is_none(), "a sentence that starts with 'verdicts' is not a verdict");

    // Broken JSON, empty input.
    assert!(parse_review("{\"findings\": [").unstructured.is_some());
    let empty = parse_review("   \n");
    assert!(empty.findings.is_empty() && empty.verdict.is_none() && empty.unstructured.is_none());
}

#[test]
fn findings_none_with_an_approve_verdict_is_a_clean_review() {
    let review = parse_review("FINDINGS: none\nVERDICT: approve | Looks correct and tested.");
    assert!(review.findings.is_empty());
    assert_eq!(review.verdict.as_ref().unwrap().level, VerdictLevel::Approve);
    assert!(review.unstructured.is_none());
}

#[test]
fn findings_are_capped_and_long_text_is_cut() {
    let many: String = (1..=150).map(|n| format!("a.ts:{n} | low | note {n}\n")).collect();
    assert_eq!(parse_review(&many).findings.len(), 100);
    let long = format!("a.ts:1 | low | {}", "w".repeat(2_000));
    let text = parse_review(&long).findings[0].text.clone();
    assert!(text.chars().count() <= 600 && text.ends_with('…'));
}

// ---------------------------------------------------------------- read-only enforcement

#[test]
fn the_review_run_is_headless_read_only_without_a_shell_or_trust() {
    let reviewer = crate::tui::read_only_backends()[0];
    let run = build_run_request(reviewer, "/tmp/p", "review this", &|_| true).unwrap();
    assert_eq!(run.agent_type, reviewer);
    assert!(run.headless && run.read_only && run.background);
    assert!(!run.allow_shell && !run.trust_workspace && !run.keep_pane);
    assert_eq!(run.wait_ms, 0);
    assert_eq!(run.context_policy, crate::operations::ContextPolicy::Fresh);
    assert_eq!(run.task, "review this");
    assert_eq!(run.project_path.as_deref(), Some("/tmp/p"));
    assert!(run.idempotency_key.is_some());
}

#[test]
fn an_agent_that_cannot_enforce_read_only_is_refused_as_reviewer() {
    for bad in ["opencode", "powershell", "nonsense", ""] {
        let err = build_run_request(bad, "/tmp/p", "x", &|_| true).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGUMENT", "{bad:?}");
    }
    let reviewer = crate::tui::read_only_backends()[0];
    let err = build_run_request(reviewer, "/tmp/p", "x", &|_| false).unwrap_err();
    assert_eq!(err.code, "BACKEND_UNAVAILABLE");
}

#[test]
fn every_offered_reviewer_is_a_read_only_backend_and_the_authors_agent_is_marked() {
    let ids: Vec<String> = crate::tui::read_only_backends().iter().map(|s| s.to_string()).collect();
    let entries = ids
        .iter()
        .map(|id| crate::askbar::AgentEntry { id: id.clone(), label: id.clone(), ready: false })
        .collect::<Vec<_>>();
    let marked = entries_with_same(entries, Some(&ids[0].to_uppercase()));
    assert_eq!(marked.iter().filter(|e| e.same).count(), 1);
    assert!(marked[0].same);
    let none = entries_with_same(
        vec![crate::askbar::AgentEntry { id: ids[0].clone(), label: "x".into(), ready: true }],
        None,
    );
    assert!(!none[0].same && none[0].ready);
}

// ---------------------------------------------------------------- poll

fn snapshot(status: crate::operations::OperationStatus, result: Option<&str>) -> crate::operations::OperationSnapshot {
    use crate::operations::{OperationSnapshot, StateSource};
    OperationSnapshot {
        operation_id: "op-1".into(),
        runtime_id: "rt".into(),
        request_fingerprint: String::new(),
        project_path: "/tmp".into(),
        task: "review".into(),
        agent_type: "codex".into(),
        pane_id: None,
        pane_created: false,
        idempotency_key: "key".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        status,
        revision: 2,
        created_at_ms: 0,
        updated_at_ms: 0,
        observed_at_ms: None,
        pane_state: None,
        required_action: None,
        source: StateSource::Native,
        stage: None,
        progress_pct: None,
        result: result.map(str::to_string),
        error: None,
        timeout_ms: 600_000,
        read_only: true,
        keep_pane: false,
        placement: None,
        keep_pane_requested: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: "h1".into(),
        turn_index: 0,
        verified: false,
        started_at_ms: None,
        finished_at_ms: Some(1),
        message_baseline_ids: vec![],
        output_baseline: None,
        result_capture: None,
        acceptance_status: Default::default(),
        worker: Default::default(),
        checks: Vec::new(),
        check_results: Vec::new(),
        warnings: Vec::new(),
    }
}

#[test]
fn polling_maps_a_run_to_running_done_or_failed() {
    use crate::operations::OperationStatus as S;
    assert_eq!(poll_of(&snapshot(S::Running, None), "Codex").state, "running");
    assert_eq!(poll_of(&snapshot(S::Queued, None), "Codex").state, "running");
    let done = poll_of(&snapshot(S::Completed, Some("a.ts:1 | high | bad\nVERDICT: changes | no")), "Codex");
    assert_eq!(done.state, "done");
    assert_eq!(done.review.unwrap().findings.len(), 1);
    let empty = poll_of(&snapshot(S::Completed, Some("  ")), "Codex");
    assert_eq!((empty.state, empty.error.as_deref()), ("failed", Some("Codex gave an empty answer.")));
    let failed = poll_of(&snapshot(S::Failed, None), "Codex");
    assert_eq!(failed.state, "failed");
    assert!(failed.error.is_some());
    assert_eq!(poll_of(&snapshot(S::WaitingInput, None), "Codex").state, "needs_input");
}

// ---------------------------------------------------------------- follow-up routing

struct World {
    run: Option<(String, String)>,
    pane: bool,
    sent: RefCell<Vec<String>>,
    fail_pane: bool,
}

impl World {
    fn new(run: Option<(&str, &str)>, pane: bool) -> Self {
        Self { run: run.map(|(p, h)| (p.into(), h.into())), pane, sent: RefCell::new(Vec::new()), fail_pane: false }
    }
}

impl Targets for World {
    fn run_of_pane(&self, _: &str) -> Option<(String, String)> {
        self.run.clone()
    }
    fn pane_exists(&self, _: &str) -> bool {
        self.pane
    }
}

impl Sender for World {
    fn followup(&self, project: &str, handle: &str, task: &str) -> Result<(), OperationError> {
        self.sent.borrow_mut().push(format!("followup {project} {handle} :: {task}"));
        Ok(())
    }
    fn pane_input(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if self.fail_pane {
            return Err("pane has exited".into());
        }
        self.sent.borrow_mut().push(format!("input {pane_id} :: {text}"));
        Ok(())
    }
}

#[test]
fn a_comment_is_one_plain_line_with_file_and_line() {
    assert_eq!(
        compose("src/a.ts", Some(12), "Null is not\nchecked here.\n"),
        "Review comment on src/a.ts:12: Null is not checked here. Please address it."
    );
    assert_eq!(compose("README.md", None, "Fix the title!"), "Review comment on README.md: Fix the title. Please address it.");
}

#[test]
fn a_pane_with_a_run_handle_gets_a_followup_on_that_handle() {
    let world = World::new(Some(("/proj", "h-42")), true);
    assert_eq!(decide(&world, "p1").unwrap(), Route::Run { project: "/proj".into(), handle: "h-42".into() });
    let sent = deliver(&world, &world, "p1", "src/a.ts", Some(3), "Handle the error").unwrap();
    assert_eq!((sent.route, sent.target.as_str()), ("run", "h-42"));
    assert_eq!(
        world.sent.borrow().as_slice(),
        ["followup /proj h-42 :: Review comment on src/a.ts:3: Handle the error. Please address it."]
    );
}

#[test]
fn a_hand_opened_pane_gets_the_comment_typed_into_it() {
    let world = World::new(None, true);
    assert_eq!(decide(&world, "p9").unwrap(), Route::Pane { pane_id: "p9".into() });
    let sent = deliver(&world, &world, "p9", "a.rs", Some(1), "Rename it").unwrap();
    assert_eq!((sent.route, sent.target.as_str()), ("pane", "p9"));
    assert_eq!(world.sent.borrow().as_slice(), ["input p9 :: Review comment on a.rs:1: Rename it. Please address it."]);
}

#[test]
fn a_closed_pane_empty_text_and_a_dead_pane_are_clear_errors() {
    let gone = World::new(None, false);
    assert_eq!(deliver(&gone, &gone, "p", "a", None, "x").unwrap_err().code, "AGENT_GONE");
    let world = World::new(None, true);
    assert_eq!(deliver(&world, &world, "p", "a", None, "  ").unwrap_err().code, "INVALID_ARGUMENT");
    assert!(world.sent.borrow().is_empty());
    let mut dead = World::new(None, true);
    dead.fail_pane = true;
    assert_eq!(deliver(&dead, &dead, "p", "a", None, "x").unwrap_err().code, "SEND_FAILED");
}
