//! The ACP bridge: a small ACP adapter that Puppet Master makes for an agent that has no ACP at all.
//!
//! The app executable itself is the adapter: `<current exe> acp-bridge --agent <id>` speaks ACP (JSON-RPC
//! lines) on stdio and drives the agent's own non-interactive mode, the one `run_agent` uses for headless
//! runs. Nothing is reimplemented: the command line comes from the agent's profile
//! (`TuiProfile::headless_command`, code profile or declarative `[headless]` table), the process starts through
//! `proc_util::hidden_command` (`agent_runs::headless::headless_command`), is contained like every adapter
//! (`acp::job`), and the output is read with the profile's own extractors (`extract_result`,
//! `extract_provider_session`, `headless_result_ready`). Generic code here names no agent.
//!
//! # What v1 does
//!
//! * `initialize` (protocol 1; no `loadSession`; text prompts only; `agentInfo.name` = the agent's label).
//!   An agent without a headless mode is refused with a JSON-RPC error and the bridge exits.
//! * `session/new` (the folder comes from the request), `session/prompt` (text blocks only), `session/cancel`.
//! * A turn runs the headless command once, streams what the agent prints as `agent_message_chunk` as it
//!   arrives and ends with `end_turn`. Structured output (json / jsonl) streams as far as the definition's
//!   result selector can give the text before the run ends; otherwise the final text is sent once.
//! * Cancel kills the whole process tree (`stopReason: cancelled`). A non-zero exit, a timeout, or output the
//!   definition cannot read ends the turn with a JSON-RPC error carrying one plain sentence (the tail of the
//!   agent's stderr goes through `redact::text`).
//! * Memory: when the profile can resume a headless run by session id (`features().resumable_headless`) and the
//!   output carries that id, turn 2 resumes turn 1. Otherwise every turn starts fresh and the first turn says
//!   so once, in a message chunk.
//!
//! # What v1 does not do (said honestly in the docs and in the chat)
//!
//! No tool-call cards, no permission requests (the agent runs with whatever its headless mode allows; the
//! bridge only ever passes the flags the definition itself declares: no `allow_shell`, no `trust_workspace`,
//! read-only only when the caller asks for it with `--read-only`), no images, no `session/load`, one turn at a
//! time per bridge, and the answer can only be as live as the agent's output is.
//!
//! # Plugging it in (agent A's wiring)
//!
//! ```text
//! let info = acp::bridge::available_for(agent_id)?;   // Err(sentence) = no headless mode
//! let spec = acp::bridge::spawn_spec(agent_id)?;      // conn::SpawnSpec: current exe + args + base env
//! // or as `[acp]`-table fields:  let (command, args) = acp::bridge::bridge_table_parts(agent_id)?;
//! // memory: info.has_memory (a resume mechanism exists); say it on the confirm screen (info.note)
//! ```
//!
//! The bridge needs the base environment of every adapter (`config::BASE_ENV_ALLOW`: PATH, the user profile
//! folders, ...): the host clears the environment and `spawn_spec().env` rebuilds exactly that, plus whatever
//! names `spawn_spec_with_env` is given. The agent's own env rules (`LaunchSpec::env` / `env_remove`) are
//! applied inside the bridge.

mod server;
mod turn;

#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::sync::Arc;

use super::config::BASE_ENV_ALLOW;
use super::conn::SpawnSpec;
use crate::tui::TuiProfile;

pub use server::{serve, ServeOptions};
pub use turn::{run_turn, TurnControl, TurnEnd, TurnLimits, TurnRequest};

/// The subcommand of the app executable.
pub const SUBCOMMAND: &str = "acp-bridge";

/// What a chat through the bridge will be like for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeInfo {
    pub agent_id: String,
    pub label: String,
    /// A resume mechanism exists: a later message continues the earlier ones. False: each message starts fresh.
    pub has_memory: bool,
    /// The agent's headless mode can be started read-only (`--read-only`).
    pub read_only_supported: bool,
    /// The program is on this machine (a missing program still counts as available: install is a separate step).
    pub installed: bool,
    /// One plain sentence for the confirm screen: what the person gets.
    pub note: String,
}

/// The sentence refusing an agent with no headless mode.
pub fn no_headless_sentence(label: &str) -> String {
    format!("{label} has no non-interactive mode, so a chat adapter cannot be made")
}

/// The sentence a chat without memory starts with (once).
pub const NO_MEMORY_NOTICE: &str = "This agent has no conversation memory in chat mode; each message starts fresh.";

/// Whether the bridge can drive `profile`, and what it will be like. Pure over the profile.
pub fn info_for_profile(profile: &dyn TuiProfile, label: &str) -> Result<BridgeInfo, String> {
    if !crate::tui::supports_headless(profile) {
        return Err(no_headless_sentence(label));
    }
    let features = profile.features();
    let has_memory = features.resumable_headless;
    let memory = if has_memory {
        "messages continue the same conversation"
    } else {
        "it has no conversation memory here, so each message starts fresh"
    };
    Ok(BridgeInfo {
        agent_id: profile.id().to_string(),
        label: label.to_string(),
        has_memory,
        read_only_supported: features.read_only_headless,
        installed: profile.is_installed(),
        note: format!("{label} chats through its non-interactive mode: text only, no tool cards, no permission questions; {memory}."),
    })
}

/// `Ok` when `agent_id` is registered (code profile or approved definition) and has a usable headless mode.
pub fn available_for(agent_id: &str) -> Result<BridgeInfo, String> {
    let descriptor = crate::tui::descriptor_for(agent_id).ok_or_else(|| format!("{agent_id} is not a known agent"))?;
    let profile = crate::tui::profile_for(agent_id);
    info_for_profile(&*profile, &descriptor.label)
}

/// The arguments that start the bridge for `agent_id` (after the executable).
pub fn bridge_args(agent_id: &str) -> Vec<String> {
    vec![SUBCOMMAND.to_string(), "--agent".to_string(), agent_id.to_string()]
}

/// `(program, args)` for `[acp]`-table-shaped config: the app executable and `acp-bridge --agent <id>`.
pub fn bridge_table_parts(agent_id: &str) -> Result<(String, Vec<String>), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the app executable: {e}"))?;
    Ok((exe.to_string_lossy().into_owned(), bridge_args(agent_id)))
}

/// The start of the bridge as a `conn::SpawnSpec` (cwd left to the caller). Fails like [`available_for`].
pub fn spawn_spec(agent_id: &str) -> Result<SpawnSpec, String> {
    spawn_spec_with_env(agent_id, &[])
}

/// `spawn_spec` plus extra names of the app's environment handed on to the bridge (and so to the agent's
/// headless process), for an agent that reads a key or a home folder from the environment.
pub fn spawn_spec_with_env(agent_id: &str, extra_env_allow: &[String]) -> Result<SpawnSpec, String> {
    available_for(agent_id)?;
    let (program, args) = bridge_table_parts(agent_id)?;
    let names = BASE_ENV_ALLOW.iter().map(|n| n.to_string()).chain(extra_env_allow.iter().cloned());
    let env: Vec<(OsString, OsString)> =
        names.filter_map(|name| std::env::var_os(&name).map(|value| (OsString::from(name), value))).collect();
    Ok(SpawnSpec { program: OsString::from(program), args: args.into_iter().map(OsString::from).collect(), env, cwd: None })
}

/// The command line of the executable: `acp-bridge --agent <id> [--read-only]`. `None` when the first
/// argument is not ours (the app starts as usual); `Some(code)` when the bridge ran, to be used as the exit code.
pub fn run_cli(args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some(SUBCOMMAND) {
        return None;
    }
    let mut agent: Option<String> = None;
    let mut options = ServeOptions::default();
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--agent" => agent = rest.next().cloned(),
            "--read-only" => options.read_only = true,
            other => {
                eprintln!("acp-bridge: unknown argument {other}");
                return Some(2);
            }
        }
    }
    let Some(agent) = agent.filter(|a| !a.trim().is_empty()) else {
        eprintln!("usage: acp-bridge --agent <id> [--read-only]");
        return Some(2);
    };
    // The same registry the app reads: code profiles, then the approved declarative definitions.
    crate::tui::declarative::loader::reload();
    let (profile, label) = match resolve(&agent) {
        Ok(found) => found,
        Err(sentence) => {
            // Still answer `initialize` with the reason, so the host shows a sentence instead of a closed pipe.
            server::refuse_initialize(std::io::stdin().lock(), std::io::stdout(), &sentence);
            return Some(1);
        }
    };
    serve(profile, label, std::io::stdin().lock(), std::io::stdout(), options);
    Some(0)
}

/// The profile and label of `agent`, or the sentence saying why the bridge cannot drive it.
fn resolve(agent: &str) -> Result<(Arc<dyn TuiProfile>, String), String> {
    let info = available_for(agent)?;
    Ok((crate::tui::profile_for(agent), info.label))
}
