//! Night Shift: Keep and Throw away (SPEC n32 3.7). Only the person does this, from the desktop
//! (the Tauri commands in `commands/night.rs`). No MCP tool, bridge route or companion path reaches
//! this file (S2).
//!
//! Keep merges a finished task's copy into the project's current branch with the same merge the
//! copies use everywhere (`worktrees::service::merge`). A conflict is aborted there, so the project is
//! unchanged and the copy stays; the task then shows why. Throw away deletes every copy the task made,
//! earlier tries too, and only when nothing is still running.
//!
//! The git side is behind [`CopyOps`], so the tests run without a repository.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};

use super::model::{Class, NightRun, Review, Task};
use super::store::Store;
use crate::audit;
use crate::operations::OperationError;
use crate::pty::PaneRegistry;
use crate::worktrees::{service, MergeOutcome};

/// The pane registry the worktree service reads (the same one the Tauri state holds).
pub type Registry = Arc<Mutex<PaneRegistry>>;

/// What worktrees says when a copy is already gone (merged or thrown away before).
const COPY_GONE: &str = "WORKTREE_NOT_FOUND";
/// What worktrees says when a merge conflicts. The merge is aborted and the project is unchanged.
const CONFLICT: &str = "MERGE_CONFLICT";
/// How many file names a conflict sentence names before it stops.
const NAMED_FILES: usize = 3;

/// The result of Keep for one task, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepView {
    pub branch: String,
    pub into: String,
    pub files: Vec<String>,
    pub merged: bool,
    pub reason: Option<String>,
}

/// The git side of a task's copies and the audit log. The live one calls `worktrees::service` and
/// `audit`; the tests use a fake.
pub trait CopyOps {
    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError>;
    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError>;
    fn audit(&self, summary: &str, detail: Value);
}

/// The desktop's implementation.
pub struct Live<'a> {
    pub registry: &'a Registry,
}

impl CopyOps for Live<'_> {
    fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
        service::merge(Some(project), handle, self.registry)
    }

    fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError> {
        service::discard(Some(project), handle, self.registry)
    }

    fn audit(&self, summary: &str, detail: Value) {
        audit::record(audit::Kind::NightShift, audit::Who::Desktop, summary, detail);
    }
}

/// Keep one finished task: merge its copy into the project's current branch. The commands pass the
/// live store and [`Live`]; the tests pass a temp store and a fake. `who` is the caller's name for the
/// audit line.
pub fn keep_with(
    store: &Store,
    ops: &dyn CopyOps,
    task_id: &str,
    who: &str,
    now: u64,
) -> Result<KeepView, OperationError> {
    let mut run = load(store)?;
    let index = task_index(&run, task_id)?;
    refuse_open(&run.tasks[index])?;
    match run.tasks[index].review {
        Review::Pending | Review::KeepFailed { .. } => {}
        Review::Kept { .. } => return Err(err("ALREADY_KEPT", "This task is already kept.")),
        Review::Discarded { .. } => return Err(err("ALREADY_THROWN_AWAY", "This task was thrown away.")),
    }
    let task = &run.tasks[index];
    let outcome = task
        .outcome
        .as_ref()
        .ok_or_else(|| err("NO_RESULT", "This task has no result to keep yet."))?;
    if !matches!(outcome.class, Class::Ready | Class::NoChecks | Class::NeedsLook) {
        return Err(err(
            "NOTHING_TO_KEEP",
            "There is nothing to keep. Throw it away instead.",
        ));
    }
    let copy = outcome
        .copy
        .clone()
        .ok_or_else(|| err("NO_COPY", "The copy for this task is gone."))?;
    let project = task.spec.project.clone();
    let spec_id = task.spec.id.clone();

    match ops.merge(&project, &copy.handle) {
        Ok(out) if out.merged => {
            ops.audit(
                "Kept a Night Shift task",
                json!({ "task": spec_id, "branch": out.branch, "into": out.into, "by": who }),
            );
            run.tasks[index].review = Review::Kept {
                at_ms: now,
                into: out.into.clone(),
            };
            save(store, &mut run)?;
            Ok(KeepView {
                branch: out.branch,
                into: out.into,
                files: out.files,
                merged: true,
                reason: None,
            })
        }
        Ok(out) => Ok(KeepView {
            branch: out.branch,
            into: out.into,
            files: out.files,
            merged: false,
            reason: Some("This copy has no changes to keep.".to_string()),
        }),
        Err(error) if error.code == CONFLICT => {
            let files = context_list(&error.context, "files");
            let into = context_text(&error.context, "into");
            let sentence = conflict_sentence(&files, &copy.branch);
            run.tasks[index].review = Review::KeepFailed {
                at_ms: now,
                sentence: sentence.clone(),
            };
            save(store, &mut run)?;
            Ok(KeepView {
                branch: copy.branch,
                into,
                files,
                merged: false,
                reason: Some(sentence),
            })
        }
        Err(error) => Err(error),
    }
}

/// Throw away one task. Deletes the copy of every try (the first try's copy too). Nothing is merged.
pub fn throw_away_with(
    store: &Store,
    ops: &dyn CopyOps,
    task_id: &str,
    who: &str,
    now: u64,
) -> Result<(), OperationError> {
    let mut run = load(store)?;
    let index = task_index(&run, task_id)?;
    refuse_open(&run.tasks[index])?;
    match run.tasks[index].review {
        Review::Pending | Review::KeepFailed { .. } => {}
        Review::Kept { .. } => {
            return Err(err(
                "ALREADY_KEPT",
                "This task is already kept, so its copy was merged.",
            ))
        }
        Review::Discarded { .. } => return Ok(()),
    }
    let project = run.tasks[index].spec.project.clone();
    let mut handles: Vec<String> = Vec::new();
    for handle in run.tasks[index].attempts.iter().filter_map(|a| a.handle.clone()) {
        if !handles.contains(&handle) {
            handles.push(handle);
        }
    }
    for handle in &handles {
        match ops.discard(&project, handle) {
            Ok(()) => {}
            Err(error) if error.code == COPY_GONE => {}
            Err(error) => return Err(error),
        }
    }
    ops.audit(
        "Threw away a Night Shift task",
        json!({ "task": run.tasks[index].spec.id, "copies": handles.len(), "by": who }),
    );
    run.tasks[index].review = Review::Discarded { at_ms: now };
    save(store, &mut run)
}

/// Keep every ready task that has no review yet, in list order, stopping at the first one that does
/// not merge. Each result is returned with its task id.
pub fn keep_all_ready_with(
    store: &Store,
    ops: &dyn CopyOps,
    who: &str,
    now: u64,
) -> Vec<(String, Result<KeepView, OperationError>)> {
    let Some(run) = store.load() else { return Vec::new() };
    let ready: Vec<String> = run
        .tasks
        .iter()
        .filter(|task| {
            task.review == Review::Pending
                && task.outcome.as_ref().is_some_and(|o| o.class == Class::Ready)
                && task.attempts.iter().all(|a| a.end.is_some())
        })
        .map(|task| task.spec.id.clone())
        .collect();
    let mut done = Vec::new();
    for id in ready {
        let result = keep_with(store, ops, &id, who, now);
        let stop = !matches!(&result, Ok(view) if view.merged);
        done.push((id, result));
        if stop {
            break;
        }
    }
    done
}

/// The sentence a conflicting task shows on its row (SPEC 3.7). Names up to `NAMED_FILES` files.
pub fn conflict_sentence(files: &[String], branch: &str) -> String {
    let shown: Vec<&str> = files.iter().take(NAMED_FILES).map(String::as_str).collect();
    let more = files.len() - shown.len();
    let names = match (shown.is_empty(), more) {
        (true, _) => String::new(),
        (false, 0) => format!(" ({})", shown.join(", ")),
        (false, n) => format!(" ({} and {n} more)", shown.join(", ")),
    };
    format!("This conflicts with a task you already kept{names}. Throw it away, or merge it yourself from its branch {branch}.")
}

fn load(store: &Store) -> Result<NightRun, OperationError> {
    store
        .load()
        .ok_or_else(|| err("NO_NIGHT", "There is no Night Shift to look at."))
}

fn save(store: &Store, run: &mut NightRun) -> Result<(), OperationError> {
    store.save(run).map_err(|message| err("SAVE_FAILED", message))
}

fn task_index(run: &NightRun, task_id: &str) -> Result<usize, OperationError> {
    run.tasks
        .iter()
        .position(|task| task.spec.id == task_id)
        .ok_or_else(|| err("NO_SUCH_TASK", "That task is not in tonight's run."))
}

/// A task with a try still open is running: it is kept or thrown away when it ends.
fn refuse_open(task: &Task) -> Result<(), OperationError> {
    if task.attempts.iter().any(|a| a.end.is_none()) {
        return Err(err(
            "TASK_ACTIVE",
            "This task is still running. Keep it or throw it away when it finishes.",
        ));
    }
    Ok(())
}

fn context_list(context: &Value, key: &str) -> Vec<String> {
    context
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn context_text(context: &Value, key: &str) -> String {
    context.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn err(code: &str, message: impl Into<String>) -> OperationError {
    OperationError::new(code, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::missions::model::AgentChoice;
    use crate::night_shift::model::{
        sample_run, sample_task, Attempt, AttemptEnd, AttemptKind, CopyRef, Outcome, RunState, TaskState,
    };
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;

    const RUN: &str = "ns-0123456789ab";
    const SECRET: &str = "SECRET_TASK_TEXT_DO_NOT_LEAK";

    /// A temp folder for one test, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            Scratch(std::env::temp_dir().join(format!("pm-ns-review-{}", uuid::Uuid::new_v4().simple())))
        }

        fn store(&self) -> Store {
            Store::at(self.0.join("store"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The git side, scripted: some handles conflict, some are already gone. Calls are recorded.
    #[derive(Default)]
    struct Fake {
        conflicts: Vec<String>,
        gone: Vec<String>,
        merges: RefCell<Vec<String>>,
        discards: RefCell<Vec<String>>,
        audits: RefCell<Vec<(String, Value)>>,
    }

    impl CopyOps for Fake {
        fn merge(&self, _project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
            self.merges.borrow_mut().push(handle.to_string());
            if self.conflicts.iter().any(|h| h == handle) {
                let mut error = OperationError::new(CONFLICT, "conflict", false);
                error.context = json!({ "files": ["src/cart.ts"], "branch": format!("pm/{handle}"), "into": "main" });
                return Err(error);
            }
            Ok(MergeOutcome {
                merged: true,
                reason: None,
                merge_commit: Some("abc123".into()),
                files: vec!["src/cart.ts".into()],
                branch: format!("pm/{handle}"),
                into: "main".into(),
            })
        }

        fn discard(&self, _project: &str, handle: &str) -> Result<(), OperationError> {
            self.discards.borrow_mut().push(handle.to_string());
            if self.gone.iter().any(|h| h == handle) {
                return Err(OperationError::new(COPY_GONE, "gone", false));
            }
            Ok(())
        }

        fn audit(&self, summary: &str, detail: Value) {
            self.audits.borrow_mut().push((summary.to_string(), detail));
        }
    }

    fn attempt(n: u8, handle: &str, ended: bool) -> Attempt {
        Attempt {
            n,
            plan: AgentChoice {
                agent_type: "fake_a".into(),
                account: None,
                model: None,
            },
            label: "Fake A".into(),
            kind: AttemptKind::Single,
            mission_id: None,
            handle: Some(handle.into()),
            pane_ids: Vec::new(),
            started_ms: 10,
            ended_ms: ended.then_some(20),
            end: ended.then_some(AttemptEnd::Completed),
        }
    }

    /// A finished task whose copies are `handles`, one per try; the last one holds the result.
    fn finished(id: &str, class: Class, handles: &[&str]) -> Task {
        let mut task = sample_task(id);
        task.spec.text = format!("{SECRET} for {id}");
        task.state = TaskState::Finished;
        task.attempts = handles
            .iter()
            .enumerate()
            .map(|(i, h)| attempt(i as u8 + 1, h, true))
            .collect();
        let last = handles.last().copied().unwrap_or("none");
        task.outcome = Some(Outcome {
            class,
            headline: "Tests pass".into(),
            reason: None,
            tests: None,
            tests_before: None,
            checks_declared: true,
            test_change_lines: Vec::new(),
            files: Some((1, 2, 0)),
            turns: 1,
            minutes: 5,
            summaries: Vec::new(),
            copy: Some(CopyRef {
                project: "/work/app".into(),
                handle: last.into(),
                branch: format!("pm/{last}"),
                base: "main".into(),
                pane_id: None,
            }),
        });
        task
    }

    fn saved(scratch: &Scratch, tasks: Vec<Task>) -> Store {
        let store = scratch.store();
        let mut run = sample_run(RUN);
        run.state = RunState::Done;
        run.tasks = tasks;
        store.save(&mut run).expect("save");
        store
    }

    fn review_of(store: &Store, id: &str) -> Review {
        store
            .load()
            .expect("run")
            .tasks
            .into_iter()
            .find(|t| t.spec.id == id)
            .expect("task")
            .review
    }

    #[test]
    fn keep_merges_the_copy_and_records_it_kept() {
        let scratch = Scratch::new();
        let store = saved(&scratch, vec![finished("nt-00000001", Class::Ready, &["mission-a"])]);
        let fake = Fake::default();
        let view = keep_with(&store, &fake, "nt-00000001", "you", 500).expect("keep");
        assert!(view.merged && view.reason.is_none());
        assert_eq!(view.into, "main");
        assert_eq!(view.files, vec!["src/cart.ts".to_string()]);
        assert_eq!(
            review_of(&store, "nt-00000001"),
            Review::Kept {
                at_ms: 500,
                into: "main".into()
            }
        );
        assert_eq!(fake.merges.borrow().as_slice(), ["mission-a"]);
        let audits = fake.audits.borrow();
        assert_eq!(audits[0].0, "Kept a Night Shift task");
    }

    #[test]
    fn keep_refuses_while_the_task_is_still_running() {
        let scratch = Scratch::new();
        let mut task = finished("nt-00000001", Class::Ready, &["mission-a"]);
        task.attempts[0].end = None;
        task.attempts[0].ended_ms = None;
        let store = saved(&scratch, vec![task]);
        let fake = Fake::default();
        let error = keep_with(&store, &fake, "nt-00000001", "you", 1).expect_err("refused");
        assert_eq!(error.code, "TASK_ACTIVE");
        assert!(fake.merges.borrow().is_empty());
        assert_eq!(review_of(&store, "nt-00000001"), Review::Pending);
    }

    #[test]
    fn a_conflict_keeps_the_copy_and_says_why() {
        let scratch = Scratch::new();
        let store = saved(&scratch, vec![finished("nt-00000001", Class::Ready, &["mission-a"])]);
        let fake = Fake {
            conflicts: vec!["mission-a".into()],
            ..Fake::default()
        };
        let view = keep_with(&store, &fake, "nt-00000001", "you", 9).expect("a conflict is a state, not an error");
        assert!(!view.merged);
        assert_eq!(view.branch, "pm/mission-a");
        assert_eq!(view.into, "main");
        let reason = view.reason.expect("reason");
        assert_eq!(
            reason,
            "This conflicts with a task you already kept (src/cart.ts). Throw it away, or merge it yourself from its branch pm/mission-a."
        );
        assert_eq!(
            review_of(&store, "nt-00000001"),
            Review::KeepFailed {
                at_ms: 9,
                sentence: reason
            }
        );
        assert!(fake.discards.borrow().is_empty(), "the copy stays");
    }

    #[test]
    fn keep_all_ready_stops_at_the_first_conflict() {
        let scratch = Scratch::new();
        let store = saved(
            &scratch,
            vec![
                finished("nt-00000001", Class::Ready, &["mission-a"]),
                finished("nt-00000002", Class::NeedsLook, &["mission-n"]),
                finished("nt-00000003", Class::Ready, &["mission-b"]),
                finished("nt-00000004", Class::Ready, &["mission-c"]),
            ],
        );
        let fake = Fake {
            conflicts: vec!["mission-b".into()],
            ..Fake::default()
        };
        let done = keep_all_ready_with(&store, &fake, "you", 3);
        let ids: Vec<&str> = done.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            ["nt-00000001", "nt-00000003"],
            "needs-a-look is skipped, the conflict stops the rest"
        );
        assert!(done[0].1.as_ref().is_ok_and(|v| v.merged));
        assert!(!done[1].1.as_ref().is_ok_and(|v| v.merged));
        assert_eq!(
            review_of(&store, "nt-00000001"),
            Review::Kept {
                at_ms: 3,
                into: "main".into()
            }
        );
        assert!(matches!(review_of(&store, "nt-00000003"), Review::KeepFailed { .. }));
        assert_eq!(review_of(&store, "nt-00000004"), Review::Pending);
        assert_eq!(fake.merges.borrow().as_slice(), ["mission-a", "mission-b"]);
    }

    #[test]
    fn keep_refuses_a_task_with_nothing_to_keep() {
        let scratch = Scratch::new();
        let mut queued = sample_task("nt-00000002");
        queued.state = TaskState::Queued;
        let mut didnt_run = finished("nt-00000001", Class::DidntRun, &[]);
        didnt_run.outcome.as_mut().expect("outcome").copy = None;
        let store = saved(&scratch, vec![didnt_run, queued]);
        let fake = Fake::default();
        assert_eq!(
            keep_with(&store, &fake, "nt-00000001", "you", 1)
                .expect_err("no copy")
                .code,
            "NOTHING_TO_KEEP"
        );
        assert_eq!(
            keep_with(&store, &fake, "nt-00000002", "you", 1)
                .expect_err("no result")
                .code,
            "NO_RESULT"
        );
        assert_eq!(
            keep_with(&store, &fake, "nt-00000009", "you", 1)
                .expect_err("no task")
                .code,
            "NO_SUCH_TASK"
        );
        assert!(fake.merges.borrow().is_empty());
    }

    #[test]
    fn throw_away_removes_every_try_copy() {
        let scratch = Scratch::new();
        let store = saved(
            &scratch,
            vec![finished("nt-00000001", Class::NeedsLook, &["mission-a1", "mission-a2"])],
        );
        let fake = Fake::default();
        throw_away_with(&store, &fake, "nt-00000001", "you", 7).expect("throw away");
        assert_eq!(fake.discards.borrow().as_slice(), ["mission-a1", "mission-a2"]);
        assert_eq!(review_of(&store, "nt-00000001"), Review::Discarded { at_ms: 7 });
        assert_eq!(fake.audits.borrow()[0].0, "Threw away a Night Shift task");
    }

    #[test]
    fn throw_away_ignores_a_copy_that_is_already_gone() {
        let scratch = Scratch::new();
        let store = saved(
            &scratch,
            vec![finished("nt-00000001", Class::Ready, &["mission-a1", "mission-a2"])],
        );
        let fake = Fake {
            gone: vec!["mission-a1".into()],
            ..Fake::default()
        };
        throw_away_with(&store, &fake, "nt-00000001", "you", 7).expect("still thrown away");
        assert_eq!(review_of(&store, "nt-00000001"), Review::Discarded { at_ms: 7 });
    }

    #[test]
    fn throw_away_refuses_a_running_or_kept_task() {
        let scratch = Scratch::new();
        let mut running = finished("nt-00000001", Class::Ready, &["mission-a"]);
        running.attempts[0].end = None;
        let mut kept = finished("nt-00000002", Class::Ready, &["mission-b"]);
        kept.review = Review::Kept {
            at_ms: 1,
            into: "main".into(),
        };
        let store = saved(&scratch, vec![running, kept]);
        let fake = Fake::default();
        assert_eq!(
            throw_away_with(&store, &fake, "nt-00000001", "you", 2)
                .expect_err("running")
                .code,
            "TASK_ACTIVE"
        );
        assert_eq!(
            throw_away_with(&store, &fake, "nt-00000002", "you", 2)
                .expect_err("kept")
                .code,
            "ALREADY_KEPT"
        );
        assert!(fake.discards.borrow().is_empty());
    }

    #[test]
    fn audits_carry_no_task_text() {
        let scratch = Scratch::new();
        let store = saved(
            &scratch,
            vec![
                finished("nt-00000001", Class::Ready, &["mission-a"]),
                finished("nt-00000002", Class::Ready, &["mission-b"]),
            ],
        );
        let fake = Fake::default();
        keep_with(&store, &fake, "nt-00000001", "you", 1).expect("keep");
        throw_away_with(&store, &fake, "nt-00000002", "you", 2).expect("throw away");
        let log = format!("{:?}", fake.audits.borrow());
        assert!(!log.contains(SECRET), "task text must not reach the audit log");
        assert!(log.contains("nt-00000001"), "the task id is enough");
    }

    #[test]
    fn no_night_means_a_plain_refusal() {
        let scratch = Scratch::new();
        let fake = Fake::default();
        assert_eq!(
            keep_with(&scratch.store(), &fake, "nt-00000001", "you", 1)
                .expect_err("none")
                .code,
            "NO_NIGHT"
        );
        assert!(keep_all_ready_with(&scratch.store(), &fake, "you", 1).is_empty());
    }

    /// The real git side: copies made with `worktrees::create` on a temp repository, merged with
    /// `merge_keep`. The conflict is git's own, not a script.
    struct GitOps;

    impl CopyOps for GitOps {
        fn merge(&self, project: &str, handle: &str) -> Result<MergeOutcome, OperationError> {
            let copy = crate::worktrees::find_by_handle(project, handle)
                .ok_or_else(|| OperationError::new(COPY_GONE, "gone", false))?;
            crate::worktrees::merge_keep(&copy)
        }

        fn discard(&self, project: &str, handle: &str) -> Result<(), OperationError> {
            match crate::worktrees::find_by_handle(project, handle) {
                Some(copy) => crate::worktrees::discard(&copy),
                None => Ok(()),
            }
        }

        fn audit(&self, _summary: &str, _detail: Value) {}
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = crate::proc_util::hidden_command("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_real_git_conflict_keeps_the_second_copy_and_names_the_file() {
        let scratch = Scratch::new();
        let project = scratch.0.join("app");
        fs::create_dir_all(&project).expect("project");
        git(&project, &["init", "-q"]);
        git(&project, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(&project, &["config", "user.email", "night@example.com"]);
        git(&project, &["config", "user.name", "Night Test"]);
        fs::write(project.join("cart.ts"), "total = 1\n").expect("file");
        git(&project, &["add", "-A"]);
        git(&project, &["commit", "-q", "-m", "base"]);
        let project_text = project.to_string_lossy().into_owned();
        let copy_a = crate::worktrees::create(&project_text, "mission-a").expect("copy a");
        let copy_b = crate::worktrees::create(&project_text, "mission-b").expect("copy b");
        fs::write(std::path::Path::new(&copy_a.path).join("cart.ts"), "total = 2\n").expect("edit a");
        fs::write(std::path::Path::new(&copy_b.path).join("cart.ts"), "total = 3\n").expect("edit b");

        let mut first = finished("nt-00000001", Class::Ready, &["mission-a"]);
        let mut second = finished("nt-00000002", Class::Ready, &["mission-b"]);
        for (task, copy) in [(&mut first, &copy_a), (&mut second, &copy_b)] {
            task.spec.project = project_text.clone();
            let outcome = task.outcome.as_mut().expect("outcome");
            let reference = outcome.copy.as_mut().expect("copy");
            reference.branch = copy.branch.clone();
        }
        let store = saved(&scratch, vec![first, second]);

        let kept = keep_with(&store, &GitOps, "nt-00000001", "you", 1).expect("first keep");
        assert!(kept.merged, "the first copy merges");
        let conflict = keep_with(&store, &GitOps, "nt-00000002", "you", 2).expect("a conflict is a state");
        assert!(!conflict.merged);
        assert_eq!(conflict.files, vec!["cart.ts".to_string()]);
        let reason = conflict.reason.expect("reason");
        assert!(reason.contains("(cart.ts)"), "{reason}");
        assert!(reason.contains(&copy_b.branch), "{reason}");
        assert!(matches!(review_of(&store, "nt-00000002"), Review::KeepFailed { .. }));
        assert!(
            crate::worktrees::find_by_handle(&project_text, "mission-b").is_some(),
            "the copy stays"
        );
        // Windows may write CRLF, so compare the words only.
        let origin = fs::read_to_string(project.join("cart.ts")).expect("read");
        assert_eq!(origin.trim(), "total = 2", "the project is unchanged");
    }

    #[test]
    fn the_conflict_sentence_names_up_to_three_files() {
        let two = conflict_sentence(&["a.ts".into(), "b.ts".into()], "pm/x");
        assert!(two.ends_with("(a.ts, b.ts). Throw it away, or merge it yourself from its branch pm/x."));
        let five: Vec<String> = (1..=5).map(|i| format!("f{i}.ts")).collect();
        let text = conflict_sentence(&five, "pm/x");
        assert!(text.contains("(f1.ts, f2.ts, f3.ts and 2 more)"), "{text}");
        assert!(conflict_sentence(&[], "pm/x").contains("already kept. Throw it away"));
    }
}
