//! The sync engine: make every agent's MCP config match the toolbelt.
//!
//! For each agent that declares an MCP config (`TuiProfile::mcp_host`) the engine computes the
//! entries it should own (Puppet Master itself when asked, plus every enabled toolbelt server in
//! that scope), compares them with what the file has under `pm-toolbelt-*` and adds, updates or
//! removes exactly those keys. The user's own entries are never read for change or touched.
//! `preview` computes the same plan without writing; `apply` writes it.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::edit::{self, Format, Op};
use super::store::{is_secret_key, Scope, ToolServer, Transport, MASK};
use super::KEY_PREFIX;
use crate::mcp_install::declarative::{key_segments, resolve_config_path, substitute, HostLaunch};
use crate::tui::declarative::schema::{McpHostFormat, McpHostTable};

/// One agent with a declared MCP config.
#[derive(Debug, Clone)]
pub struct HostSpec {
    pub agent_id: String,
    pub label: String,
    pub installed: bool,
    pub table: McpHostTable,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentRef {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Unsupported {
    pub id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub home: PathBuf,
    pub project: Option<PathBuf>,
    /// Also keep Puppet Master's own entry (under the key the agent declares) in the global config.
    pub pm_launch: Option<HostLaunch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Add,
    Update,
    Remove,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// The key in the agent's file.
    pub key: String,
    /// Toolbelt server name (or "Puppet Master").
    pub name: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostReport {
    pub agents: Vec<AgentRef>,
    pub scope: Scope,
    pub path: String,
    pub format: String,
    /// `ready` (changes pending / written), `unchanged`, `skipped`, `error`.
    pub status: &'static str,
    pub reason: Option<String>,
    pub changes: Vec<Change>,
    pub notes: Vec<String>,
    /// Changed lines only, secrets masked.
    pub diff: String,
    pub written: bool,
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub dry_run: bool,
    pub hosts: Vec<HostReport>,
    pub unsupported: Vec<Unsupported>,
}

/// Agents that declare an MCP config and agents that do not (shells are not agents here).
pub fn collect_hosts() -> (Vec<HostSpec>, Vec<Unsupported>) {
    let mut hosts = Vec::new();
    let mut unsupported = Vec::new();
    for d in crate::tui::descriptors() {
        if crate::tui::is_shell(d.id) {
            continue;
        }
        let profile = crate::tui::profile_for(d.id);
        match profile.mcp_host() {
            Some(table) => hosts.push(HostSpec { agent_id: d.id.to_string(), label: d.label.clone(), installed: d.installed, table }),
            None => unsupported.push(Unsupported {
                id: d.id.to_string(),
                label: d.label.clone(),
                reason: "This agent doesn't declare where it keeps MCP servers, so Puppet Master can't write to it.".into(),
            }),
        }
    }
    (hosts, unsupported)
}

/// Agents sharing one config file, in one scope.
pub(super) struct Group {
    pub agents: Vec<AgentRef>,
    pub installed: bool,
    pub scope: Scope,
    pub path: Result<PathBuf, String>,
    pub table: McpHostTable,
}

pub(super) fn groups(hosts: &[HostSpec], opts: &SyncOptions) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for h in hosts {
        let mut scopes = vec![(Scope::Global, h.table.config_path.clone())];
        if let (Some(rel), Some(_)) = (&h.table.project_config_path, &opts.project) {
            scopes.push((Scope::Project, rel.clone()));
        }
        for (scope, raw) in scopes {
            let path = resolve_config_path(&raw, &opts.home, opts.project.as_deref());
            let agent = AgentRef { id: h.agent_id.clone(), label: h.label.clone() };
            let same = |g: &Group| g.scope == scope && matches!((&g.path, &path), (Ok(a), Ok(b)) if a == b);
            match out.iter_mut().find(|g| same(g)) {
                Some(g) => {
                    g.installed |= h.installed;
                    g.agents.push(agent);
                }
                None => out.push(Group { agents: vec![agent], installed: h.installed, scope, path, table: h.table.clone() }),
            }
        }
    }
    out
}

fn json_of(t: &toml::Table) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// The agent-specific entry for one toolbelt server, or why it can't be written.
pub fn render_server(table: &McpHostTable, server: &ToolServer) -> Result<Value, String> {
    let (template, launch) = match server.transport {
        Transport::Stdio => (
            &table.entry,
            HostLaunch {
                command: server.command.clone().unwrap_or_default(),
                args: server.args.clone(),
                env: server.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                ..Default::default()
            },
        ),
        Transport::Http => match &table.entry_http {
            Some(t) => (
                t,
                HostLaunch {
                    url: server.url.clone(),
                    headers: server.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    ..Default::default()
                },
            ),
            None => return Err("this agent can't take servers that run at a web address".into()),
        },
    };
    Ok(substitute(&json_of(template), &launch))
}

fn managed_key(server: &ToolServer) -> String {
    format!("{KEY_PREFIX}{}", server.id)
}

fn strip_prefix_name(key: &str, servers: &[ToolServer]) -> String {
    let id = key.strip_prefix(KEY_PREFIX).unwrap_or(key);
    servers.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string())
}

fn secret_strings(servers: &[ToolServer], existing: &[(String, Value)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in servers {
        for (k, v) in s.env.iter().chain(s.headers.iter()) {
            if is_secret_key(k) && v.len() >= 3 {
                out.push(v.clone());
            }
        }
    }
    for (_, v) in existing {
        for field in ["env", "environment", "headers", "http_headers"] {
            if let Some(map) = v.get(field).and_then(Value::as_object) {
                out.extend(map.values().filter_map(Value::as_str).filter(|s| s.len() >= 3).map(str::to_string));
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out.dedup();
    out
}

/// Lines that differ (common head and tail dropped), prefixed `-` / `+`, with secrets masked.
fn diff_lines(before: &str, after: &str, secrets: &[String]) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let mut out = Vec::new();
    for l in &a[head..a.len() - tail] {
        out.push(format!("- {l}"));
    }
    for l in &b[head..b.len() - tail] {
        out.push(format!("+ {l}"));
    }
    let mut text = out.join("\n");
    for s in secrets {
        text = text.replace(s.as_str(), MASK);
    }
    text
}

fn format_name(f: McpHostFormat) -> &'static str {
    match f {
        McpHostFormat::Json => "json",
        McpHostFormat::Toml => "toml",
        McpHostFormat::Yaml => "yaml",
    }
}

fn write_with_backup(path: &Path, existed: bool, text: &str) -> Result<Option<PathBuf>, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let mut backup = None;
    if existed {
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        if !bak.exists() {
            fs::copy(path, &bak).map_err(|e| format!("could not write {}: {e}", bak.display()))?;
            backup = Some(bak);
        }
    }
    let tmp = PathBuf::from(format!("{}.pm-tmp", path.display()));
    fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(backup)
}

fn plan_group(g: &Group, servers: &[ToolServer], opts: &SyncOptions, apply: bool) -> HostReport {
    let table = &g.table;
    let mut report = HostReport {
        agents: g.agents.clone(),
        scope: g.scope,
        path: g.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        format: format_name(table.format).into(),
        status: "unchanged",
        reason: None,
        changes: Vec::new(),
        notes: Vec::new(),
        diff: String::new(),
        written: false,
        backup: None,
    };
    let fail = |mut r: HostReport, why: String| {
        r.status = "error";
        r.reason = Some(why);
        r
    };
    let path = match &g.path {
        Ok(p) => p.clone(),
        Err(e) => return fail(report, e.clone()),
    };
    let segs = match key_segments(&table.key_path) {
        Ok(s) => s,
        Err(e) => return fail(report, e),
    };
    let (parent, pm_key) = segs.split_at(segs.len() - 1);
    let pm_key = pm_key[0];
    let format: Format = table.format;

    let existing_text = match fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return fail(report, format!("could not read {}: {e}", path.display())),
    };
    if g.scope == Scope::Global && existing_text.is_none() && !g.installed {
        report.status = "skipped";
        report.reason = Some("Not installed on this computer, and it has no config file yet.".into());
        return report;
    }
    let before = existing_text.clone().unwrap_or_default();
    let existing = match edit::members(format, &before, parent) {
        Ok(m) => m,
        Err(e) => return fail(report, format!("{e}; left untouched")),
    };
    let existing_map: BTreeMap<&str, &Value> = existing.iter().map(|(k, v)| (k.as_str(), v)).collect();

    // Desired entries, in order.
    let mut desired: Vec<(String, String, Value)> = Vec::new();
    if g.scope == Scope::Global {
        if let Some(launch) = &opts.pm_launch {
            desired.push((pm_key.to_string(), "Puppet Master".into(), crate::mcp_install::declarative::render_entry(table, launch).unwrap_or(Value::Null)));
        }
    }
    for s in servers.iter().filter(|s| s.enabled && s.scope == g.scope) {
        match render_server(table, s) {
            Ok(v) => desired.push((managed_key(s), s.name.clone(), v)),
            Err(why) => report.notes.push(format!("{} was skipped: {why}.", s.name)),
        }
    }

    let mut ops: Vec<Op> = Vec::new();
    for (key, name, value) in &desired {
        let kind = match existing_map.get(key.as_str()) {
            None => ChangeKind::Add,
            Some(cur) if *cur == value => ChangeKind::Unchanged,
            Some(_) => ChangeKind::Update,
        };
        if kind != ChangeKind::Unchanged {
            ops.push(Op::Set { key: key.clone(), value: value.clone() });
        }
        report.changes.push(Change { key: key.clone(), name: name.clone(), kind });
    }
    for (key, _) in &existing {
        if key.starts_with(KEY_PREFIX) && !desired.iter().any(|(k, _, _)| k == key) {
            ops.push(Op::Remove { key: key.clone() });
            report.changes.push(Change { key: key.clone(), name: strip_prefix_name(key, servers), kind: ChangeKind::Remove });
        }
    }
    if ops.is_empty() {
        return report;
    }

    let after = match edit::apply(format, &before, parent, &ops) {
        Ok(t) => t,
        Err(e) => return fail(report, e),
    };
    if after == before {
        return report;
    }
    report.status = "ready";
    report.diff = diff_lines(&before, &after, &secret_strings(servers, &existing));
    if apply {
        match write_with_backup(&path, existing_text.is_some(), &after) {
            Ok(backup) => {
                report.written = true;
                report.backup = backup.map(|b| b.display().to_string());
            }
            Err(e) => return fail(report, e),
        }
    }
    report
}

/// Plan (and, with `apply`, write) the sync. `servers` are the real, unredacted toolbelt entries.
pub fn run(servers: &[ToolServer], hosts: &[HostSpec], unsupported: Vec<Unsupported>, opts: &SyncOptions, apply: bool) -> SyncReport {
    let reports = groups(hosts, opts).iter().map(|g| plan_group(g, servers, opts, apply)).collect();
    SyncReport { dry_run: !apply, hosts: reports, unsupported }
}

/// Convenience for callers that want the live agent registry.
pub fn run_live(servers: &[ToolServer], opts: &SyncOptions, apply: bool) -> SyncReport {
    let (hosts, unsupported) = collect_hosts();
    run(servers, &hosts, unsupported, opts, apply)
}
