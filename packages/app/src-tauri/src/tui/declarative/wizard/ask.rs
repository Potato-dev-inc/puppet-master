//! The local drafter (SPEC 4.8): one read-only `quick_ask` of an installed agent, with the wizard
//! session's work folder as its project, never the person's project. This file only prepares the
//! request (pure, tested); the Tauri command in `commands/agent_wizard.rs` runs it.

use crate::agent_runs::quick::QuickAskRequest;

use super::report::WizardError;
use super::session::Wizard;

/// A Windows command line holds about 32,000 characters and both agents take the task as an argument.
pub const MAX_PROMPT_CHARS: usize = 30_000;
/// `quick_ask` itself clamps its wait at 120 seconds.
pub const WAIT_MS: u64 = 120_000;

pub fn prepare(wizard: &Wizard, session: &str, agent: &str, prompt: &str) -> Result<QuickAskRequest, WizardError> {
    if prompt.trim().is_empty() {
        return Err(WizardError::bad_request("prompt is required"));
    }
    let chars = prompt.chars().count();
    if chars > MAX_PROMPT_CHARS {
        return Err(WizardError::new(
            400,
            "WIZARD_PROMPT_TOO_LONG",
            format!("the prompt is {chars} characters; a local agent takes at most {MAX_PROMPT_CHARS}"),
        ));
    }
    let agent = agent.trim();
    if agent.is_empty() {
        return Err(WizardError::bad_request("agent is required"));
    }
    let folder = wizard.read(session, |s| s.work_dir.to_string_lossy().into_owned())?;
    Ok(QuickAskRequest {
        task: prompt.to_string(),
        project_path: Some(folder),
        backend: Some(agent.to_string()),
        model: None,
        wait_ms: Some(WAIT_MS),
        read_only: Some(true),
        idempotency_key: None,
    })
}
