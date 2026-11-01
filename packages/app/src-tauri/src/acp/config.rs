//! Agent spawn configuration, as data.
//!
//! [`AgentConfig`] is also the schema of the `[acp]` table that agent definitions will carry (A3), so a new
//! ACP agent is a TOML table and not code:
//!
//! ```toml
//! [acp]
//! command = "node"
//! args = ["{adapters}/codex/node_modules/@agentclientprotocol/codex-acp/dist/index.js"]
//! env_allow = []
//! resume = "prefer_resume"
//!
//! [acp.env]
//! INITIAL_AGENT_MODE = "read-only"
//!
//! [acp.env_which]            # NAME -> program found on PATH, e.g. the user's own CLI
//! CODEX_PATH = "codex"
//!
//! [acp.private_home]         # a folder only this lane uses, so the user's own agent config is not loaded
//! env_var = "CODEX_HOME"
//! config_file = "config.toml"
//! config_text = "# no tool servers\n"
//!
//! [[acp.pins]]               # versions this build accepts, and the SHA-256 of the file that runs
//! package = "@agentclientprotocol/codex-acp"
//! version = "1.1.4"
//! entry = "{adapters}/codex/node_modules/@agentclientprotocol/codex-acp/dist/index.js"
//! sha256 = "..."
//! ```
//!
//! Nothing outside this file (and `builtin_agents`) names an agent: the connection, session and
//! supervisor code only reads these fields.

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
#[serde(default)]
pub struct AgentConfig {
    /// The agent id used everywhere else in the app (`"codex"`).
    pub id: String,
    pub label: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateHome {
    /// The variable that points the agent at the folder.
    pub env_var: String,
    pub config_file: String,
    pub config_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
pub struct StartRetry {
    /// Lower-case fragments of the error (or of the adapter's stderr) that make a retry worth it.
    pub patterns: Vec<String>,
    /// Waits before each retry, in milliseconds. One entry = one retry.
    pub delays_ms: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
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
    if cfg.command.trim().is_empty() {
        return Err(AcpError::Spawn(format!("agent {:?} has no command", cfg.id)));
    }
    let home = prepare_private_home(cfg, host)?;
    Ok(SpawnSpec {
        program: OsString::from(expand(&cfg.command, host)),
        args: cfg.args.iter().map(|a| OsString::from(expand(a, host))).collect(),
        env: build_env(cfg, host, home.as_deref()),
        cwd: Some(cwd.to_path_buf()),
    })
}

/// `_meta` for the session calls: the config's, or nothing.
pub fn session_meta(cfg: &AgentConfig) -> Option<Value> {
    cfg.session_meta.clone().filter(|v| !v.is_null())
}

// ---- built-in agents -------------------------------------------------------------------------------

/// Agents that ship in this build. Data only: the generic code never matches on `id`.
pub fn builtin_agents() -> Vec<AgentConfig> {
    vec![codex()]
}

pub fn find_builtin(id: &str) -> Option<AgentConfig> {
    builtin_agents().into_iter().find(|cfg| cfg.id.eq_ignore_ascii_case(id.trim()))
}

const CODEX_ADAPTER_ENTRY: &str = "{adapters}/codex/node_modules/@agentclientprotocol/codex-acp/dist/index.js";

/// Codex through its official ACP adapter.
///
/// The user's own Codex config would start every tool server the person has set up (about 25 processes,
/// our own MCP exe among them). So the adapter runs with a private `CODEX_HOME` whose config lists no tool
/// server. The cost: that folder has no sign-in, so the first use needs the agent's own login (A5), or an
/// API key. Sharing the user's `auth.json` is NOT an option: refreshing a token there would invalidate the
/// copy in the user's own folder. [`codex_with_user_config`] is the alternative for development.
pub fn codex() -> AgentConfig {
    AgentConfig {
        id: "codex".into(),
        label: "Codex".into(),
        command: "node".into(),
        args: vec![CODEX_ADAPTER_ENTRY.into()],
        env: BTreeMap::from([
            // Start read-only: a write asks (session/request_permission) instead of happening.
            ("INITIAL_AGENT_MODE".to_string(), "read-only".to_string()),
        ]),
        env_which: BTreeMap::from([("CODEX_PATH".to_string(), "codex".to_string())]),
        private_home: Some(PrivateHome {
            env_var: "CODEX_HOME".into(),
            config_file: "config.toml".into(),
            config_text: "# Puppet Master chat lane. No tool servers are started from this folder.\n".into(),
        }),
        pins: vec![AdapterPin {
            package: "@agentclientprotocol/codex-acp".into(),
            version: "1.1.4".into(),
            entry: CODEX_ADAPTER_ENTRY.into(),
            sha256: Some("7534a0ad3cc4c9affd0b2da5007fa53ea0f1d6fcd71b2c5ef202e2056a976a97".into()),
        }],
        // The adapter answers a session it has never seen with an error; any agent error counts as that.
        unknown_session_patterns: Vec::new(),
        // Codex's own state database can be locked for a moment by a previous process that is still closing.
        start_retry: Some(StartRetry {
            patterns: vec!["failed to initialize sqlite state runtime".into()],
            delays_ms: vec![4000, 8000, 12000],
        }),
        ..AgentConfig::default()
    }
}

/// Codex with the user's own config and login (loads their whole tool-server list). Development only.
pub fn codex_with_user_config() -> AgentConfig {
    AgentConfig { private_home: None, ..codex() }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Host {
        vars: BTreeMap<String, String>,
        programs: BTreeMap<String, PathBuf>,
        root: PathBuf,
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

    #[test]
    fn the_private_home_replaces_the_users_own_config_folder() {
        let host = Host::new();
        let cfg = codex();
        let dir = prepare_private_home(&cfg, &host).unwrap().expect("codex has a private home");
        assert!(dir.starts_with(host.homes_dir()));
        let written = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(!written.contains("mcp_servers"), "the minimal config lists no tool server");
        let env = build_env(&cfg, &host, Some(&dir));
        assert_eq!(lookup(&env, "CODEX_HOME").map(PathBuf::from), Some(dir.clone()));
        // A second call does not rewrite an unchanged file.
        let before = std::fs::metadata(dir.join("config.toml")).unwrap().modified().unwrap();
        prepare_private_home(&cfg, &host).unwrap();
        assert_eq!(std::fs::metadata(dir.join("config.toml")).unwrap().modified().unwrap(), before);
        let _ = std::fs::remove_dir_all(&host.root);
    }

    #[test]
    fn the_dev_variant_keeps_the_users_own_config() {
        let cfg = codex_with_user_config();
        assert!(cfg.private_home.is_none());
        assert_eq!(cfg.id, "codex");
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
