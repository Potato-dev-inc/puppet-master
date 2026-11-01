//! "Add chat (ACP)" for an agent that has no `[acp]` table of its own: a per-agent OVERLAY.
//!
//! A built-in code profile, a bundled catalog definition or a definition the person does not want to edit cannot
//! always carry an `[acp]` table. The chat setup is kept beside it instead: `<app data>/acp_overlay.json`, agent id ->
//! `{command, args, env_allow, resume_command?}` (the same fields as the wizard's `acp` object).
//!
//! Rules:
//! * a real `[acp]` table always wins; the overlay answers only when the agent has none ([`resolve_with`]);
//! * an overlay is built into an [`AgentConfig`] and held to the same checks as a definition's table
//!   (`table_problems`, VAL-4 the shell/interpreter rule, VAL-5 the denied environment names, no secret-looking text);
//!   an invalid one is refused on write and ignored on read, never a crash;
//! * the file is written only by the desktop app's own commands (`commands/acp_overlay.rs`), after the person saw
//!   the exact command line and confirmed. No bridge route, no MCP tool.
//!
//! Nothing here names an agent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config::AgentConfig;

const FILE: &str = "acp_overlay.json";
const FILE_VERSION: u32 = 1;

/// Serialises read-modify-write of the file inside this process.
static WRITE: Mutex<()> = Mutex::new(());

/// What the person confirmed: the command line of the adapter, the variables passed on, and how to continue the
/// session in the agent's own terminal (empty = no hand-over).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OverlayEntry {
    pub command: String,
    pub args: Vec<String>,
    pub env_allow: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resume_command: Vec<String>,
}

impl OverlayEntry {
    /// Reads the `acp` object of a command. Strict: an unknown field or a wrong type is an error.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        if !value.is_object() {
            return Err("INVALID_ARGUMENT: acp must be an object {command, args, env_allow, resume_command?}".into());
        }
        let entry: OverlayEntry = serde_json::from_value(value.clone()).map_err(|e| format!("INVALID_ARGUMENT: acp: {e}"))?;
        Ok(entry)
    }

    /// The command line as one line of text, for the person to read before confirming.
    pub fn command_line(&self) -> String {
        std::iter::once(self.command.as_str()).chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }
}

/// The overlay as an `AgentConfig` for agent `id`, after the same checks a definition's `[acp]` table gets.
pub fn build_config(id: &str, label: &str, entry: &OverlayEntry) -> Result<AgentConfig, String> {
    let cfg = AgentConfig {
        command: entry.command.trim().to_string(),
        args: entry.args.clone(),
        env_allow: entry.env_allow.clone(),
        resume_command: entry.resume_command.clone(),
        ..AgentConfig::default()
    };
    // No secret in a stored command line: anything the redactor would change is refused.
    for text in std::iter::once(&cfg.command).chain(&cfg.args).chain(&cfg.resume_command) {
        if crate::redact::text(text).into_inner() != *text {
            return Err("INVALID_ARGUMENT: acp holds text that looks like a secret; it is never stored".into());
        }
    }
    // The overlay never has `allow_interpreter`: a shell or interpreter as the adapter is refused (VAL-4).
    if let Some((code, path, message)) = crate::tui::declarative::validate::acp_table_errors(&cfg, false).into_iter().next() {
        return Err(format!("{code}: {path}: {message}"));
    }
    Ok(AgentConfig { id: id.to_string(), label: label.to_string(), ..cfg })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoreFile {
    version: u32,
    /// Kept as raw values: one entry this build cannot read never discards the others.
    agents: BTreeMap<String, Value>,
}

fn key(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

/// The overlay file. `at` is a parameter so tests use a temp folder.
#[derive(Debug, Clone)]
pub struct OverlayStore {
    path: PathBuf,
}

impl OverlayStore {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }

    pub fn in_app_data() -> Self {
        Self::at(crate::app_paths::app_data_dir().join(FILE))
    }

    fn read(&self) -> StoreFile {
        std::fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// The stored entry of `agent`; a missing file, a broken file, an unreadable entry or one that no longer
    /// passes the checks is `None`.
    pub fn get(&self, agent: &str) -> Option<OverlayEntry> {
        let id = key(agent);
        let entry = OverlayEntry::from_json(self.read().agents.get(&id)?).ok()?;
        build_config(&id, &id, &entry).ok().map(|_| entry)
    }

    /// Ids that have a usable overlay.
    pub fn ids(&self) -> Vec<String> {
        self.read().agents.keys().filter(|id| self.get(id).is_some()).cloned().collect()
    }

    /// Writes one agent's entry. The caller has checked it with [`build_config`].
    pub fn put(&self, agent: &str, entry: &OverlayEntry) -> Result<(), String> {
        let id = key(agent);
        if id.is_empty() {
            return Err("INVALID_ARGUMENT: agent id is required".into());
        }
        let _guard = WRITE.lock();
        let mut file = self.read();
        file.version = FILE_VERSION;
        file.agents.insert(id, serde_json::to_value(entry).map_err(|e| e.to_string())?);
        self.save(&file)
    }

    /// Removes one agent's entry. Returns whether there was one.
    pub fn remove(&self, agent: &str) -> Result<bool, String> {
        let _guard = WRITE.lock();
        let mut file = self.read();
        let had = file.agents.remove(&key(agent)).is_some();
        if had {
            file.version = FILE_VERSION;
            self.save(&file)?;
        }
        Ok(had)
    }

    fn save(&self, file: &StoreFile) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
        crate::settings_patch::write_atomic(&self.path, &text).map_err(|e| format!("could not save: {e}"))
    }
}

/// The chat config of an agent: its own table when it has one (always wins), else the overlay. `known_label` is
/// the agent's label when the id is a registered agent (`None` = unknown id: no overlay is applied to it).
pub fn resolve_with(store: &OverlayStore, id: &str, table: Option<AgentConfig>, known_label: Option<String>) -> Option<AgentConfig> {
    if table.is_some() {
        return table;
    }
    let label = known_label?;
    let entry = store.get(id)?;
    build_config(&key(id), &label, &entry).ok()
}

/// Whether `agent` is served by the overlay: no table of its own and a usable overlay.
pub fn is_overlay_backed(store: &OverlayStore, id: &str, has_own_table: bool) -> bool {
    !has_own_table && store.get(id).is_some()
}

/// Registered agents served by the overlay (no table of their own, a usable overlay), in registry order.
pub fn overlay_agents(store: &OverlayStore) -> Vec<AgentConfig> {
    let ids = store.ids();
    if ids.is_empty() {
        return Vec::new();
    }
    crate::tui::registry::registry()
        .ordered()
        .iter()
        .filter(|profile| profile.acp_table().is_none() && ids.iter().any(|id| id.eq_ignore_ascii_case(profile.id())))
        .filter_map(|profile| {
            let label = crate::tui::descriptor_for(profile.id()).map(|d| d.label)?;
            resolve_with(store, profile.id(), None, Some(label))
        })
        .collect()
}

/// An agent the person could add chat to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addable {
    pub id: String,
    pub label: String,
    /// Why the screen may want to say something (for example the program is not on this PC); never a blocker.
    pub reason: Option<String>,
}

/// Registered agents that are runnable agents (not plain shells) with no `[acp]` table and no overlay.
pub fn addable_agents(store: &OverlayStore) -> Vec<Addable> {
    crate::tui::registry::registry()
        .ordered()
        .iter()
        .filter(|profile| profile.features().kind != crate::tui::Kind::Shell)
        .filter(|profile| profile.acp_table().is_none() && store.get(profile.id()).is_none())
        .map(|profile| {
            let label = crate::tui::descriptor_for(profile.id()).map(|d| d.label).unwrap_or_else(|| profile.id().to_string());
            let reason = (!profile.is_installed()).then(|| "not found on this PC".to_string());
            Addable { id: profile.id().to_string(), label, reason }
        })
        .collect()
}

/// The program names the help probes may look for, as the agent itself declares them (`version_command`), else its id.
pub fn program_of(id: &str) -> Option<String> {
    let profile = crate::tui::registry::registry().find(id.trim())?;
    let name = profile.version_command().map(|v| v.program).unwrap_or_else(|| profile.id().to_string());
    let file = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string();
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::config::HostEnv;
    use crate::acp::conversations::{AgentSource, Conversations};
    use crate::acp::fake_agent;
    use crate::acp::record::RecordStore;
    use crate::acp::supervisor::{Supervisor, SupervisorConfig};
    use serde_json::json;
    use std::ffi::OsString;
    use std::sync::Arc;
    use std::time::Duration;

    fn temp() -> (OverlayStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-acp-overlay-{}", uuid::Uuid::new_v4()));
        (OverlayStore::at(dir.join(FILE)), dir)
    }

    fn entry(command: &str, args: &[&str]) -> OverlayEntry {
        OverlayEntry { command: command.into(), args: args.iter().map(|a| a.to_string()).collect(), ..OverlayEntry::default() }
    }

    fn table(id: &str) -> AgentConfig {
        AgentConfig { id: id.into(), label: "Own".into(), command: "own-adapter".into(), ..AgentConfig::default() }
    }

    #[test]
    fn the_overlay_answers_only_when_there_is_no_table() {
        let (store, dir) = temp();
        store.put("plain", &entry("plain-acp", &["--stdio"])).unwrap();
        let cfg = resolve_with(&store, "Plain", None, Some("Plain agent".into())).expect("overlay");
        assert_eq!((cfg.id.as_str(), cfg.label.as_str(), cfg.command.as_str()), ("plain", "Plain agent", "plain-acp"));
        assert_eq!(cfg.args, vec!["--stdio".to_string()]);
        // an unknown id gets nothing, even with a file entry
        assert!(resolve_with(&store, "plain", None, None).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_real_table_always_wins() {
        let (store, dir) = temp();
        store.put("plain", &entry("overlay-adapter", &[])).unwrap();
        let cfg = resolve_with(&store, "plain", Some(table("plain")), Some("Plain".into())).unwrap();
        assert_eq!(cfg.command, "own-adapter");
        assert!(!is_overlay_backed(&store, "plain", true));
        assert!(is_overlay_backed(&store, "plain", false));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_invalid_overlay_is_refused_on_write_check_and_ignored_on_read() {
        let (store, dir) = temp();
        // refused by the build step
        assert!(build_config("a", "A", &entry("", &[])).is_err());
        assert!(build_config("a", "A", &entry("node", &["x.js"])).unwrap_err().contains("AGENT_DEF_SHELL_INTERPRETER"));
        assert!(build_config("a", "A", &entry("C:\\Windows\\System32\\cmd.exe", &["/c"])).unwrap_err().contains("AGENT_DEF_SHELL_INTERPRETER"));
        let mut denied = entry("adapter", &[]);
        denied.env_allow = vec!["NODE_OPTIONS".into()];
        assert!(build_config("a", "A", &denied).unwrap_err().contains("AGENT_DEF_DENIED_ENV"));
        let mut bad_resume = entry("adapter", &[]);
        bad_resume.resume_command = vec!["resume".into()];
        assert!(build_config("a", "A", &bad_resume).is_err(), "resume_command needs {{session}}");
        let secret = entry("adapter", &["--token", "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"]);
        assert!(build_config("a", "A", &secret).is_err());
        assert!(OverlayEntry::from_json(&json!({"command": "x", "surprise": 1})).is_err());
        assert!(OverlayEntry::from_json(&json!("text")).is_err());
        // ignored on read: a hand-edited file with a bad entry and a good one
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"version":1,"agents":{"bad":{"command":"bash","args":["-c","x"]},"junk":7,"good":{"command":"adapter","args":[]}}}"#,
        )
        .unwrap();
        assert!(store.get("bad").is_none());
        assert!(store.get("junk").is_none());
        assert!(store.get("good").is_some());
        assert_eq!(store.ids(), vec!["good".to_string()]);
        assert!(resolve_with(&store, "bad", None, Some("Bad".into())).is_none());
        std::fs::write(dir.join(FILE), "{ not json").unwrap();
        assert!(store.get("good").is_none() && store.ids().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn put_and_remove_round_trip() {
        let (store, dir) = temp();
        let mut e = entry("adapter", &["acp", "--stdio"]);
        e.env_allow = vec!["MY_KEY".into()];
        e.resume_command = vec!["--resume".into(), "{session}".into()];
        store.put(" Plain ", &e).unwrap();
        assert_eq!(OverlayStore::at(dir.join(FILE)).get("plain"), Some(e.clone()));
        store.put("other", &entry("adapter2", &[])).unwrap();
        assert!(store.remove("plain").unwrap());
        assert!(!store.remove("plain").unwrap(), "removing twice is not an error");
        assert!(store.get("plain").is_none());
        assert!(store.get("other").is_some(), "the others stay");
        assert!(store.put("  ", &e).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    struct TestHost(PathBuf);
    impl HostEnv for TestHost {
        fn var(&self, name: &str) -> Option<OsString> {
            std::env::var_os(name)
        }
        fn which(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn adapters_dir(&self) -> PathBuf {
            self.0.join("adapters")
        }
        fn homes_dir(&self) -> PathBuf {
            self.0.join("homes")
        }
    }

    /// The registry is replaced by a table of labels: "plain" is a registered agent with no table.
    struct Source {
        store: OverlayStore,
        fake: AgentConfig,
    }

    impl AgentSource for Source {
        fn config(&self, agent: &str) -> Option<Arc<AgentConfig>> {
            let label = (agent == "plain").then(|| "Plain agent".to_string());
            let mut cfg = resolve_with(&self.store, agent, None, label)?;
            // The overlay of this test is a bare name (a path under the home folder looks private to the
            // redactor and is refused); the fake adapter is the test exe, which needs fixed variables.
            assert_eq!(cfg.command, "adapter");
            cfg.command = self.fake.command.clone();
            cfg.args = self.fake.args.clone();
            cfg.env = self.fake.env.clone();
            cfg.pin_policy = crate::acp::config::PinPolicy::Off;
            Some(Arc::new(cfg))
        }
    }

    #[test]
    fn a_chat_starts_from_an_overlay_agent() {
        let (store, dir) = temp();
        std::fs::create_dir_all(&dir).unwrap();
        let fake = fake_agent::agent_config("", &dir.join("agent-sessions"));
        let e = entry("adapter", &["acp"]);
        store.put("plain", &e).unwrap();
        let source = Arc::new(Source { store: store.clone(), fake });
        let host: Arc<dyn HostEnv> = Arc::new(TestHost(dir.clone()));
        let conversations =
            Conversations::new(RecordStore::at(dir.join("conversations.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, source.clone());
        assert!(conversations.create("unregistered", &dir).is_err(), "an unknown agent still has no definition");
        let view = conversations.create("plain", &dir).expect("create from the overlay");
        let session = conversations.session(&view.id).unwrap();
        session.prompt_text("stream 2").unwrap();
        let snap = session.wait_for(|s| s.turn >= 1 && !s.turn_in_flight(), Duration::from_secs(30));
        assert!(snap.is_some(), "the turn did not finish");
        session.close();
        // removing the overlay: the agent is gone for new chats
        store.remove("plain").unwrap();
        assert!(conversations.create("plain", &dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
