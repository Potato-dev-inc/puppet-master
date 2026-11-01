//! Agent spawn configuration, as data.
//!
//! [`AgentConfig`] is also the schema of the `[acp]` table that agent definitions carry, so a new ACP
//! agent is a TOML table and not code. A code-profile agent (Codex, Claude) keeps its table in
//! `tui/<agent>/acp.toml`; a declarative definition carries it as `[acp]` (docs/specs/agent-definitions).
//!
//! ```toml
//! [acp]
//! command = "node"
//! args = ["{adapters}/codex/node_modules/@agentclientprotocol/codex-acp/dist/index.js"]
//! env_allow = ["CODEX_HOME"]          # names passed on from the app's environment
//! resume = "prefer_resume"
//! resume_command = ["resume", "{session}"]   # arguments added to the agent's normal terminal launch
//!
//! [acp.env]
//! INITIAL_AGENT_MODE = "read-only"
//!
//! [acp.env_which]                     # NAME -> program found on PATH, e.g. the user's own CLI
//! CODEX_PATH = "codex"
//!
//! [acp.quirks]                        # how this adapter differs from the protocol's happy path
//! cancel_reports_end_turn = true      # a stop that was asked for may still end with `end_turn`
//! serial_prompts = true               # one prompt at a time (the only value this build supports)
//!
//! [acp.mcp_config]                    # keep the person's own tool-server list out of the chat session
//! mode = "disable_listed"             # agent_default | session_meta | private_home | disable_listed
//!
//! [[acp.pins]]                        # versions this build accepts, and the SHA-256 of the file that runs
//! package = "@agentclientprotocol/codex-acp"
//! version = "1.1.4"
//! entry = "{adapters}/codex/node_modules/@agentclientprotocol/codex-acp/dist/index.js"
//! sha256 = "..."
//! ```
//!
//! Nothing outside this file, `tui/<agent>/acp.toml` and the definitions names an agent: the connection,
//! session, supervisor and hand-over code only read these fields.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::conn::{ConnLimits, SpawnSpec};
use super::error::AcpError;

/// Names passed from the app's own environment to every adapter (a Windows adapter needs these to start
/// at all). Everything else is dropped; an agent that needs more lists it in `env_allow`.
pub const BASE_ENV_ALLOW: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "USERNAME",
    "TEMP",
    "TMP",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "COMPUTERNAME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "LANG",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    /// The agent id used everywhere else in the app (`"codex"`). Not part of the `[acp]` table: the id and
    /// the label come from the definition's `[agent]` (see [`from_toml_text`]).
    pub id: String,
    pub label: String,
    /// Present in the build but switched off: it is listed (with `disabled_note`) and never started.
    pub disabled: bool,
    /// Why it is off, in a plain sentence the settings screen can show.
    pub disabled_note: String,
    /// The program to start, found on PATH or absolute.
    pub command: String,
    /// Arguments. `{adapters}` is the folder the adapters are installed in.
    pub args: Vec<String>,
    /// Extra names to pass through from the app's environment.
    pub env_allow: Vec<String>,
    /// Fixed variables.
    pub env: BTreeMap<String, String>,
    /// `NAME -> program`: sets NAME to where `program` is on PATH (when found).
    pub env_which: BTreeMap<String, String>,
    /// A folder owned by this lane, so the agent does not read the user's own config (which, for some
    /// agents, starts all their tool servers). The agent then needs its own sign-in in that folder.
    pub private_home: Option<PrivateHome>,
    /// Tool servers handed to `session/new`. Empty by default: the agent gets only what is listed here.
    pub mcp_servers: Vec<Value>,
    /// Merged into `_meta` of `session/new`, `session/load` and `session/resume`.
    pub session_meta: Option<Value>,
    /// Adapter versions this build accepts.
    pub pins: Vec<AdapterPin>,
    pub pin_policy: PinPolicy,
    pub timeouts: Timeouts,
    pub resume: ResumePreference,
    pub history_lost: HistoryLostPolicy,
    /// Retry a start that failed with one of these messages (the agent's own start-up lock, for instance).
    pub start_retry: Option<StartRetry>,
    /// Error text (lower case) that means "the agent does not know this session id", as opposed to a
    /// transient failure. Empty = every agent error on resume counts as "does not know".
    pub unknown_session_patterns: Vec<String>,
    pub quirks: Quirks,
    /// How the person's own tool-server list is kept out of a chat session.
    pub mcp_config: McpConfig,
    /// Arguments added to the agent's normal terminal launch to continue this session there ("Open in
    /// terminal"). `{session}` is the agent's own session id. Empty = the agent cannot be handed over.
    pub resume_command: Vec<String>,
    /// What the install and sign-in screens say about this agent's chat add-on (A5).
    pub addon: AddonInfo,
}

/// `[acp.addon]`: plain words for the install prompt and the sign-in card. Data, so the screens never ask
/// which agent they are showing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AddonInfo {
    /// Roughly how big the install is, for "about N MB". Absent: no size is claimed.
    pub size_mb: Option<u32>,
    /// One sentence for the sign-in card: what to do in the terminal that opens. Empty: a general sentence.
    pub signin_hint: String,
}

/// How an adapter differs from the protocol's happy path. Data, so the session code never asks which
/// agent it is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Quirks {
    /// After a stop was asked for the adapter may still answer `end_turn` instead of `cancelled`. True:
    /// the turn is reported as cancelled anyway (what the person did). False: the adapter's word is kept.
    pub cancel_reports_end_turn: bool,
    /// The adapter takes one prompt at a time. Reserved: only `true` is supported by this build.
    pub serial_prompts: bool,
}

impl Default for Quirks {
    fn default() -> Self {
        Self { cancel_reports_end_turn: true, serial_prompts: true }
    }
}

/// What to do about the person's own tool-server list (the MCP servers in the agent's own settings). A
/// chat session started by this app must not start them all (about 25 processes for one Codex user,
/// including this app's own MCP program).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpMode {
    /// Nothing is done: the agent loads whatever it is set up with.
    #[default]
    AgentDefault,
    /// The agent has a session switch for it, sent in `session_meta`.
    SessionMeta,
    /// A folder only this lane uses (`private_home`). The agent needs its own sign-in there, and its
    /// sessions are separate, so a conversation cannot be handed over to the terminal.
    PrivateHome,
    /// The person's own config is used (login and sessions are shared, so hand-over works) and every tool
    /// server it lists is switched off through a JSON setting the adapter merges into each session.
    DisableListed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListFailure {
    /// The chat session is not started (the caller falls back to the terminal).
    #[default]
    Refuse,
    /// Start anyway, with the agent's own list loaded.
    Continue,
}

/// `[acp.mcp_config]`. Only the fields of the chosen `mode` are read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    pub mode: McpMode,
    /// `disable_listed`: the variable that holds the JSON setting (the adapter merges it into each session).
    pub env: String,
    /// `disable_listed`: JSON merged first, for settings that are not about one server.
    pub base: Value,
    /// `disable_listed`: the agent's own program (found on PATH) that lists its tool servers as JSON.
    pub list_program: String,
    pub list_args: Vec<String>,
    /// `disable_listed`: the field of each listed entry that holds the server's name.
    pub name_field: String,
    /// `disable_listed`: where one server's switch goes in the JSON; `{name}` is the server's name.
    pub disable_path: Vec<String>,
    /// `disable_listed`: the value put there.
    pub disable_value: Value,
    pub on_list_failure: ListFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivateHome {
    /// The variable that points the agent at the folder.
    pub env_var: String,
    pub config_file: String,
    pub config_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdapterPin {
    pub package: String,
    pub version: String,
    /// The file that runs (template). Its SHA-256 is compared when `sha256` is set.
    pub entry: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PinPolicy {
    /// A version or file that is not pinned is refused (the caller falls back to the terminal).
    #[default]
    Enforce,
    /// Logged, then started anyway (development).
    Warn,
    Off,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResumePreference {
    /// `session/resume` first (no history replay, cheaper), `session/load` if the agent has no resume.
    #[default]
    PreferResume,
    /// `session/load` first (replays the history).
    PreferLoad,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryLostPolicy {
    /// Start a new session, tell everyone (`HistoryLost`), and send the prompt there.
    #[default]
    ContinueFresh,
    /// Start a new session, tell everyone, and refuse the prompt: the caller asks the person first.
    Refuse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StartRetry {
    /// Lower-case fragments of the error (or of the adapter's stderr) that make a retry worth it.
    pub patterns: Vec<String>,
    /// Waits before each retry, in milliseconds. One entry = one retry.
    pub delays_ms: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Timeouts {
    pub request_secs: u64,
    pub new_session_secs: u64,
    pub load_secs: u64,
    pub stdin_write_secs: u64,
    /// No byte at all from the agent for this long during a turn: it is asked to stop.
    pub inactivity_secs: u64,
    /// Bytes keep coming but nothing that is real progress (text, tool, plan) for this long: asked to stop.
    pub progress_secs: u64,
    /// After the stop request, how long the agent has to end the turn before it is killed.
    pub hung_grace_secs: u64,
    /// Idle this long and the adapter is stopped (the session stays resumable).
    pub idle_ttl_secs: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            request_secs: 30,
            new_session_secs: 120,
            load_secs: 120,
            stdin_write_secs: 10,
            // The stock adapters send nothing while a model thinks for minutes, so this is generous.
            inactivity_secs: 300,
            progress_secs: 30 * 60,
            hung_grace_secs: 120,
            idle_ttl_secs: 30 * 60,
        }
    }
}

impl Timeouts {
    pub fn request(&self) -> Duration {
        Duration::from_secs(self.request_secs)
    }
    pub fn new_session(&self) -> Duration {
        Duration::from_secs(self.new_session_secs)
    }
    pub fn load(&self) -> Duration {
        Duration::from_secs(self.load_secs)
    }
    pub fn inactivity(&self) -> Duration {
        Duration::from_secs(self.inactivity_secs)
    }
    pub fn progress(&self) -> Duration {
        Duration::from_secs(self.progress_secs)
    }
    pub fn hung_grace(&self) -> Duration {
        Duration::from_secs(self.hung_grace_secs)
    }
    pub fn idle_ttl(&self) -> Duration {
        Duration::from_secs(self.idle_ttl_secs)
    }

    pub fn conn_limits(&self) -> ConnLimits {
        ConnLimits { stdin_write_timeout: Duration::from_secs(self.stdin_write_secs.max(1)), ..ConnLimits::default() }
    }
}

// ---- host environment ------------------------------------------------------------------------------

/// Everything the spawn needs from the machine, behind a trait so tests never read the real environment.
pub trait HostEnv: Send + Sync {
    fn var(&self, name: &str) -> Option<OsString>;
    fn which(&self, program: &str) -> Option<PathBuf>;
    /// Where adapters are installed (`{adapters}`).
    fn adapters_dir(&self) -> PathBuf;
    /// Where private agent homes live; one folder per agent id.
    fn homes_dir(&self) -> PathBuf;
    /// Runs `program args` in `cwd` with exactly `env` (no window, a short time limit) and returns what it
    /// printed on stdout; `None` when it cannot run, fails, or takes too long. Tests answer from a table.
    fn capture(&self, _program: &Path, _args: &[String], _cwd: &Path, _env: &[(OsString, OsString)]) -> Option<String> {
        None
    }
}

pub struct RealHost;

impl HostEnv for RealHost {
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
    fn which(&self, program: &str) -> Option<PathBuf> {
        crate::env_probe::find_on_path(program)
    }
    fn adapters_dir(&self) -> PathBuf {
        crate::app_paths::app_data_dir().join("acp").join("adapters")
    }
    fn homes_dir(&self) -> PathBuf {
        crate::app_paths::app_data_dir().join("acp").join("homes")
    }
    fn capture(&self, program: &Path, args: &[String], cwd: &Path, env: &[(OsString, OsString)]) -> Option<String> {
        capture_stdout(program, args, cwd, env, CAPTURE_LIMIT)
    }
}

/// How long a listing command may run, and how much of its output is read.
const CAPTURE_LIMIT: Duration = Duration::from_secs(10);
const CAPTURE_MAX_BYTES: u64 = 1024 * 1024;

fn capture_stdout(program: &Path, args: &[String], cwd: &Path, env: &[(OsString, OsString)], limit: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut command = crate::proc_util::hidden_command(program);
    command.args(args).current_dir(cwd).env_clear().envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = (&mut stdout).take(CAPTURE_MAX_BYTES).read_to_end(&mut text);
        text
    });
    let deadline = std::time::Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                // Our own child only: killed by handle, never by image name.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().ok()?;
    status.filter(|status| status.success())?;
    String::from_utf8(bytes).ok()
}

pub fn expand(template: &str, host: &dyn HostEnv) -> String {
    template.replace("{adapters}", &host.adapters_dir().to_string_lossy())
}

/// The environment an adapter gets. Pure: the caller's `host` answers every question.
pub fn build_env(cfg: &AgentConfig, host: &dyn HostEnv, private_home: Option<&Path>) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let key = |name: &str| if cfg!(windows) { name.to_ascii_lowercase() } else { name.to_string() };
    let mut put = |out: &mut Vec<(OsString, OsString)>, name: &str, value: OsString| {
        if seen.insert(key(name)) {
            out.push((OsString::from(name), value));
        } else if let Some(slot) = out.iter_mut().find(|(n, _)| key(&n.to_string_lossy()) == key(name)) {
            slot.1 = value;
        }
    };
    for name in BASE_ENV_ALLOW.iter().copied().chain(cfg.env_allow.iter().map(String::as_str)) {
        if let Some(value) = host.var(name) {
            put(&mut out, name, value);
        }
    }
    for (name, program) in &cfg.env_which {
        if let Some(path) = host.which(program) {
            put(&mut out, name, path.into_os_string());
        }
    }
    for (name, value) in &cfg.env {
        put(&mut out, name, OsString::from(expand(value, host)));
    }
    if let (Some(home), Some(spec)) = (private_home, &cfg.private_home) {
        put(&mut out, &spec.env_var, home.as_os_str().to_os_string());
    }
    out
}

/// Makes the private home folder and writes its config (only when the text changed). Returns the folder.
pub fn prepare_private_home(cfg: &AgentConfig, host: &dyn HostEnv) -> Result<Option<PathBuf>, AcpError> {
    let Some(spec) = &cfg.private_home else { return Ok(None) };
    let dir = host.homes_dir().join(sanitize_dir_name(&cfg.id));
    std::fs::create_dir_all(&dir).map_err(|e| AcpError::Spawn(format!("cannot make {}: {e}", dir.display())))?;
    let file = dir.join(&spec.config_file);
    let unchanged = std::fs::read_to_string(&file).map(|old| old == spec.config_text).unwrap_or(false);
    if !unchanged {
        crate::settings_patch::write_atomic(&file, spec.config_text.as_bytes())
            .map_err(|e| AcpError::Spawn(format!("cannot write {}: {e}", file.display())))?;
    }
    Ok(Some(dir))
}

fn sanitize_dir_name(id: &str) -> String {
    let cleaned: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if cleaned.is_empty() {
        "agent".into()
    } else {
        cleaned
    }
}

/// Everything needed to start the adapter for a session in `cwd`.
pub fn resolve_spawn(cfg: &AgentConfig, host: &dyn HostEnv, cwd: &Path) -> Result<SpawnSpec, AcpError> {
    if cfg.disabled {
        let why = if cfg.disabled_note.trim().is_empty() { "it is switched off in this build" } else { cfg.disabled_note.trim() };
        return Err(AcpError::Spawn(format!("chat for {:?} is off: {why}", cfg.id)));
    }
    if cfg.command.trim().is_empty() {
        return Err(AcpError::Spawn(format!("agent {:?} has no command", cfg.id)));
    }
    let home = prepare_private_home(cfg, host)?;
    let mut env = build_env(cfg, host, home.as_deref());
    if let Some((name, value)) = mcp_isolation(cfg, host, cwd, &env)? {
        env.retain(|(n, _)| !n.to_string_lossy().eq_ignore_ascii_case(&name.to_string_lossy()));
        env.push((name, value));
    }
    Ok(SpawnSpec {
        program: OsString::from(expand(&cfg.command, host)),
        args: cfg.args.iter().map(|a| OsString::from(expand(a, host))).collect(),
        env,
        cwd: Some(cwd.to_path_buf()),
    })
}

/// `_meta` for the session calls: the config's, or nothing.
pub fn session_meta(cfg: &AgentConfig) -> Option<Value> {
    cfg.session_meta.clone().filter(|v| !v.is_null())
}

// ---- keeping the person's own tool servers out ------------------------------------------------------

/// Most tool servers one listing may name, and the longest name accepted.
const MAX_LISTED_SERVERS: usize = 256;
const MAX_SERVER_NAME_CHARS: usize = 128;

/// The variable and JSON text that switch the person's tool servers off for one adapter start
/// (`McpMode::DisableListed`), or `None` for every other mode. The person's own CLI is asked which servers
/// are set up (so a server added yesterday is covered today); each is switched off by name.
pub fn mcp_isolation(cfg: &AgentConfig, host: &dyn HostEnv, cwd: &Path, env: &[(OsString, OsString)]) -> Result<Option<(OsString, OsString)>, AcpError> {
    let m = &cfg.mcp_config;
    if m.mode != McpMode::DisableListed {
        return Ok(None);
    }
    let names = match list_server_names(m, host, cwd, env) {
        Ok(names) => names,
        Err(why) => match m.on_list_failure {
            ListFailure::Refuse => {
                return Err(AcpError::Spawn(format!("cannot tell which tool servers {:?} is set up with ({why}), so chat is not started", cfg.id)));
            }
            ListFailure::Continue => {
                tracing::warn!(agent = %cfg.id, "acp: could not list the agent's tool servers ({why}); starting with its own list");
                Vec::new()
            }
        },
    };
    let json = isolation_json(m, &names).map_err(|e| AcpError::Spawn(format!("tool-server setting for {:?}: {e}", cfg.id)))?;
    Ok(Some((OsString::from(&m.env), OsString::from(json))))
}

fn list_server_names(m: &McpConfig, host: &dyn HostEnv, cwd: &Path, env: &[(OsString, OsString)]) -> Result<Vec<String>, String> {
    let program = host.which(&m.list_program).ok_or_else(|| format!("{} was not found", m.list_program))?;
    let output = host.capture(&program, &m.list_args, cwd, env).ok_or_else(|| format!("{} did not list them", m.list_program))?;
    parse_server_names(&output, &m.name_field)
}

/// Names from a JSON array of objects, each with a string `name_field`. Anything else is an error: a list
/// that cannot be read is not a list that is empty.
pub fn parse_server_names(output: &str, name_field: &str) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_str(output.trim()).map_err(|_| "the list is not JSON".to_string())?;
    let items = value.as_array().ok_or_else(|| "the list is not an array".to_string())?;
    if items.len() > MAX_LISTED_SERVERS {
        return Err(format!("more than {MAX_LISTED_SERVERS} servers"));
    }
    let mut names: Vec<String> = Vec::new();
    for item in items {
        let name = item.get(name_field).and_then(Value::as_str).ok_or_else(|| format!("an entry has no {name_field:?}"))?;
        if name.is_empty() || name.chars().count() > MAX_SERVER_NAME_CHARS || name.chars().any(char::is_control) {
            return Err("a server name is empty, too long or has control characters".into());
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// `base` with one switch per name merged in.
pub fn isolation_json(m: &McpConfig, names: &[String]) -> Result<String, String> {
    let mut root = if m.base.is_object() { m.base.clone() } else { Value::Object(Default::default()) };
    for name in names {
        let path: Vec<String> = m.disable_path.iter().map(|part| part.replace("{name}", name)).collect();
        set_at(&mut root, &path, m.disable_value.clone());
    }
    serde_json::to_string(&root).map_err(|e| e.to_string())
}

/// Puts `value` at `path` (objects are made on the way; two objects are merged, not replaced).
fn set_at(root: &mut Value, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else { return };
    let mut at = root;
    for part in parents {
        if !at.is_object() {
            *at = Value::Object(Default::default());
        }
        at = at.as_object_mut().expect("just made an object").entry(part.clone()).or_insert_with(|| Value::Object(Default::default()));
    }
    if !at.is_object() {
        *at = Value::Object(Default::default());
    }
    let slot = at.as_object_mut().expect("just made an object").entry(last.clone()).or_insert(Value::Null);
    merge_json(slot, value);
}

fn merge_json(into: &mut Value, from: Value) {
    match (into, from) {
        (Value::Object(into), Value::Object(from)) => {
            for (key, value) in from {
                merge_json(into.entry(key).or_insert(Value::Null), value);
            }
        }
        (into, from) => *into = from,
    }
}

// ---- the table: parsing and checking -----------------------------------------------------------------

/// An `[acp]` table's text, with the identity (`[agent] id`, `label`) put in. Fails on an unknown field, a
/// field with the wrong type, or a table that does not pass [`table_problems`].
pub fn from_toml_text(id: &str, label: &str, text: &str) -> Result<AgentConfig, String> {
    let mut cfg: AgentConfig = toml::from_str(text).map_err(|e| e.to_string())?;
    if let Some((path, message)) = table_problems(&cfg).into_iter().next() {
        return Err(format!("{path}: {message}"));
    }
    cfg.id = id.to_string();
    cfg.label = label.to_string();
    Ok(cfg)
}

/// What is wrong with a table as it appears in a definition (the id and label are not part of it).
pub fn table_problems(cfg: &AgentConfig) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !cfg.id.is_empty() || !cfg.label.is_empty() {
        out.push(("acp".to_string(), "id and label come from [agent]; remove them from [acp]".to_string()));
    }
    out.extend(problems(cfg));
    out
}

const MAX_TEXT: usize = 512;

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')')
        && name.len() <= 128
}

fn plain_text(text: &str) -> bool {
    text.chars().count() <= MAX_TEXT && !text.chars().any(char::is_control)
}

/// Every reason this config could not be used, as `(path, message)`. Empty = fine. Run on definitions that
/// come from outside the build, and on the built-in tables (a test keeps those clean).
pub fn problems(cfg: &AgentConfig) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |path: &str, message: String| out.push((format!("acp.{path}"), message));
    if cfg.command.trim().is_empty() || !plain_text(&cfg.command) {
        add("command", "needed, at most 512 characters, no control characters".into());
    }
    if cfg.args.len() > 64 || !cfg.args.iter().all(|a| plain_text(a)) {
        add("args", "at most 64 arguments of at most 512 characters, no control characters".into());
    }
    for name in cfg.env_allow.iter().chain(cfg.env.keys()).chain(cfg.env_which.keys()) {
        if !valid_env_name(name) {
            add("env", format!("{name:?} is not a variable name"));
        }
    }
    if cfg.env.len() > 32 || cfg.env_allow.len() > 32 || cfg.env_which.len() > 8 || !cfg.env.values().all(|v| plain_text(v)) {
        add("env", "too many variables, or a value is too long".into());
    }
    if cfg.env_which.values().any(|p| p.is_empty() || p.contains(['/', '\\', ':']) || !plain_text(p)) {
        add("env_which", "each value is a bare program name looked up on PATH".into());
    }
    if cfg.mcp_servers.len() > 16 || !cfg.mcp_servers.iter().all(Value::is_object) {
        add("mcp_servers", "at most 16 tool servers, each an object".into());
    }
    if cfg.pins.len() > 16 {
        add("pins", "at most 16 pins".into());
    }
    for (i, pin) in cfg.pins.iter().enumerate() {
        if pin.package.trim().is_empty() || pin.version.trim().is_empty() || pin.entry.trim().is_empty() {
            add(&format!("pins.{i}"), "package, version and entry are needed".into());
        }
        if pin.sha256.as_deref().is_some_and(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit())) {
            add(&format!("pins.{i}.sha256"), "must be 64 hex characters".into());
        }
    }
    if cfg.pin_policy == PinPolicy::Enforce && !cfg.pins.is_empty() && cfg.pins.iter().all(|p| p.sha256.is_none()) {
        add("pins", "with pin_policy \"enforce\" at least one pin needs a sha256".into());
    }
    let t = &cfg.timeouts;
    let secs = [t.request_secs, t.new_session_secs, t.load_secs, t.stdin_write_secs, t.inactivity_secs, t.progress_secs, t.hung_grace_secs, t.idle_ttl_secs];
    if secs.iter().any(|s| *s == 0 || *s > 24 * 3600) {
        add("timeouts", "every value is between 1 and 86400 seconds".into());
    }
    if !cfg.quirks.serial_prompts {
        add("quirks.serial_prompts", "reserved: this build supports one prompt at a time only (true)".into());
    }
    if cfg.unknown_session_patterns.len() > 16 || !cfg.unknown_session_patterns.iter().all(|p| plain_text(p)) {
        add("unknown_session_patterns", "at most 16 short patterns".into());
    }
    if cfg.addon.size_mb.is_some_and(|mb| mb == 0 || mb > 4096) || cfg.addon.signin_hint.chars().count() > 300 || cfg.addon.signin_hint.chars().any(char::is_control) {
        add("addon", "size_mb is 1 to 4096; signin_hint is at most 300 characters on one line".into());
    }
    if cfg.disabled_note.chars().count() > 300 {
        add("disabled_note", "at most 300 characters".into());
    }
    if !cfg.resume_command.is_empty() && crate::session_chat::history::resume_args_from(&cfg.resume_command, SAMPLE_SESSION).is_none() {
        add("resume_command", "must contain {session} and no other braces".into());
    }
    if cfg.resume_command.len() > 16 || !cfg.resume_command.iter().all(|a| plain_text(a)) {
        add("resume_command", "at most 16 arguments of at most 512 characters".into());
    }
    let m = &cfg.mcp_config;
    match m.mode {
        McpMode::AgentDefault => {}
        McpMode::SessionMeta => {
            if session_meta(cfg).is_none() {
                add("mcp_config.mode", "\"session_meta\" needs a session_meta table".into());
            }
        }
        McpMode::PrivateHome => {
            if cfg.private_home.is_none() {
                add("mcp_config.mode", "\"private_home\" needs a private_home table".into());
            }
        }
        McpMode::DisableListed => {
            if !valid_env_name(&m.env) {
                add("mcp_config.env", "must be the name of the variable that holds the setting".into());
            }
            if m.list_program.is_empty() || m.list_program.contains(['/', '\\', ':']) || !plain_text(&m.list_program) {
                add("mcp_config.list_program", "a bare program name looked up on PATH".into());
            }
            if m.list_args.len() > 16 || !m.list_args.iter().all(|a| plain_text(a)) {
                add("mcp_config.list_args", "at most 16 short arguments".into());
            }
            if m.name_field.is_empty() || !plain_text(&m.name_field) {
                add("mcp_config.name_field", "needed".into());
            }
            if m.disable_path.is_empty() || m.disable_path.len() > 8 || !m.disable_path.iter().any(|p| p.contains("{name}")) {
                add("mcp_config.disable_path", "1 to 8 parts, at least one with {name}".into());
            }
            if !m.disable_value.is_object() && !m.disable_value.is_boolean() {
                add("mcp_config.disable_value", "an object or a boolean".into());
            }
            if !m.base.is_null() && !m.base.is_object() {
                add("mcp_config.base", "an object".into());
            }
            if cfg.env.keys().any(|k| k.eq_ignore_ascii_case(&m.env)) {
                add("mcp_config.env", "is also set in [acp.env]".into());
            }
        }
    }
    if m.mode != McpMode::PrivateHome && cfg.private_home.is_some() {
        add("private_home", "set mcp_config.mode = \"private_home\" to use it".into());
    }
    out
}

/// A session id shaped like a real one, for checking templates.
const SAMPLE_SESSION: &str = "00000000-0000-0000-0000-000000000000";

// ---- agents with an ACP table -------------------------------------------------------------------------

/// Every registered agent that carries an `[acp]` table (code profiles and approved definitions), in
/// registry order. Data only: the generic code never matches on `id`.
pub fn builtin_agents() -> Vec<AgentConfig> {
    crate::tui::registry::registry().ordered().iter().filter_map(|profile| profile.acp_table()).collect()
}

/// The chat config of a registered agent: its own `[acp]` table, else (only when it has none) the person's
/// chat overlay (`acp::overlay`, "Add chat (ACP)").
pub fn find_builtin(id: &str) -> Option<AgentConfig> {
    let profile = crate::tui::registry::registry().find(id.trim())?;
    match profile.acp_table() {
        Some(table) => Some(table),
        None => {
            let label = crate::tui::descriptor_for(profile.id()).map(|d| d.label);
            super::overlay::resolve_with(&super::overlay::OverlayStore::in_app_data(), profile.id(), None, label)
        }
    }
}

/// The Codex table (for tests and tools).
pub fn codex() -> AgentConfig {
    find_builtin("codex").expect("the codex profile carries an [acp] table")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Host {
        vars: BTreeMap<String, String>,
        programs: BTreeMap<String, PathBuf>,
        root: PathBuf,
        /// What the agent's listing command prints (None = it fails).
        listing: Option<String>,
        asked: std::sync::Mutex<Vec<Vec<String>>>,
    }

    impl Host {
        fn new() -> Self {
            Host {
                vars: BTreeMap::from([
                    ("PATH".into(), "C:\\bin".into()),
                    ("SystemRoot".into(), "C:\\Windows".into()),
                    ("ANTHROPIC_API_KEY".into(), "sk-secret".into()),
                    ("OPENAI_API_KEY".into(), "sk-other".into()),
                    ("GITHUB_TOKEN".into(), "ghp_secret".into()),
                ]),
                programs: BTreeMap::from([("codex".into(), PathBuf::from("C:\\tools\\codex.exe"))]),
                root: std::env::temp_dir().join(format!("pm-acp-config-{}", uuid::Uuid::new_v4())),
                listing: Some(r#"[{"name":"chrome-devtools","enabled":true},{"name":"puppet-master","enabled":true}]"#.into()),
                asked: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl HostEnv for Host {
        fn var(&self, name: &str) -> Option<OsString> {
            self.vars.get(name).map(OsString::from)
        }
        fn which(&self, program: &str) -> Option<PathBuf> {
            self.programs.get(program).cloned()
        }
        fn adapters_dir(&self) -> PathBuf {
            self.root.join("adapters")
        }
        fn homes_dir(&self) -> PathBuf {
            self.root.join("homes")
        }
        fn capture(&self, _program: &Path, args: &[String], _cwd: &Path, _env: &[(OsString, OsString)]) -> Option<String> {
            self.asked.lock().unwrap().push(args.to_vec());
            self.listing.clone()
        }
    }

    fn lookup(env: &[(OsString, OsString)], name: &str) -> Option<String> {
        env.iter().find(|(n, _)| n.to_string_lossy() == name).map(|(_, v)| v.to_string_lossy().into_owned())
    }

    #[test]
    fn secrets_in_the_apps_environment_do_not_reach_the_adapter() {
        let host = Host::new();
        let env = build_env(&codex(), &host, None);
        assert_eq!(lookup(&env, "PATH").as_deref(), Some("C:\\bin"));
        assert_eq!(lookup(&env, "SystemRoot").as_deref(), Some("C:\\Windows"));
        assert!(lookup(&env, "ANTHROPIC_API_KEY").is_none());
        assert!(lookup(&env, "OPENAI_API_KEY").is_none());
        assert!(lookup(&env, "GITHUB_TOKEN").is_none());
    }

    #[test]
    fn an_agent_can_ask_for_more_names_and_resolves_its_own_cli() {
        let host = Host::new();
        let mut cfg = codex();
        cfg.env_allow.push("OPENAI_API_KEY".into());
        let env = build_env(&cfg, &host, None);
        assert_eq!(lookup(&env, "OPENAI_API_KEY").as_deref(), Some("sk-other"));
        assert_eq!(lookup(&env, "CODEX_PATH").as_deref(), Some("C:\\tools\\codex.exe"));
        assert_eq!(lookup(&env, "INITIAL_AGENT_MODE").as_deref(), Some("read-only"));
    }

    fn private_home_config() -> AgentConfig {
        AgentConfig {
            id: "x".into(),
            command: "node".into(),
            private_home: Some(PrivateHome { env_var: "X_HOME".into(), config_file: "config.toml".into(), config_text: "# no tool servers\n".into() }),
            mcp_config: McpConfig { mode: McpMode::PrivateHome, ..McpConfig::default() },
            ..AgentConfig::default()
        }
    }

    #[test]
    fn the_private_home_replaces_the_users_own_config_folder() {
        let host = Host::new();
        let cfg = private_home_config();
        let dir = prepare_private_home(&cfg, &host).unwrap().expect("this agent has a private home");
        assert!(dir.starts_with(host.homes_dir()));
        let written = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(!written.contains("mcp_servers"), "the minimal config lists no tool server");
        let env = build_env(&cfg, &host, Some(&dir));
        assert_eq!(lookup(&env, "X_HOME").map(PathBuf::from), Some(dir.clone()));
        // A second call does not rewrite an unchanged file.
        let before = std::fs::metadata(dir.join("config.toml")).unwrap().modified().unwrap();
        prepare_private_home(&cfg, &host).unwrap();
        assert_eq!(std::fs::metadata(dir.join("config.toml")).unwrap().modified().unwrap(), before);
        let _ = std::fs::remove_dir_all(&host.root);
    }

    #[test]
    fn codex_uses_the_persons_own_home_not_a_private_one() {
        let cfg = codex();
        assert!(cfg.private_home.is_none());
        assert_eq!(cfg.mcp_config.mode, McpMode::DisableListed);
        assert_eq!(cfg.resume_command, vec!["resume".to_string(), "{session}".to_string()]);
        let host = Host { vars: BTreeMap::from([("CODEX_HOME".into(), "D:\\mine".into()), ("PATH".into(), "C:\\bin".into())]), ..Host::new() };
        assert_eq!(lookup(&build_env(&cfg, &host, None), "CODEX_HOME").as_deref(), Some("D:\\mine"), "a custom home reaches the adapter");
    }

    #[test]
    fn the_listed_tool_servers_are_switched_off_by_name_and_plugins_are_off() {
        let host = Host::new();
        let spec = resolve_spawn(&codex(), &host, Path::new("C:\\work")).unwrap();
        let setting = lookup(&spec.env, "CODEX_CONFIG").expect("the setting is passed");
        let value: Value = serde_json::from_str(&setting).unwrap();
        assert_eq!(value["mcp_servers"]["chrome-devtools"]["enabled"], false);
        assert_eq!(value["mcp_servers"]["puppet-master"]["enabled"], false);
        assert_eq!(value["features"]["plugins"], false);
        assert_eq!(value["mcp_servers"].as_object().unwrap().len(), 2, "only the listed ones");
        let asked = host.asked.lock().unwrap().clone();
        assert_eq!(asked, vec![vec!["mcp", "list", "--json", "--disable", "plugins"]]);
        let _ = std::fs::remove_dir_all(&host.root);
    }

    #[test]
    fn a_listing_that_fails_or_cannot_be_read_refuses_the_start() {
        let cwd = Path::new("C:\\work");
        for listing in [None, Some("not json".to_string()), Some(r#"{"name":"x"}"#.to_string()), Some(r#"[{"id":"x"}]"#.to_string())] {
            let host = Host { listing, ..Host::new() };
            let err = resolve_spawn(&codex(), &host, cwd).unwrap_err().to_string();
            assert!(err.contains("chat is not started"), "{err}");
        }
        let host = Host { programs: BTreeMap::new(), ..Host::new() };
        assert!(resolve_spawn(&codex(), &host, cwd).is_err(), "no CLI, no start");
    }

    #[test]
    fn continue_policy_starts_with_the_base_setting_only() {
        let mut cfg = codex();
        cfg.mcp_config.on_list_failure = ListFailure::Continue;
        let host = Host { listing: None, ..Host::new() };
        let spec = resolve_spawn(&cfg, &host, Path::new("C:\\work")).unwrap();
        let value: Value = serde_json::from_str(&lookup(&spec.env, "CODEX_CONFIG").unwrap()).unwrap();
        assert!(value.get("mcp_servers").is_none());
        assert_eq!(value["features"]["plugins"], false);
    }

    #[test]
    fn an_empty_listing_still_passes_the_base_setting() {
        let host = Host { listing: Some("[]".into()), ..Host::new() };
        let spec = resolve_spawn(&codex(), &host, Path::new("C:\\work")).unwrap();
        let value: Value = serde_json::from_str(&lookup(&spec.env, "CODEX_CONFIG").unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"features": {"plugins": false}}));
    }

    #[test]
    fn server_names_with_odd_characters_stay_data() {
        let names = parse_server_names(r#"[{"name":"a.b"},{"name":"a.b"},{"name":"c d\"e"}]"#, "name").unwrap();
        assert_eq!(names, vec!["a.b".to_string(), "c d\"e".to_string()]);
        let json = isolation_json(&codex().mcp_config, &names).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["mcp_servers"]["a.b"]["enabled"], false, "the name is a key, not a path");
        assert!(parse_server_names(r#"[{"name":"\u0007"}]"#, "name").is_err());
        assert!(parse_server_names(&format!(r#"[{{"name":"{}"}}]"#, "x".repeat(300)), "name").is_err());
    }

    #[test]
    fn claude_is_on_and_keeps_the_persons_own_login_and_sessions() {
        let cfg = find_builtin("claude").expect("claude has an [acp] table");
        assert!(!cfg.disabled, "founder decision 2026-11-01: the subscription login");
        assert_eq!(cfg.label, "Claude");
        assert!(!cfg.label.contains("Code"), "branding rule");
        assert!(cfg.private_home.is_none());
        assert_eq!(cfg.mcp_config.mode, McpMode::SessionMeta);
        assert_eq!(session_meta(&cfg).unwrap()["claudeCode"]["options"]["strictMcpConfig"], true);
        assert_eq!(cfg.resume_command, vec!["--resume".to_string(), "{session}".to_string()]);
        let env_names: Vec<&String> = cfg.env.keys().chain(cfg.env_allow.iter()).collect();
        assert!(env_names.iter().all(|n| !n.to_ascii_uppercase().contains("KEY")), "no API key is asked for");
    }

    #[test]
    fn a_disabled_agent_is_listed_but_never_started() {
        let cfg = AgentConfig { id: "x".into(), command: "node".into(), disabled: true, disabled_note: "waiting".into(), ..AgentConfig::default() };
        let err = resolve_spawn(&cfg, &Host::new(), Path::new("C:\\work")).unwrap_err().to_string();
        assert!(err.contains("waiting"), "{err}");
    }

    #[test]
    fn the_built_in_tables_pass_their_own_checks() {
        for cfg in builtin_agents() {
            let found = problems(&cfg);
            assert!(found.is_empty(), "{}: {found:?}", cfg.id);
        }
        assert!(find_builtin("codex").is_some() && find_builtin("claude").is_some());
    }

    #[test]
    fn resume_commands_match_what_the_chat_history_already_uses() {
        for id in ["codex", "claude"] {
            let cfg = find_builtin(id).unwrap();
            let profile = crate::tui::registry::registry().find(id).unwrap();
            let history = profile.session_history().expect("history");
            assert_eq!(
                crate::session_chat::history::resume_args_from(&cfg.resume_command, SAMPLE_SESSION),
                history.resume_args(SAMPLE_SESSION),
                "{id}: the [acp] resume_command and the history resume_args disagree"
            );
        }
    }

    #[test]
    fn a_table_from_a_definition_is_checked_strictly() {
        let ok = "command = \"adapter\"\n[quirks]\ncancel_reports_end_turn = false\n";
        let cfg = from_toml_text("mine", "Mine", ok).unwrap();
        assert_eq!((cfg.id.as_str(), cfg.label.as_str()), ("mine", "Mine"));
        assert!(!cfg.quirks.cancel_reports_end_turn && cfg.quirks.serial_prompts);
        for bad in [
            "command = \"a\"\ntypo = 1",
            "command = \"a\"\nid = \"other\"",
            "command = \"\"",
            "command = \"a\"\n[quirks]\nserial_prompts = false",
            "command = \"a\"\nresume_command = [\"resume\"]",
            "command = \"a\"\nresume_command = [\"x\", \"{session}\", \"{other}\"]",
            "command = \"a\"\n[mcp_config]\nmode = \"session_meta\"",
            "command = \"a\"\n[mcp_config]\nmode = \"disable_listed\"",
            "command = \"a\"\n[env]\n\"BAD NAME\" = \"x\"",
            "command = \"a\"\n[[pins]]\npackage = \"p\"\nversion = \"1\"\nentry = \"e\"\nsha256 = \"zz\"",
            "command = \"a\"\n[[pins]]\npackage = \"p\"\nversion = \"1\"\nentry = \"e\"",
        ] {
            assert!(from_toml_text("mine", "Mine", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn capture_reads_stdout_and_gives_up_on_failure() {
        let cwd = std::env::temp_dir();
        let env = [
            (OsString::from("SystemRoot"), OsString::from(std::env::var("SystemRoot").unwrap_or_default())),
            (OsString::from("PATH"), OsString::from(std::env::var("PATH").unwrap_or_default())),
        ];
        let (program, args): (&str, Vec<String>) = if cfg!(windows) { ("cmd", vec!["/C".into(), "echo hi".into()]) } else { ("sh", vec!["-c".into(), "echo hi".into()]) };
        let out = capture_stdout(Path::new(program), &args, &cwd, &env, Duration::from_secs(10));
        assert_eq!(out.map(|t| t.trim().to_string()), Some("hi".to_string()));
        assert!(capture_stdout(Path::new("definitely-not-a-program-xyz"), &[], &cwd, &[], Duration::from_secs(2)).is_none());
    }

    #[test]
    fn templates_expand_in_the_command_and_arguments() {
        let host = Host::new();
        let spec = resolve_spawn(&codex(), &host, Path::new("C:\\work")).unwrap();
        let arg = spec.args[0].to_string_lossy().into_owned();
        assert!(arg.starts_with(&host.adapters_dir().to_string_lossy().into_owned()), "{arg}");
        assert!(arg.ends_with("codex-acp/dist/index.js"));
        assert_eq!(spec.cwd.as_deref(), Some(Path::new("C:\\work")));
        let _ = std::fs::remove_dir_all(&host.root);
    }

    #[test]
    fn a_config_without_a_command_is_refused() {
        let host = Host::new();
        assert!(matches!(resolve_spawn(&AgentConfig::default(), &host, Path::new("C:\\")), Err(AcpError::Spawn(_))));
    }

    #[test]
    fn the_toml_table_of_the_design_note_parses_into_the_struct() {
        let text = r##"
            command = "node"
            args = ["{adapters}/x/index.js"]
            resume = "prefer_load"
            history_lost = "refuse"
            [env]
            MODE = "read-only"
            [env_which]
            X_PATH = "xcli"
            [private_home]
            env_var = "X_HOME"
            config_file = "config.toml"
            config_text = "# none\n"
            [timeouts]
            inactivity_secs = 60
            [session_meta]
            claudeLike = { strict = true }
            [[pins]]
            package = "pkg"
            version = "1.0.0"
            entry = "{adapters}/x/index.js"
        "##;
        let cfg: AgentConfig = toml::from_str(text).unwrap();
        assert_eq!(cfg.command, "node");
        assert_eq!(cfg.resume, ResumePreference::PreferLoad);
        assert_eq!(cfg.history_lost, HistoryLostPolicy::Refuse);
        assert_eq!(cfg.timeouts.inactivity_secs, 60);
        assert_eq!(cfg.timeouts.progress_secs, 1800, "unset fields keep their defaults");
        assert_eq!(cfg.private_home.unwrap().env_var, "X_HOME");
        assert_eq!(cfg.session_meta.unwrap()["claudeLike"]["strict"], serde_json::json!(true));
        assert_eq!(cfg.pins[0].sha256, None);
        assert_eq!(cfg.pin_policy, PinPolicy::Enforce);
    }

    #[test]
    fn builtin_lookup_ignores_case_and_unknown_ids() {
        assert!(find_builtin("Codex").is_some());
        assert!(find_builtin("nope").is_none());
    }
}
