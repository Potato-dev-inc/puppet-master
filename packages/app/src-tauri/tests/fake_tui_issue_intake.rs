//! "Assign an issue" end to end (zero tokens, no network, no GitHub): the real intake engine
//! (`issue_intake::live`, started by `IssueRig`), the real question store, real git, real worktrees, a real
//! ConPTY pane with `pm-fake-tui` as the agent.
//!
//! What is fake, and nothing else:
//! - GitHub: a `gh.cmd` first on `PATH` (and `PUPPET_MASTER_GH`) that answers `auth status`, `api user`, the
//!   issues, events and single-issue paths from files, `repo view`, `pr view` (none) and `pr create` (prints a
//!   URL, keeps the body file, logs every call to `calls.log`). Same idea as `worktrees/tests_ship.rs`.
//! - The remote: a local bare repository behind `url.<file:///...>.insteadOf = https://github.com/acme/<repo>.git`,
//!   so the project's `origin` is the plain GitHub URL (`project_key_of` reads `acme/<repo>`) and every
//!   fetch, `ls-remote` and push lands in the bare repo.
//! - The agent: a script (`write_file`, `run`, `say`).
//!
//! Fixture names are `acme`, `you`, `someone` only. Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{IssueRig, Session, World};
use puppet_master_app_lib::questions::{now_ms, routes, store, NewQuestion};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

// ---- helpers -----------------------------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").output().expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A scratch folder of the session for the bare repo, the fake gh and the agent's output files. Not
/// `session.aux()`: `aux` is a reserved device name on Windows, and git and cmd.exe cannot use a path through it.
fn scratch(session: &Session) -> PathBuf {
    let dir = session.project().parent().expect("the session folder").join("issue-scratch");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|event| event["ev"] == name).collect()
}

fn text_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

/// The process environment this test changes (the fake `gh` first on `PATH`, a fake service token, a fake
/// `GH_TOKEN`), put back when the scenario ends.
struct EnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn new() -> Self {
        let names = ["PATH", "PUPPET_MASTER_GH", "GH_TOKEN", "PM_E2E_SERVICE_TOKEN", "PM_E2E_SERVICE_API_KEY"];
        Self { saved: names.iter().map(|name| (*name, std::env::var_os(name))).collect() }
    }

    fn use_gh(&self, dir: &Path) {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(real) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&real));
        }
        std::env::set_var("PATH", std::env::join_paths(paths).expect("a PATH"));
        std::env::set_var("PUPPET_MASTER_GH", dir.join("gh.cmd"));
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, old) in &self.saved {
            match old {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

// ---- the fake gh -------------------------------------------------------------------------------------------

/// A folder with a fake `gh.cmd` and the files it answers from. Every call is logged to `calls.log`.
struct FakeGh {
    dir: PathBuf,
}

const GH_SCRIPT: &str = r#"@echo off
setlocal
echo %* >> "%~dp0calls.log"
set "D=%~dp0"
if "%~1"=="auth" exit /b 0
if "%~1"=="repo" goto repo
if "%~1"=="pr" goto pr
if "%~1"=="api" goto api
exit /b 2
:repo
type "%D%repo.json"
exit /b 0
:pr
if "%~2"=="view" goto prview
if "%~2"=="create" goto prcreate
exit /b 2
:prview
echo no pull requests found 1>&2
exit /b 1
:prcreate
if "%~1"=="" goto created
if "%~1"=="--body-file" copy /Y "%~2" "%D%body.txt" >nul
shift
goto prcreate
:created
type "%D%pr_url.txt"
exit /b 0
:api
set "LAST="
:scan
if "%~1"=="" goto dispatch
set "LAST=%~1"
shift
goto scan
:dispatch
echo "%LAST%" | findstr /C:"/events" >nul && goto events
echo "%LAST%" | findstr /C:"/issues?" >nul && goto issues
echo "%LAST%" | findstr /C:"/issues/" >nul && goto issue
if /i "%LAST%"=="user" goto user
exit /b 1
:events
type "%D%http200.txt"
type "%D%events.json"
exit /b 0
:issues
type "%D%http200.txt"
type "%D%issues.json"
exit /b 0
:issue
type "%D%http200.txt"
type "%D%issue.json"
exit /b 0
:user
type "%D%http200.txt"
type "%D%user.json"
exit /b 0
"#;

impl FakeGh {
    /// One open issue `number` that carries the label `puppet`, written by `you`.
    fn new(dir: &Path, name: &str, default_branch: &str, number: u64, title: &str, body: &str) -> Self {
        std::fs::create_dir_all(dir).expect("gh dir");
        let gh = FakeGh { dir: dir.to_path_buf() };
        std::fs::write(dir.join("gh.cmd"), GH_SCRIPT.replace('\n', "\r\n")).expect("gh.cmd");
        gh.put("http200.txt", "HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n");
        gh.put("user.json", r#"{"login":"you","id":10001,"type":"User"}"#);
        gh.put(
            "repo.json",
            &json!({
                "url": format!("https://github.com/acme/{name}"),
                "nameWithOwner": format!("acme/{name}"),
                "defaultBranchRef": { "name": default_branch },
            })
            .to_string(),
        );
        gh.put("pr_url.txt", &format!("https://github.com/acme/{name}/pull/7\r\n"));
        let issue = json!({
            "url": format!("https://api.github.com/repos/acme/{name}/issues/{number}"),
            "html_url": format!("https://github.com/acme/{name}/issues/{number}"),
            "number": number,
            "title": title,
            "user": { "login": "you", "id": 10001, "type": "User" },
            "labels": [{ "name": "puppet" }],
            "state": "open",
            "locked": false,
            "author_association": "OWNER",
            "body": body,
            "created_at": "2026-10-26T08:00:00Z",
            "updated_at": "2026-10-27T09:00:00Z",
            "comments": 0
        });
        gh.put("issue.json", &issue.to_string());
        gh.put("issues.json", &json!([issue]).to_string());
        gh.put("events.json", "[]");
        gh
    }

    fn put(&self, name: &str, text: &str) {
        std::fs::write(self.dir.join(name), text).expect("write a fake gh file");
    }

    /// `who` adds the label now (after the repo was added in Settings, so it counts).
    fn label_by(&self, who: &str, event_id: u64) {
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.put(
            "events.json",
            &json!([{
                "id": event_id,
                "event": "labeled",
                "actor": { "login": who, "id": 10002, "type": "User" },
                "label": { "name": "puppet", "color": "ededed" },
                "created_at": at,
            }])
            .to_string(),
        );
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default()
    }

    fn body(&self) -> String {
        std::fs::read_to_string(self.dir.join("body.txt")).unwrap_or_default()
    }
}

// ---- the repository -----------------------------------------------------------------------------------------

/// The session's project as a clone of `https://github.com/acme/<name>.git` whose remote is a local bare repo.
struct Repo {
    name: String,
    project: PathBuf,
    bare: PathBuf,
    default_branch: String,
}

impl Repo {
    fn new(session: &Session, name: &str) -> Repo {
        let project = session.project().to_path_buf();
        let bare = scratch(session).join(format!("{name}.git"));
        git(&scratch(session), &["init", "-q", "--bare", &bare.to_string_lossy()]);
        let default_branch = git(&project, &["rev-parse", "--abbrev-ref", "HEAD"]);
        git(&bare, &["symbolic-ref", "HEAD", &format!("refs/heads/{default_branch}")]);
        let url = format!("https://github.com/acme/{name}.git");
        git(&project, &["config", "user.name", "you"]);
        git(&project, &["config", "user.email", "you@example.invalid"]);
        git(&project, &["config", "commit.gpgsign", "false"]);
        git(&project, &["config", "core.autocrlf", "false"]);
        git(&project, &["remote", "add", "origin", &url]);
        git(&project, &["config", &format!("url.file:///{}.insteadOf", slashes(&bare)), &url]);
        git(&project, &["push", "-q", "origin", &format!("HEAD:refs/heads/{default_branch}")]);
        Repo { name: name.to_string(), project, bare, default_branch }
    }

    fn owner_repo(&self) -> String {
        format!("acme/{}", self.name)
    }

    /// Every branch of the bare remote, sorted.
    fn remote_heads(&self) -> Vec<String> {
        let list = git(&self.bare, &["for-each-ref", "--format=%(refname)=%(objectname)", "refs/heads"]);
        let mut heads: Vec<String> = list.lines().map(str::to_string).collect();
        heads.sort();
        heads
    }

    fn pm_branches(&self, number: u64) -> Vec<String> {
        let prefix = format!("refs/heads/pm/issue-{number}-");
        self.remote_heads().into_iter().filter(|line| line.starts_with(&prefix)).collect()
    }
}

/// The saved settings for one watched repo (what `issue_intake_save` stores), `fake_claude` as the agent.
fn config_for(repo: &Repo, index: u32, ask_before_push: bool) -> Value {
    json!({
        "enabled": true,
        "allowed": [],
        "write_access_counts": false,
        "parallel": 1,
        "repos": [{
            "id": format!("rw-e2e0000{index}"),
            "owner_repo": repo.owner_repo(),
            "folder": repo.project.to_string_lossy(),
            "label": "puppet",
            "agent": { "agent_type": "fake_claude", "account": null },
            "turns": 5,
            "minutes": 10,
            "runs_per_day": 5,
            "ask_before_start": true,
            "ask_before_push": ask_before_push,
            // Labels count only when added after this.
            "added_at_ms": now_ms().saturating_sub(5 * 60 * 1000),
            "paused": false,
        }],
        "ignored": [],
    })
}

// ---- what the test looks at ----------------------------------------------------------------------------------

fn run_of(rig: &IssueRig, number: u64) -> Option<Value> {
    rig.runs().into_iter().find(|run| run["issue"]["number"] == number)
}

fn state_of(rig: &IssueRig, number: u64) -> String {
    run_of(rig, number).map(|run| text_of(&run["state"], "state")).unwrap_or_default()
}

/// A pending card of `kind` that is about issue `number`.
fn card_of(kind: &str, number: u64) -> Option<Value> {
    let mark = format!("#{number}");
    store()
        .list_pending(now_ms())
        .into_iter()
        .filter(|q| q.kind.as_deref() == Some(kind) && q.question.contains(&mark))
        .filter_map(|q| serde_json::to_value(q).ok())
        .next()
}

fn choice_ids(card: &Value) -> Vec<String> {
    card["choices"].as_array().map(|list| list.iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The desktop's answer to a card (what the Inbox button sends through the Tauri command).
fn answer_as_desktop(card: &Value, choice: &str) {
    let id = card["id"].as_str().expect("a card id");
    store().answer(id, Some(choice), None, "desktop", now_ms()).unwrap_or_else(|e| panic!("answer {choice} refused: {}", e.message));
}

/// Looks at GitHub (every 1.5 s) until `check` says yes; panics with the runs and the fake's log when not.
fn drive<T>(session: &Session, rig: &IssueRig, secs: u64, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut last_poll: Option<Instant> = None;
    loop {
        if let Some(found) = check() {
            return found;
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for: {what}\n-- runs: {}\n-- pending cards: {}\n-- fake events: {:#?}",
                serde_json::to_string_pretty(&rig.runs()).unwrap_or_default(),
                serde_json::to_string_pretty(&store().list_pending(now_ms())).unwrap_or_default(),
                session.events()
            );
        }
        if last_poll.is_none_or(|at| at.elapsed() > Duration::from_millis(1500)) {
            rig.poll_now();
            last_poll = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A scenario: a repo, the fake gh with one issue, the settings saved, the engine running.
struct Scenario {
    repo: Repo,
    gh: FakeGh,
    rig: IssueRig,
    number: u64,
    _env: EnvGuard,
}

impl Scenario {
    /// `labeled_by` put the label on the issue before the repo is watched (after it was added, as far as
    /// the baseline is concerned).
    fn new(session: &Session, name: &str, index: u32, number: u64, title: &str, labeled_by: &str, ask_before_push: bool) -> Scenario {
        let rig = IssueRig::start();
        // The engine of the scenario before this one may be in the middle of a look at its own repo:
        // switch it off and let that look end before the fake gh answers something else.
        rig.save_config(json!({ "enabled": false })).expect("switch the watcher off");
        std::thread::sleep(Duration::from_millis(2500));
        let env = EnvGuard::new();
        let repo = Repo::new(session, name);
        let gh = FakeGh::new(&scratch(session).join("fake-gh"), name, &repo.default_branch, number, title, "Clicking the login button does nothing.");
        env.use_gh(&gh.dir);
        gh.label_by(labeled_by, 9000 + u64::from(index));
        rig.save_config(config_for(&repo, index, ask_before_push)).expect("save the settings");
        Scenario { repo, gh, rig, number, _env: env }
    }

    fn wait_for_start_card(&self, session: &Session) -> Value {
        drive(session, &self.rig, 90, "the Start card", || card_of("issue_start", self.number))
    }

    fn wait_for_state(&self, session: &Session, state: &str, secs: u64) -> Value {
        drive(session, &self.rig, secs, &format!("run state {state}"), || {
            run_of(&self.rig, self.number).filter(|run| run["state"]["state"] == state)
        })
    }
}

/// The fake agent's script. `steps` run in order; the agent always ends with a short reply.
fn agent_script(steps: &[String]) -> String {
    format!("agent = \"claude\"\nboot_ms = 200\n[[turn]]\nsteps = [\n  {{ do = \"think\", ms = 300 }},\n{}\n  {{ do = \"say\", text = \"Done.\" }},\n]\n", {
        steps.iter().map(|s| format!("  {s},")).collect::<Vec<_>>().join("\n")
    })
}

/// A `run` step (TOML literal string: no escapes, so the command must not contain a single quote).
fn run_step(command: &str) -> String {
    format!("{{ do = \"run\", command = '{command}' }}")
}

fn write_step(path: &str, content: &str) -> String {
    format!("{{ do = \"write_file\", path = \"{path}\", content = \"{content}\" }}")
}

// ---- 1. labeled by you: card, start, a guarded pane, push card, draft PR --------------------------------------

/// The whole story. Labeled by you -> the Start card (nothing runs yet) -> Start -> a pane in a separate copy
/// under `.puppet-master/worktrees/`, a short task that does not carry the issue, no `GH_TOKEN` in its
/// environment even though this process has one, and a `git push` from the pane that fails -> the agent
/// finishes -> the push card -> Open draft PR -> the branch `pm/issue-42-*` is on the bare remote, the fake
/// gh was asked for `pr create --draft`, and the body starts `Refs #42`.
#[test]
fn issue_intake_labeled_by_you_runs_in_a_guarded_copy_and_opens_a_draft_pr() {
    let session = World::get().session("issue-intake-happy");
    let s = Scenario::new(&session, "demo", 1, 42, "Login button does nothing", "you", true);
    // A token in the process that starts the panes: the guard must keep it out of the agent.
    std::env::set_var("GH_TOKEN", "ghp_e2e_fixture_not_a_real_token");
    std::env::set_var("PM_E2E_SERVICE_TOKEN", "service-token-fixture");
    std::env::set_var("PM_E2E_SERVICE_API_KEY", "service-key-fixture");
    let out = scratch(&session);
    let (cwd_file, env_file, push_file) = (out.join("cwd.txt"), out.join("env.txt"), out.join("push.txt"));
    session.script(&agent_script(&[
        run_step(&format!("cd > \"{}\"", cwd_file.display())),
        run_step(&format!("set > \"{}\"", env_file.display())),
        write_step("src/login.txt", "fixed\\n"),
        run_step("git add src/login.txt && git commit -q -m \"Fix the login button\""),
        run_step(&format!("git push origin HEAD > \"{}\" 2>&1", push_file.display())),
    ]));

    // Labeled by you after the repo was added: a card, and nothing else yet.
    let card = s.wait_for_start_card(&session);
    assert!(text_of(&card, "question").starts_with("Start on #42"), "{card}");
    assert_eq!(choice_ids(&card), ["start", "not_now", "ignore"], "{card}");
    assert_eq!(state_of(&s.rig, 42), "asking");
    assert!(events_named(&session, "prompt").is_empty(), "the agent started before anyone said yes");
    assert!(!s.gh.calls().contains("pr create"));

    // Start, as the desktop.
    answer_as_desktop(&card, "start");
    let running = s.wait_for_state(&session, "running", 90);
    assert_eq!(running["approved_by"], "desktop", "{running}");
    let branch = text_of(&running, "branch");
    assert!(branch.starts_with("pm/issue-42-"), "{branch}");

    // The pane got the short task, not the issue.
    let prompt = drive(&session, &s.rig, 60, "the task typed into the pane", || {
        events_named(&session, "prompt").first().and_then(|e| e["text"].as_str().map(str::to_string))
    });
    assert!(prompt.starts_with("Work on GitHub issue #42"), "{prompt}");
    assert_eq!(session.pane_ids().len(), 1, "one visible pane: {:?}", session.pane_ids());
    assert!(!prompt.contains("Clicking the login button"), "the issue text is in the prompt: {prompt}");

    // The agent finishes; the push card is raised (the repo asks before each push).
    let push_card = drive(&session, &s.rig, 120, "the push card", || card_of("issue_push", 42));
    assert_eq!(text_of(&push_card, "question"), "Draft PR ready for #42?", "{push_card}");
    assert_eq!(choice_ids(&push_card), ["push", "look", "throw_away"], "{push_card}");
    assert_eq!(state_of(&s.rig, 42), "ask_push");

    // Where the agent worked, and what its environment looked like.
    let cwd = std::fs::read_to_string(&cwd_file).expect("the pane's working folder").trim().replace('\\', "/");
    assert!(cwd.contains("/.puppet-master/worktrees/"), "the pane is not in a separate copy: {cwd}");
    let env = std::fs::read_to_string(&env_file).expect("the pane's environment");
    let has = |name: &str| env.lines().any(|line| line.to_ascii_uppercase().starts_with(&format!("{name}=")));
    assert!(has("PATH"), "the dump is not an environment: {env}");
    assert!(has("GH_CONFIG_DIR"), "GH_CONFIG_DIR is missing in the pane");
    assert!(!has("GH_TOKEN"), "GH_TOKEN reached the agent");
    assert!(!has("PM_E2E_SERVICE_TOKEN"), "a *_TOKEN reached the agent");
    assert!(has("PM_E2E_SERVICE_API_KEY"), "an agent's own *_API_KEY must be kept");
    let config_dir = env.lines().find(|l| l.to_ascii_uppercase().starts_with("GH_CONFIG_DIR=")).unwrap_or_default();
    assert!(config_dir.contains("gh-empty"), "gh would see a login: {config_dir}");

    // The agent's own push failed; nothing is on the remote and the fake gh was not asked for a PR yet.
    let tried = std::fs::read_to_string(&push_file).unwrap_or_default();
    assert!(!tried.trim().is_empty(), "the agent's `git push` printed nothing, so it did not run");
    assert!(s.repo.pm_branches(42).is_empty(), "the agent pushed: {:?}", s.repo.remote_heads());
    assert!(!s.gh.calls().contains("pr create"), "{}", s.gh.calls());

    // Open draft PR.
    answer_as_desktop(&push_card, "push");
    let open = s.wait_for_state(&session, "pr_open", 60);
    assert_eq!(open["state"]["number"], 7, "{open}");
    assert_eq!(open["state"]["url"], "https://github.com/acme/demo/pull/7", "{open}");
    let pushed = s.repo.pm_branches(42);
    assert_eq!(pushed.len(), 1, "{:?}", s.repo.remote_heads());
    assert!(pushed[0].starts_with(&format!("refs/heads/{branch}=")), "{pushed:?} vs {branch}");
    let calls = s.gh.calls();
    assert!(calls.contains("pr create --draft"), "{calls}");
    assert!(!calls.contains("--force") && !calls.contains("pr merge") && !calls.contains("pr ready"), "{calls}");
    let body = s.gh.body();
    assert!(body.trim_start().starts_with("Refs #42"), "{body}");
    assert!(!body.contains("Clicking the login button"), "the issue text is in the PR body: {body}");
    // The agent's work is in the pushed commit.
    let shown = git(&s.repo.bare, &["show", &format!("refs/heads/{branch}:src/login.txt")]);
    assert_eq!(shown.trim(), "fixed");
}

// ---- 2. labeled by an outsider: nothing ----------------------------------------------------------------------

/// `someone` (not you, not on the list) adds the label. No card, no run, no pane; the record says Skipped.
#[test]
fn issue_intake_labeled_by_an_outsider_raises_no_card_and_starts_nothing() {
    let session = World::get().session("issue-intake-outsider");
    let s = Scenario::new(&session, "outsider", 2, 44, "Export button is missing", "someone", true);
    session.script(&agent_script(&[write_step("never.txt", "no\\n")]));

    let skipped = s.wait_for_state(&session, "skipped", 60);
    assert!(text_of(&skipped["state"], "why").contains("someone"), "the row should say who: {skipped}");
    // A few more looks: still nothing.
    for _ in 0..3 {
        s.rig.poll_now();
        std::thread::sleep(Duration::from_millis(1500));
    }
    assert!(card_of("issue_start", 44).is_none(), "an outsider's label raised a card");
    assert!(card_of("issue_push", 44).is_none());
    assert_eq!(s.rig.runs().iter().filter(|run| run["issue"]["number"] == 44).count(), 1, "one record, not one per look");
    assert!(events_named(&session, "prompt").is_empty(), "an agent started");
    assert!(session.pane_ids().is_empty(), "a pane was opened: {:?}", session.pane_ids());
    assert!(s.repo.pm_branches(44).is_empty());
    assert!(!s.gh.calls().contains("pr create"));
}

// ---- 3. a workflow file: the card says why, no push until answered ----------------------------------------------

/// The agent edits `.github/workflows/x.yml`. The repo does NOT ask before pushing, but the CI file forces the
/// card, with its reason. Nothing is pushed while the card is open; the person says yes and then it is.
#[test]
fn issue_intake_a_workflow_edit_forces_the_push_card_and_nothing_is_pushed_before_the_answer() {
    let session = World::get().session("issue-intake-ci");
    let s = Scenario::new(&session, "ci", 3, 43, "Speed up the build", "you", false);
    session.script(&agent_script(&[
        write_step(".github/workflows/x.yml", "name: x\\n"),
        write_step("notes.txt", "build\\n"),
        run_step("git add notes.txt .github/workflows/x.yml && git commit -q -m \"Change the build\""),
    ]));
    let start = s.wait_for_start_card(&session);
    answer_as_desktop(&start, "start");

    let card = drive(&session, &s.rig, 120, "the push card", || card_of("issue_push", 43));
    assert_eq!(text_of(&card, "question"), "Draft PR ready for #43?", "{card}");
    let context = text_of(&card, "context");
    assert!(context.contains("Changes CI or workflow files: .github/workflows/x.yml."), "the CI reason is missing: {context}");
    assert!(choice_ids(&card).contains(&"push".to_string()), "{card}");
    assert!(!context.contains("You asked Puppet Master to check each push first."), "the repo does not ask: {context}");

    // It waits for the person.
    std::thread::sleep(Duration::from_secs(4));
    assert!(s.repo.pm_branches(43).is_empty(), "pushed before the answer: {:?}", s.repo.remote_heads());
    assert!(!s.gh.calls().contains("pr create"), "{}", s.gh.calls());
    assert_eq!(state_of(&s.rig, 43), "ask_push");

    answer_as_desktop(&card, "push");
    s.wait_for_state(&session, "pr_open", 60);
    assert_eq!(s.repo.pm_branches(43).len(), 1, "{:?}", s.repo.remote_heads());
    assert!(s.gh.calls().contains("pr create --draft"));
}

// ---- 4. git settings changed by the agent: held ---------------------------------------------------------------

/// The agent sets `core.fsmonitor` in the repository's git config (what a planted program would use to run as
/// the person during PM's own git calls). Puppet Master notices, holds the run (no push button at all), and
/// the bare remote is exactly as it was.
#[test]
fn issue_intake_a_git_config_change_holds_the_run_and_the_remote_is_untouched() {
    let session = World::get().session("issue-intake-held");
    let s = Scenario::new(&session, "held", 4, 45, "Tidy the readme", "you", false);
    session.script(&agent_script(&[
        write_step("README.md", "# tidy\\n"),
        run_step("git config core.fsmonitor pm-e2e-not-a-program"),
        run_step("git add README.md && git commit -q -m \"Tidy the readme\""),
    ]));
    let before = s.repo.remote_heads();
    let start = s.wait_for_start_card(&session);
    answer_as_desktop(&start, "start");

    let held = s.wait_for_state(&session, "held", 120);
    assert_eq!(held["state"]["code"], "ISSUE_GIT_CHANGED", "{held}");
    // The card has no way to push.
    let card = drive(&session, &s.rig, 30, "the hold card", || card_of("issue_push", 45));
    assert_eq!(choice_ids(&card), ["look", "throw_away"], "{card}");
    assert!(text_of(&card, "context").contains("Puppet Master won't push this."), "{card}");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(s.repo.remote_heads(), before, "the remote changed");
    assert!(s.repo.pm_branches(45).is_empty());
    let calls = s.gh.calls();
    assert!(!calls.contains("pr create"), "{calls}");
    // The run is still held, not pushed, after more looks.
    s.rig.poll_now();
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(state_of(&s.rig, 45), "held");
}

// ---- 5. only the person answers --------------------------------------------------------------------------------

/// A plain loopback answer or dismiss of the Start card is refused (`ISSUE_NEEDS_PERSON`); the card stays and
/// nothing starts. A question an agent makes up with the same choice ids is not a run's card: answering it
/// starts nothing either. The desktop's own answer still works.
#[test]
fn issue_intake_a_loopback_answer_is_refused_and_a_look_alike_card_starts_nothing() {
    let session = World::get().session("issue-intake-loopback");
    let s = Scenario::new(&session, "loop", 5, 46, "Rename a setting", "you", true);
    session.script(&agent_script(&[write_step("never.txt", "no\\n")]));
    let card = s.wait_for_start_card(&session);
    let id = text_of(&card, "id");

    // Any local program (an agent's shell included) can reach loopback; it may not answer this card.
    let path = ["questions", id.as_str(), "answer"];
    let refused = routes::handle("POST", &path, &json!({ "choice_id": "start" }), None, false, &|| {})
        .expect("a questions route")
        .expect_err("a loopback answer of an issue card");
    assert_eq!(refused.0, 403, "{refused:?}");
    assert_eq!(refused.1["code"], "ISSUE_NEEDS_PERSON", "{refused:?}");
    let dismissed = routes::handle("POST", &["questions", id.as_str(), "dismiss"], &json!({}), None, false, &|| {})
        .expect("a questions route")
        .expect_err("a loopback dismiss of an issue card");
    assert_eq!(dismissed.1["code"], "ISSUE_NEEDS_PERSON", "{dismissed:?}");
    assert!(card_of("issue_start", 46).is_some(), "the card is gone");
    assert_eq!(state_of(&s.rig, 46), "asking");

    // A card made up by someone else, with the same choice ids and the same words, is no run's card.
    let fake = store()
        .create(
            NewQuestion {
                question: "Start on #46 \"Rename a setting\" with Fake?".to_string(),
                choices: vec![
                    puppet_master_app_lib::questions::Choice { id: "start".into(), label: "Start".into() },
                    puppet_master_app_lib::questions::Choice { id: "not_now".into(), label: "Not now".into() },
                ],
                ..NewQuestion::default()
            },
            now_ms(),
        )
        .expect("a plain question");
    store().answer(&fake.id, Some("start"), None, "desktop", now_ms()).expect("answered");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(state_of(&s.rig, 46), "asking", "a look-alike card moved the run");
    assert!(events_named(&session, "prompt").is_empty(), "an agent started");
    assert!(session.pane_ids().is_empty(), "a pane was opened");

    // The person's own answer works and leaves nothing running.
    let card = card_of("issue_start", 46).expect("the card is still up");
    answer_as_desktop(&card, "not_now");
    s.wait_for_state(&session, "declined", 30);
    assert!(session.pane_ids().is_empty());
}
