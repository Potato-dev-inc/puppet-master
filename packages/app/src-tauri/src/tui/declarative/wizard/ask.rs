//! The local drafter (SPEC 4.8): one read-only headless turn of an installed agent, with the wizard
//! session's work folder as its project (never the person's project) and the prompt on **standard
//! input**, so its size is not limited by the Windows command line (about 32,000 characters).
//!
//! The command comes from the agent's own profile (`headless_launch`, read-only policy included), built
//! with a marker where the task goes. The marker argument is then taken out and the prompt is written
//! to stdin instead: `claude -p` and `codex exec` both read their prompt from stdin when none is given
//! as an argument. An agent whose command line cannot be split that way is refused (`WIZARD_DRAFTER_NO_STDIN`).

use std::path::Path;
use std::time::Duration;

use crate::tui::types::{HeadlessLaunch, LaunchSpec};

use super::exec::{self, RunSpec};
use super::report::WizardError;
use super::session::{State, Wizard};

/// A prompt this long is a bug, not a probe that was a bit long (about 100 pages of text).
pub const MAX_PROMPT_CHARS: usize = 400_000;
/// The task slot of the profile's command line while it is built.
pub const MARKER: &str = "__PM_WIZARD_PROMPT__";
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;

/// The command line without the task, or `None` when the marker is not exactly one argument.
pub fn without_task(args: &[String]) -> Option<Vec<String>> {
    let mut hits = args.iter().enumerate().filter(|(_, a)| a.as_str() == MARKER);
    let (at, _) = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    let mut rest = args.to_vec();
    rest.remove(at);
    Some(rest)
}

/// Run the drafter: `build` makes the agent's command line for a launch, `extract` reads the answer
/// from its output. Returns the answer text.
pub fn run_local(
    wizard: &Wizard,
    session: &str,
    prompt: &str,
    build: &dyn Fn(&HeadlessLaunch) -> Result<LaunchSpec, String>,
    extract: &dyn Fn(&str, i32) -> Option<String>,
) -> Result<String, WizardError> {
    if prompt.trim().is_empty() {
        return Err(WizardError::bad_request("prompt is required"));
    }
    let chars = prompt.chars().count();
    if chars > MAX_PROMPT_CHARS {
        return Err(WizardError::new(
            400,
            "WIZARD_PROMPT_TOO_LONG",
            format!("the prompt is {chars} characters; the limit is {MAX_PROMPT_CHARS}"),
        ));
    }
    let snap = wizard.begin(session, "ask", &[State::Probed, State::Verified, State::Tested])?;
    let env = wizard.env();
    let result = ask_once(&snap.work_dir, &snap.cancel, env.limits.draft_timeout, prompt, build, extract);
    // A cancel that arrived meanwhile wins over whatever the child printed.
    wizard.end(session, |_| ())?;
    result
}

fn ask_once(
    work_dir: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    timeout: Duration,
    prompt: &str,
    build: &dyn Fn(&HeadlessLaunch) -> Result<LaunchSpec, String>,
    extract: &dyn Fn(&str, i32) -> Option<String>,
) -> Result<String, WizardError> {
    let project = work_dir.to_string_lossy().into_owned();
    // Read-only, no shell, no trust prompts: the drafter only writes text.
    let launch = HeadlessLaunch {
        project: &project,
        task: MARKER,
        read_only: true,
        resume_session: None,
        trust_workspace: false,
        allow_shell: false,
        model: None,
    };
    let spec = build(&launch).map_err(|e| WizardError::new(409, "WIZARD_DRAFTER_UNAVAILABLE", e))?;
    let args = without_task(&spec.args).ok_or_else(|| {
        WizardError::new(
            409,
            "WIZARD_DRAFTER_NO_STDIN",
            "this agent's command line cannot take the prompt on standard input; pick Claude Code, Codex or an OpenRouter model",
        )
    })?;
    let program = Path::new(&spec.program);
    let mut run = RunSpec::new(program, &args, work_dir);
    run.stdin = Some(prompt);
    run.env = &spec.env;
    run.env_remove = &spec.env_remove;
    run.timeout = timeout;
    run.max_bytes = MAX_ANSWER_BYTES;
    run.cancel = Some(cancel);
    let out = exec::run(&run);
    if out.cancelled {
        return Err(WizardError::new(409, "WIZARD_CANCELLED", "the wizard session was cancelled"));
    }
    if let Some(e) = &out.spawn_error {
        return Err(WizardError::new(502, "DRAFTER_FAILED", format!("the agent could not be started: {e}")));
    }
    if out.timed_out {
        return Err(WizardError::new(
            504,
            "DRAFTER_TIMEOUT",
            format!("the agent did not answer within {} seconds; try again or pick another writer", timeout.as_secs()),
        ));
    }
    let stdout = exec::plain(&out.stdout);
    let answer = extract(&stdout, out.exit.unwrap_or(-1)).map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    answer.ok_or_else(|| {
        let why = exec::clean(&out.stderr).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
        let code = out.exit.map_or_else(|| "no exit code".to_string(), |c| format!("exit {c}"));
        WizardError::new(502, "DRAFTER_EMPTY", format!("the agent gave no answer ({code}). {why}").trim().to_string())
    })
}

/// The installed agent `agent` as the drafter: it must be one that can run read-only and headless.
pub fn run_for_agent(wizard: &Wizard, session: &str, agent: &str, prompt: &str) -> Result<String, WizardError> {
    let id = agent.trim();
    if id.is_empty() {
        return Err(WizardError::bad_request("agent is required"));
    }
    let usable = crate::tui::read_only_backends().iter().any(|b| b.eq_ignore_ascii_case(id)) && crate::tui::is_agent_backend(id);
    if !usable {
        return Err(WizardError::bad_request(format!("`{id}` cannot write the profile: it has no read-only headless mode")));
    }
    let profile = crate::tui::profile_for(id);
    if !profile.is_installed() {
        return Err(WizardError::new(404, "BACKEND_UNAVAILABLE", format!("`{id}` was not found on PATH")));
    }
    let folder = wizard.read(session, |s| s.work_dir.to_string_lossy().into_owned())?;
    profile.spawn_gate(&folder).map_err(|e| WizardError::new(409, e.code, e.message))?;
    run_local(
        wizard,
        session,
        prompt,
        &|launch| crate::tui::headless_launch_with(&*profile, profile.id(), launch),
        &|output, code| profile.extract_result(output, code),
    )
}
