//! The `[hooks]` table (Hook Lane, DESIGN-HOOK-LANE §2): how an agent CLI reports its own
//! lifecycle to Puppet Master through PM's one fixed hook command.
//!
//! Declarative definitions carry it as `[hooks]`; code profiles embed the same TOML (see
//! `tui/claude/hooks.toml`, `tui/codex/hooks.toml`) and parse it with [`parse_hooks_toml`]. Generic
//! code (`crate::agent_hooks`) reads only this table, never an agent name.
//!
//! Every value set is closed, so a bad value is an error, not a fallback: [`validate_hooks`] returns
//! the `AGENT_DEF_HOOKS_*` codes listed in the design (§2.3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::schema::DefError;

/// The only `schema` value of the table this build understands.
pub const HOOKS_SCHEMA: i64 = 1;

/// Files more than one agent reads (Cursor imports Claude's). PM never writes a hook into one, so
/// a hook meant for one agent can never fire in another. A path rule, not an agent check.
pub const SHARED_HOOK_FILES: &[&str] = &[".claude/settings.json", ".claude/settings.local.json"];

/// PM's closed event set (§2.2) with the fields each one may carry: (kind, required, optional).
pub const PM_EVENTS: &[(&str, &[&str], &[&str])] = &[
    ("session_start", &["session"], &["source", "cwd", "model"]),
    ("prompt_submitted", &["session"], &["turn", "prompt"]),
    ("tool_pre", &["session", "tool"], &["turn", "tool_use", "command", "path", "subagent"]),
    ("tool_post", &["session", "tool"], &["turn", "tool_use", "path", "paths", "subagent"]),
    ("permission_request", &["session", "tool"], &["turn", "command", "path", "description", "subagent"]),
    ("turn_end", &["session"], &["turn", "final_text", "continuing", "subagent"]),
    ("turn_failed", &["session"], &["turn", "error", "final_text"]),
    ("turn_interrupted", &["session"], &["turn"]),
    ("subagent_end", &["session"], &["turn", "subagent"]),
    ("notice", &["session", "kind"], &["message"]),
    ("session_end", &["session"], &["reason"]),
];

/// Entry-template keys whose blocking value would make a PM outage block the agent.
const FAIL_CLOSED_KEYS: &[&str] = &["onFailure", "on_failure", "failClosed", "fail_closed"];

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksTable {
    pub schema: i64,
    /// `launch_flag` (a command-line flag at spawn, no file) or `project_file` (a managed entry in a
    /// project-local file). A user/global file is never an option.
    pub install: String,
    /// `async` (the vendor runs the hook in the background) or `sync`.
    #[serde(default = "default_delivery")]
    pub delivery: String,
    /// Vendor-side backstop written into the vendor config; PM's emitter has its own 300 ms budget.
    #[serde(default = "default_vendor_timeout")]
    pub vendor_timeout_s: u64,
    /// What the user must do once before hooks run: `none`, `folder_trust` or `vendor_review`.
    #[serde(default = "default_trust")]
    pub trust: String,
    /// CLI versions these mappings were captured on, e.g. `>=2.1.299, <=2.1.300`. Empty = untested.
    #[serde(default)]
    pub tested: String,
    /// The vendor JSON key that names the event (`hook_event_name` in Claude and Codex).
    #[serde(default = "default_event_field")]
    pub event_field: String,
    pub launch: Option<HooksLaunch>,
    pub project_file: Option<HooksProjectFile>,
    /// The vendor hook entry template. Placeholders: `{exe}` `{args}` `{exe_quoted}` `{args_joined}`.
    pub entry: toml::Table,
    #[serde(default)]
    pub trust_screen: HooksTrustScreen,
    #[serde(default)]
    pub events: Vec<HookEventRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HooksLayout {
    /// `{root_key: {Event: [{"matcher"?: m, "hooks": [entry]}]}}`
    EventGroups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchValue {
    /// PM writes one JSON file under `<app data>/hooks/` and passes its path.
    File,
    /// The JSON is passed as the argument value.
    Inline,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksLaunch {
    pub flag: String,
    pub value: LaunchValue,
    pub layout: HooksLayout,
    pub root_key: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksProjectFile {
    /// Relative to the project root, e.g. `.codex/hooks.json`.
    pub path: String,
    pub layout: HooksLayout,
    pub root_key: String,
    /// Hide a file PM created from git (`.git/info/exclude`); a file the user had is never hidden.
    #[serde(default)]
    pub exclude_from_git: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksTrustScreen {
    /// Phrases (lowercase substrings) of the vendor's "review hooks" screen; captured live, never guessed.
    #[serde(default)]
    pub pending_phrases: Vec<String>,
    /// One plain sentence: what the user does once to trust PM's hooks.
    #[serde(default)]
    pub how_to: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookEventRow {
    /// The vendor's event name, written into the vendor config.
    pub vendor: String,
    /// PM's event kind (`PM_EVENTS`).
    pub pm: String,
    #[serde(default)]
    pub matcher: String,
    /// Only for `delivery = "sync"`: exact bytes the emitter prints.
    #[serde(default)]
    pub stdout: String,
    /// PM field -> dotted path into the vendor JSON.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

fn default_delivery() -> String {
    "async".into()
}
fn default_vendor_timeout() -> u64 {
    5
}
fn default_trust() -> String {
    "none".into()
}
fn default_event_field() -> String {
    "hook_event_name".into()
}

impl HooksTable {
    /// The row for a vendor event (first match; duplicates are a validation error).
    pub fn row_for(&self, vendor_event: &str) -> Option<&HookEventRow> {
        self.events.iter().find(|row| row.vendor == vendor_event)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HooksFile {
    hooks: HooksTable,
}

/// Parse a code profile's embedded `hooks.toml` (a file whose only table is `[hooks]`) and validate
/// it. `None` on any problem: a broken scheme means no hooks, never a half-installed one.
pub fn parse_hooks_toml(text: &str) -> Option<HooksTable> {
    let parsed: HooksFile = toml::from_str(text).ok()?;
    validate_hooks(&parsed.hooks).is_empty().then_some(parsed.hooks)
}

/// Every problem with a `[hooks]` table (§2.3). Paths are relative to the definition root.
pub fn validate_hooks(h: &HooksTable) -> Vec<DefError> {
    let mut e = Vec::new();
    let mut add = |code: &'static str, path: &str, msg: String| e.push(DefError::new(code, path, msg));

    if h.schema != HOOKS_SCHEMA {
        add("AGENT_DEF_HOOKS_SCHEMA", "hooks.schema", format!("hooks schema {} is not supported; use {HOOKS_SCHEMA}", h.schema));
    }
    match h.install.as_str() {
        "launch_flag" => {
            if h.launch.is_none() || h.project_file.is_some() {
                add("AGENT_DEF_INCONSISTENT", "hooks.launch", "install = \"launch_flag\" needs [hooks.launch] and no [hooks.project_file]".into());
            }
        }
        "project_file" => {
            if h.project_file.is_none() || h.launch.is_some() {
                add("AGENT_DEF_INCONSISTENT", "hooks.project_file", "install = \"project_file\" needs [hooks.project_file] and no [hooks.launch]".into());
            }
        }
        other => add(
            "AGENT_DEF_HOOKS_GLOBAL_CONFIG",
            "hooks.install",
            format!("`{other}` is not allowed: Puppet Master installs hooks only by a launch flag or a project file, never in a user or global config"),
        ),
    }
    if let Some(launch) = &h.launch {
        if !launch.flag.starts_with('-') || launch.flag.contains(char::is_whitespace) {
            add("AGENT_DEF_INCONSISTENT", "hooks.launch.flag", "must be one command-line flag such as --settings".into());
        }
        if !valid_key(&launch.root_key) {
            add("AGENT_DEF_INCONSISTENT", "hooks.launch.root_key", "must be [A-Za-z0-9_-]+".into());
        }
    }
    if let Some(file) = &h.project_file {
        let p = file.path.replace('\\', "/");
        let absolute = p.starts_with('/') || p.starts_with('~') || std::path::Path::new(&file.path).is_absolute() || p.get(1..2) == Some(":");
        if p.trim().is_empty() || absolute || p.split('/').any(|seg| seg == "..") {
            add("AGENT_DEF_HOOKS_GLOBAL_CONFIG", "hooks.project_file.path", "must be a relative path inside the project, without `..`, `~` or a drive".into());
        }
        let lower = p.trim_start_matches("./").to_ascii_lowercase();
        if SHARED_HOOK_FILES.contains(&lower.as_str()) {
            add(
                "AGENT_DEF_HOOKS_SHARED_FILE",
                "hooks.project_file.path",
                format!("`{}` is read by other agents too, so Puppet Master never writes hooks into it", file.path),
            );
        }
        if !valid_key(&file.root_key) {
            add("AGENT_DEF_INCONSISTENT", "hooks.project_file.root_key", "must be [A-Za-z0-9_-]+".into());
        }
    }
    if !valid_path(&h.event_field) {
        add("AGENT_DEF_INCONSISTENT", "hooks.event_field", "must be a dotted path such as hook_event_name".into());
    }
    if !matches!(h.trust.as_str(), "none" | "folder_trust" | "vendor_review") {
        add("AGENT_DEF_INCONSISTENT", "hooks.trust", "must be none, folder_trust or vendor_review".into());
    }
    if h.vendor_timeout_s == 0 || h.vendor_timeout_s > 60 {
        add("AGENT_DEF_INCONSISTENT", "hooks.vendor_timeout_s", "must be 1-60 seconds".into());
    }
    match h.delivery.as_str() {
        "async" => {}
        "sync" => {
            if h.vendor_timeout_s > 10 {
                add("AGENT_DEF_HOOKS_SYNC", "hooks.vendor_timeout_s", "sync delivery allows at most 10 seconds".into());
            }
            for (i, row) in h.events.iter().enumerate() {
                if row.stdout.is_empty() {
                    add("AGENT_DEF_HOOKS_SYNC", &format!("hooks.events.{i}.stdout"), "sync delivery needs the exact stdout of every event".into());
                }
            }
        }
        _ => add("AGENT_DEF_INCONSISTENT", "hooks.delivery", "must be async or sync".into()),
    }
    check_entry(&h.entry, &mut add);
    if h.trust_screen.pending_phrases.len() > 8 || h.trust_screen.how_to.chars().count() > 240 {
        add("AGENT_DEF_TOO_LARGE", "hooks.trust_screen", "at most 8 phrases and a 240-character how_to".into());
    }
    if h.events.is_empty() {
        add("AGENT_DEF_HOOKS_EVENT", "hooks.events", "declare at least one event".into());
    }
    if h.events.len() > 32 {
        add("AGENT_DEF_TOO_LARGE", "hooks.events", "at most 32 events".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for (i, row) in h.events.iter().enumerate() {
        let here = format!("hooks.events.{i}");
        if !valid_vendor_name(&row.vendor) {
            add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.vendor"), "vendor event names are [A-Za-z0-9_]+".into());
        }
        if !seen.insert((row.vendor.clone(), row.matcher.clone())) {
            add("AGENT_DEF_HOOKS_EVENT", &here, format!("duplicate event `{}` with matcher `{}`", row.vendor, row.matcher));
        }
        let Some((_, required, optional)) = PM_EVENTS.iter().find(|(kind, _, _)| *kind == row.pm) else {
            add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.pm"), format!("unknown event kind `{}`", row.pm));
            continue;
        };
        for field in *required {
            if !row.fields.contains_key(*field) {
                add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.fields"), format!("`{}` needs the field `{field}`", row.pm));
            }
        }
        for (field, path) in &row.fields {
            if !required.contains(&field.as_str()) && !optional.contains(&field.as_str()) {
                add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.fields.{field}"), format!("`{}` may not carry `{field}`", row.pm));
            }
            if !valid_path(path) {
                add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.fields.{field}"), "must be a dotted path such as tool_input.command".into());
            }
        }
        if row.matcher.len() > 200 || row.matcher.chars().any(char::is_control) {
            add("AGENT_DEF_HOOKS_EVENT", &format!("{here}.matcher"), "at most 200 characters, no control characters".into());
        }
    }
    e
}

fn check_entry(entry: &toml::Table, add: &mut impl FnMut(&'static str, &str, String)) {
    if entry.is_empty() {
        add("AGENT_DEF_INCONSISTENT", "hooks.entry", "must not be empty".into());
    }
    for key in FAIL_CLOSED_KEYS {
        if let Some(value) = entry.get(*key) {
            let blocks = match value {
                toml::Value::Boolean(b) => *b,
                toml::Value::String(s) => !matches!(s.to_ascii_lowercase().as_str(), "allow" | "ignore" | "continue" | "open"),
                _ => true,
            };
            if blocks {
                add("AGENT_DEF_HOOKS_FAIL_CLOSED", &format!("hooks.entry.{key}"), "a hook that blocks the agent when Puppet Master is down is not allowed".into());
            }
        }
    }
    for (key, value) in entry {
        let toml::Value::String(s) = value else { continue };
        for name in placeholders(s) {
            if !["exe", "args", "exe_quoted", "args_joined"].contains(&name.as_str()) {
                add("AGENT_DEF_BAD_PLACEHOLDER", &format!("hooks.entry.{key}"), format!("unknown placeholder {{{name}}}; allowed: exe, args, exe_quoted, args_joined"));
            }
        }
        if s.contains("{args}") && s != "{args}" {
            add("AGENT_DEF_BAD_PLACEHOLDER", &format!("hooks.entry.{key}"), "{args} must be a whole value (it becomes a list)".into());
        }
    }
}

fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

fn valid_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn valid_vendor_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn valid_path(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.split('.').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

#[cfg(test)]
#[path = "tests/hooks.rs"]
mod tests;
