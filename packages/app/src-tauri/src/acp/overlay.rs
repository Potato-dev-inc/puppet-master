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
//! Entries are tagged by `source` (files without it are `command`, so old files load unchanged):
//! * `command`: the person confirmed the exact command line of a program that is already installed;
//! * `registry`: the app installed an adapter or ACP-speaking build listed in the official ACP registry
//!   (`acp::registry`, `acp::install_registry`). The spawn spec is built by the app (node on the installed script, or
//!   the unpacked program), the file's SHA-256 is pinned (trust on first use) and checked before every start;
//! * `bridge`: the built-in bridge (`acp::bridge`): the app's own executable as the adapter (`acp-bridge --agent <id>`),
//!   driving the agent's non-interactive mode. Nothing is downloaded; the entry is `{source: "bridge"}` and is built
//!   fresh on every read, so an agent that lost its headless mode drops out by itself.
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

/// How a registry entry was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryKind {
    Npx,
    Binary,
}

/// `source: "registry"`: what the app installed for an agent from the official ACP registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryEntry {
    pub registry_id: String,
    pub version: String,
    pub kind: RegistryKind,
    /// npx: the npm package that was installed (exact version in `version`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// binary: the https address of the archive that was installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    /// The file that runs, relative to the adapters folder, with `/`.
    pub installed_entry: String,
    /// SHA-256 of that file when it was installed; checked before every start.
    pub entry_sha256: String,
    pub args: Vec<String>,
    pub env_allow: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resume_command: Vec<String>,
}

impl Default for RegistryEntry {
    fn default() -> Self {
        Self {
            registry_id: String::new(),
            version: String::new(),
            kind: RegistryKind::Npx,
            package: None,
            archive: None,
            installed_entry: String::new(),
            entry_sha256: String::new(),
            args: Vec::new(),
            env_allow: Vec::new(),
            resume_command: Vec::new(),
        }
    }
}

/// `source: "bridge"`: the built-in ACP bridge for an agent that has neither ACP in its own program nor a registry
/// adapter. `raw` is kept as written (so extra fields of a newer build are not lost on a rewrite); the spawn spec is
/// never read from it: it is built by the app from the agent id (`acp::bridge::bridge_table_parts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeEntry {
    pub raw: Value,
}

/// One stored entry, tagged by `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayItem {
    Command(OverlayEntry),
    Registry(RegistryEntry),
    Bridge(BridgeEntry),
}

impl OverlayItem {
    /// Reads one stored value. No `source` means `command`.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let Some(map) = value.as_object() else { return Err("INVALID_ARGUMENT: an overlay entry must be an object".into()) };
        match map.get("source").map(|v| v.as_str()) {
            None | Some(Some("command")) => {
                let mut rest = map.clone();
                rest.remove("source");
                OverlayEntry::from_json(&Value::Object(rest)).map(OverlayItem::Command)
            }
            Some(Some("registry")) => serde_json::from_value::<RegistryEntry>(value.clone()).map(OverlayItem::Registry).map_err(|e| format!("INVALID_ARGUMENT: registry entry: {e}")),
            Some(Some("bridge")) => Ok(OverlayItem::Bridge(BridgeEntry { raw: value.clone() })),
            _ => Err("INVALID_ARGUMENT: unknown overlay source".into()),
        }
    }

    pub fn to_value(&self) -> Result<Value, String> {
        let (mut value, source) = match self {
            OverlayItem::Command(e) => (serde_json::to_value(e), "command"),
            OverlayItem::Registry(e) => (serde_json::to_value(e), "registry"),
            OverlayItem::Bridge(_) => return Ok(serde_json::json!({ "source": "bridge" })),
        };
        if let Ok(Value::Object(map)) = &mut value {
            map.insert("source".into(), Value::String(source.into()));
        }
        value.map_err(|e| e.to_string())
    }

    pub fn source(&self) -> &'static str {
        match self {
            OverlayItem::Command(_) => "command",
            OverlayItem::Registry(_) => "registry",
            OverlayItem::Bridge(_) => "bridge",
        }
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

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The spawn spec of a registry install, built by the app (not typed by the person), then held to the table's own
/// checks and VAL-5; VAL-4 (no interpreter) does not apply to it: for `npx` the app itself runs `node` on the
/// installed script. The pin makes `acp::pin::enforce` compare the file's SHA-256 before every start.
pub fn registry_config(id: &str, label: &str, e: &RegistryEntry) -> Result<AgentConfig, String> {
    let bad = |what: &str| Err(format!("INVALID_ARGUMENT: registry entry: {what}"));
    if e.registry_id.trim().is_empty() || e.version.trim().is_empty() {
        return bad("registry_id and version are needed");
    }
    if !super::registry::relative_safe(&e.installed_entry) || e.installed_entry.contains([':', '\\']) {
        return bad("installed_entry must be a relative path inside the add-on folder");
    }
    if !hex64(&e.entry_sha256) {
        return bad("entry_sha256 must be a SHA-256");
    }
    let template = format!("{{adapters}}/{}", e.installed_entry);
    let (command, mut args, package) = match e.kind {
        RegistryKind::Npx => {
            let package = e.package.clone().filter(|p| super::install::valid_package_name(p)).ok_or("INVALID_ARGUMENT: registry entry: package is not a package name")?;
            ("node".to_string(), vec![template.clone()], package)
        }
        RegistryKind::Binary => (template.clone(), Vec::new(), super::install_registry::marker_name(&e.registry_id)),
    };
    args.extend(e.args.iter().cloned());
    let cfg = AgentConfig {
        command,
        args,
        env_allow: e.env_allow.clone(),
        resume_command: e.resume_command.clone(),
        pins: vec![super::config::AdapterPin { package, version: e.version.clone(), entry: template, sha256: Some(e.entry_sha256.to_ascii_lowercase()) }],
        pin_policy: super::config::PinPolicy::Enforce,
        ..AgentConfig::default()
    };
    // Arguments came from outside: no secret-looking text is stored or passed.
    for text in cfg.args.iter().chain(&cfg.resume_command) {
        if crate::redact::text(text).into_inner() != *text {
            return Err("INVALID_ARGUMENT: registry entry holds text that looks like a secret; it is never stored".into());
        }
    }
    if let Some((code, path, message)) = crate::tui::declarative::validate::acp_table_errors(&cfg, true).into_iter().next() {
        return Err(format!("{code}: {path}: {message}"));
    }
    Ok(AgentConfig { id: id.to_string(), label: label.to_string(), ..cfg })
}

/// The config of the built-in bridge for agent `id`: the app's own executable with `acp-bridge --agent <id>`. The
/// command line is built by the app, not typed by the person, so VAL-4 (no interpreter) does not apply; the other
/// table checks still do. Refused (so the entry is ignored on read) when the agent has no headless mode any more.
///
/// * `env_allow` is empty: the host passes only the base environment (`BASE_ENV_ALLOW`), which is what the bridge
///   process needs to start; the agent's own headless env rules are applied inside the bridge.
/// * no `resume_command`: a bridge chat cannot be handed over to the agent's terminal (its sessions are the
///   agent's headless runs, not an ACP session the terminal knows).
/// * quirks: the defaults fit. The bridge answers `cancelled` itself after a stop, and a turn the person stopped is
///   reported as cancelled whatever the adapter says; prompts are serial (the bridge runs one turn at a time).
/// * no pins and `PinPolicy::Off`: there is no adapter file to pin (the adapter is the app itself); the pin check
///   answers "nothing pinned" for an empty list under any policy, and `Off` states that plainly. Entries of other
///   sources keep their own policy.
pub fn bridge_config(id: &str, label: &str) -> Result<AgentConfig, String> {
    super::bridge::available_for(id)?;
    let (command, args) = super::bridge::bridge_table_parts(id)?;
    let cfg = AgentConfig { command, args, pin_policy: super::config::PinPolicy::Off, ..AgentConfig::default() };
    if let Some((code, path, message)) = crate::tui::declarative::validate::acp_table_errors(&cfg, true).into_iter().next() {
        return Err(format!("{code}: {path}: {message}"));
    }
    Ok(AgentConfig { id: id.to_string(), label: label.to_string(), ..cfg })
}

/// The config of any stored item.
pub fn build_item_config(id: &str, label: &str, item: &OverlayItem) -> Result<AgentConfig, String> {
    match item {
        OverlayItem::Command(entry) => build_config(id, label, entry),
        OverlayItem::Registry(entry) => registry_config(id, label, entry),
        OverlayItem::Bridge(_) => bridge_config(id, label),
    }
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

    /// The stored item of `agent`; a missing file, a broken file, an unreadable entry, one that no longer passes
    /// the checks or a bridge entry whose agent has no headless mode any more is `None`.
    pub fn get_item(&self, agent: &str) -> Option<OverlayItem> {
        let id = key(agent);
        let item = OverlayItem::from_value(self.read().agents.get(&id)?).ok()?;
        build_item_config(&id, &id, &item).ok().map(|_| item)
    }

    /// The stored entry as it is written, without checking that it can still start (a broken entry still names the
    /// add-on folder it installed, which Remove needs).
    pub fn stored_item(&self, agent: &str) -> Option<OverlayItem> {
        OverlayItem::from_value(self.read().agents.get(&key(agent))?).ok()
    }

    /// The stored command entry of `agent` (`source: "command"`, also an entry without `source`).
    pub fn get(&self, agent: &str) -> Option<OverlayEntry> {
        match self.get_item(agent)? {
            OverlayItem::Command(entry) => Some(entry),
            _ => None,
        }
    }

    /// Ids that have a usable overlay.
    pub fn ids(&self) -> Vec<String> {
        self.read().agents.keys().filter(|id| self.get_item(id).is_some()).cloned().collect()
    }

    /// Writes one agent's command entry. The caller has checked it with [`build_config`].
    pub fn put(&self, agent: &str, entry: &OverlayEntry) -> Result<(), String> {
        self.put_item(agent, &OverlayItem::Command(entry.clone()))
    }

    /// Writes one agent's item (any source). The caller has checked it with [`build_item_config`].
    pub fn put_item(&self, agent: &str, item: &OverlayItem) -> Result<(), String> {
        let id = key(agent);
        if id.is_empty() {
            return Err("INVALID_ARGUMENT: agent id is required".into());
        }
        let value = item.to_value()?;
        let _guard = WRITE.lock();
        let mut file = self.read();
        file.version = FILE_VERSION;
        file.agents.insert(id, value);
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
    let item = store.get_item(id)?;
    build_item_config(&key(id), &label, &item).ok()
}

/// Whether `agent` is served by the overlay: no table of its own and a usable overlay.
pub fn is_overlay_backed(store: &OverlayStore, id: &str, has_own_table: bool) -> bool {
    !has_own_table && store.get_item(id).is_some()
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
        .filter(|profile| profile.acp_table().is_none() && store.get_item(profile.id()).is_none())
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

    fn registry_entry(kind: RegistryKind) -> RegistryEntry {
        RegistryEntry {
            registry_id: "demo-acp".into(),
            version: "1.2.3".into(),
            kind,
            package: (kind == RegistryKind::Npx).then(|| "@scope/demo-acp".to_string()),
            archive: (kind == RegistryKind::Binary).then(|| "https://example.test/a.zip".to_string()),
            installed_entry: "reg-demo-acp/node_modules/@scope/demo-acp/dist/index.js".into(),
            entry_sha256: "ab".repeat(32),
            args: vec!["--acp".into()],
            ..RegistryEntry::default()
        }
    }

    #[test]
    fn files_without_a_source_load_as_command_and_new_writes_are_tagged() {
        let (store, dir) = temp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"version":1,"agents":{"old":{"command":"adapter","args":["acp"],"env_allow":[]},"tagged":{"source":"command","command":"adapter2"}}}"#,
        )
        .unwrap();
        assert_eq!(store.get("old").unwrap().args, vec!["acp".to_string()]);
        assert_eq!(store.get_item("old").unwrap().source(), "command");
        assert_eq!(store.get("tagged").unwrap().command, "adapter2");
        store.put("new", &entry("adapter3", &[])).unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.contains("\"source\": \"command\""), "{text}");
        assert_eq!(store.get("new").unwrap().command, "adapter3");
        // the other entries survive a write
        assert!(store.get("old").is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_registry_entry_round_trips_and_builds_a_pinned_spawn_spec() {
        let (store, dir) = temp();
        for kind in [RegistryKind::Npx, RegistryKind::Binary] {
            let e = registry_entry(kind);
            let cfg = registry_config("plain", "Plain", &e).unwrap();
            assert_eq!(cfg.pin_policy, crate::acp::config::PinPolicy::Enforce);
            assert_eq!(cfg.pins[0].sha256.as_deref(), Some(e.entry_sha256.as_str()));
            match kind {
                RegistryKind::Npx => {
                    // node is the app's own choice here: VAL-4 does not apply to a registry entry
                    assert_eq!((cfg.command.as_str(), cfg.args[0].as_str()), ("node", "{adapters}/reg-demo-acp/node_modules/@scope/demo-acp/dist/index.js"));
                    assert_eq!(cfg.pins[0].package, "@scope/demo-acp");
                }
                RegistryKind::Binary => assert!(cfg.command.starts_with("{adapters}/") && cfg.pins[0].package == "pm-registry-demo-acp"),
            }
            store.put_item("plain", &OverlayItem::Registry(e.clone())).unwrap();
            assert_eq!(store.get_item("plain"), Some(OverlayItem::Registry(e)));
            assert!(store.get("plain").is_none(), "get() is the command entry only");
            assert!(is_overlay_backed(&store, "plain", false));
            assert!(resolve_with(&store, "plain", None, Some("Plain".into())).is_some());
        }
        // the same typed program is refused as a command entry (VAL-4 still holds there)
        assert!(build_config("a", "A", &entry("node", &["x.js"])).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_registry_entry_that_is_unsafe_is_refused_and_ignored() {
        let good = registry_entry(RegistryKind::Npx);
        let mutate = |f: &dyn Fn(&mut RegistryEntry)| {
            let mut e = good.clone();
            f(&mut e);
            registry_config("a", "A", &e)
        };
        assert!(mutate(&|e| e.installed_entry = "../../outside.js".into()).is_err());
        assert!(mutate(&|e| e.installed_entry = "/abs/outside.js".into()).is_err());
        assert!(mutate(&|e| e.installed_entry = "C:/outside.js".into()).is_err());
        assert!(mutate(&|e| e.installed_entry = "a\\b.js".into()).is_err());
        assert!(mutate(&|e| e.entry_sha256 = "zz".into()).is_err());
        assert!(mutate(&|e| e.package = Some("not a package!".into())).is_err());
        assert!(mutate(&|e| e.version = " ".into()).is_err());
        assert!(mutate(&|e| e.env_allow = vec!["NODE_OPTIONS".into()]).unwrap_err().contains("AGENT_DEF_DENIED_ENV"));
        assert!(mutate(&|e| e.args = vec!["--token".into(), "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into()]).is_err());
        // on disk: a tampered file is ignored, the good entry beside it is kept
        let (store, dir) = temp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"version":1,"agents":{"bad":{"source":"registry","registry_id":"x","version":"1","kind":"npx","package":"p","installed_entry":"../x.js","entry_sha256":"00"},"mystery":{"source":"future"},"ok":{"command":"adapter"}}}"#,
        )
        .unwrap();
        assert!(store.get_item("bad").is_none() && store.get_item("mystery").is_none());
        assert_eq!(store.ids(), vec!["ok".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_bridge_source_builds_the_app_exe_as_the_adapter_and_is_ignored_without_headless() {
        let (store, dir) = temp();
        std::fs::create_dir_all(&dir).unwrap();
        let headless = crate::tui::registered_agent_ids().into_iter().find(|id| crate::acp::bridge::available_for(id).is_ok()).expect("an agent with a headless mode");
        let none = crate::tui::registered_agent_ids().into_iter().find(|id| crate::acp::bridge::available_for(id).is_err());
        let mut agents = serde_json::Map::new();
        agents.insert(headless.clone(), json!({"source": "bridge", "future": 1}));
        if let Some(none) = &none {
            agents.insert(none.clone(), json!({"source": "bridge"}));
        }
        agents.insert("ok".into(), json!({"command": "adapter"}));
        std::fs::write(dir.join(FILE), serde_json::to_string(&json!({"version": 1, "agents": agents})).unwrap()).unwrap();
        let item = store.get_item(&headless).expect("bridge entry usable");
        assert_eq!(item.source(), "bridge");
        let cfg = build_item_config(&headless, "H", &item).unwrap();
        assert_eq!(std::path::PathBuf::from(&cfg.command), std::env::current_exe().unwrap());
        assert_eq!(cfg.args, vec!["acp-bridge".to_string(), "--agent".to_string(), headless.clone()]);
        assert!(cfg.env_allow.is_empty() && cfg.resume_command.is_empty() && cfg.pins.is_empty());
        assert_eq!(cfg.pin_policy, crate::acp::config::PinPolicy::Off);
        assert!(store.get(&headless).is_none(), "get() is the command entry only");
        assert!(is_overlay_backed(&store, &headless, false));
        assert!(resolve_with(&store, &headless, None, Some("H".into())).is_some());
        if let Some(none) = &none {
            assert!(store.get_item(none).is_none() && resolve_with(&store, none, None, Some("N".into())).is_none(), "no headless mode: ignored");
        }
        // writing another agent keeps the bridge entry readable; the entry is written as {source: bridge}
        store.put("other", &entry("adapter", &[])).unwrap();
        assert!(store.get_item(&headless).is_some());
        store.put_item(&headless, &OverlayItem::Bridge(BridgeEntry { raw: json!({"source": "bridge"}) })).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(after["agents"][&headless], json!({"source": "bridge"}));
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
