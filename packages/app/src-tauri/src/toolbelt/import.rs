//! Import: read the MCP servers an agent already has and offer them for the toolbelt.
//!
//! The agent's own entry template (`[mcp_host.entry]` / `entry_http`) is run backwards: a template
//! value `{command}` says where the command sits in the agent's entry, `{args}`, `{env}`, `{url}`
//! and `{headers}` likewise. Entries the toolbelt wrote (`pm-toolbelt-*`) and Puppet Master's own
//! entry are not offered. Secrets are masked in what the UI sees; the apply step reads the real
//! values itself, so they never travel to the UI.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;

use super::edit;
use super::store::{self, redacted, Scope, ToolServer, Transport};
use super::sync::{groups, AgentRef, HostSpec, SyncOptions};
use super::KEY_PREFIX;
use crate::mcp_install::declarative::key_segments;

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// Opaque handle for `apply`.
    pub reference: String,
    pub agents: Vec<AgentRef>,
    /// The server as the UI may see it (secrets masked).
    pub server: ToolServer,
    /// Already in the toolbelt (same command or address).
    pub already: bool,
}

#[derive(Default)]
struct Found {
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    url: Option<String>,
    headers: BTreeMap<String, String>,
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect()
}

fn string_map(v: &Value) -> Option<BTreeMap<String, String>> {
    Some(v.as_object()?.iter().filter_map(|(k, x)| Some((k.clone(), x.as_str()?.to_string()))).collect())
}

/// Walk the template next to the agent's entry and collect what the placeholders point at.
fn extract(template: &Value, entry: &Value, out: &mut Found) {
    match template {
        Value::String(s) => match s.as_str() {
            "{command}" => out.command = out.command.take().or_else(|| entry.as_str().map(str::to_string)),
            "{args}" => out.args = strings(entry).unwrap_or_default(),
            "{env}" => out.env = string_map(entry).unwrap_or_default(),
            "{url}" => out.url = entry.as_str().map(str::to_string),
            "{headers}" => out.headers = string_map(entry).unwrap_or_default(),
            _ => {}
        },
        Value::Array(items) => {
            // ["{command}", "{args}"]: the entry is one array, first element the command.
            if items.first().and_then(Value::as_str) == Some("{command}") && items.get(1).and_then(Value::as_str) == Some("{args}") {
                if let Some(all) = strings(entry) {
                    if let Some((first, rest)) = all.split_first() {
                        out.command = Some(first.clone());
                        out.args = rest.to_vec();
                    }
                }
            }
        }
        Value::Object(map) => {
            for (k, t) in map {
                if let Some(e) = entry.get(k) {
                    extract(t, e, out);
                }
            }
        }
        _ => {}
    }
}

fn to_server(key: &str, entry: &Value, host: &HostSpec, scope: Scope) -> Option<ToolServer> {
    let json_of = |t: &toml::Table| serde_json::to_value(t).unwrap_or(Value::Null);
    let mut http = Found::default();
    if let Some(t) = &host.table.entry_http {
        extract(&json_of(t), entry, &mut http);
    }
    let mut stdio = Found::default();
    extract(&json_of(&host.table.entry), entry, &mut stdio);
    let base = |transport| ToolServer {
        id: String::new(),
        name: key.to_string(),
        transport,
        command: None,
        args: Vec::new(),
        env: BTreeMap::new(),
        url: None,
        headers: BTreeMap::new(),
        enabled: true,
        scope,
    };
    if let Some(command) = stdio.command.filter(|c| !c.is_empty()) {
        return Some(ToolServer { command: Some(command), args: stdio.args, env: stdio.env, ..base(Transport::Stdio) });
    }
    http.url.filter(|u| !u.is_empty()).map(|url| ToolServer { url: Some(url), headers: http.headers, ..base(Transport::Http) })
}

fn same_server(a: &ToolServer, b: &ToolServer) -> bool {
    a.transport == b.transport && a.command == b.command && a.args == b.args && a.url == b.url
}

/// Every importable server found in the agents' configs, with the real values (callers redact).
fn scan(hosts: &[HostSpec], opts: &SyncOptions, existing: &[ToolServer]) -> Vec<(String, Vec<AgentRef>, ToolServer, bool)> {
    let mut out: Vec<(String, Vec<AgentRef>, ToolServer, bool)> = Vec::new();
    for g in groups(hosts, opts) {
        let Ok(path) = &g.path else { continue };
        let Ok(text) = fs::read_to_string(path) else { continue };
        let Ok(segs) = key_segments(&g.table.key_path) else { continue };
        let (parent, last) = segs.split_at(segs.len() - 1);
        let Ok(members) = edit::members(g.table.format, &text, parent) else { continue };
        let host = hosts.iter().find(|h| g.agents.iter().any(|a| a.id == h.agent_id)).expect("group has a host");
        for (key, entry) in members {
            if key.starts_with(KEY_PREFIX) || key == last[0] {
                continue;
            }
            let Some(server) = to_server(&key, &entry, host, g.scope) else { continue };
            let reference = format!("{}|{}|{}", g.scope.as_str(), g.agents[0].id, key);
            if let Some(found) = out.iter_mut().find(|(_, _, s, _)| same_server(s, &server) && s.scope == server.scope) {
                for a in &g.agents {
                    if !found.1.contains(a) {
                        found.1.push(a.clone());
                    }
                }
                continue;
            }
            let already = existing.iter().any(|e| same_server(e, &server));
            out.push((reference, g.agents.clone(), server, already));
        }
    }
    out
}

pub fn preview(hosts: &[HostSpec], opts: &SyncOptions, existing: &[ToolServer]) -> Vec<Candidate> {
    scan(hosts, opts, existing)
        .into_iter()
        .map(|(reference, agents, server, already)| Candidate { reference, agents, server: redacted(&server), already })
        .collect()
}

/// Add the chosen candidates to the toolbelt file; returns the servers added (masked).
pub fn apply(hosts: &[HostSpec], opts: &SyncOptions, store_path: &std::path::Path, references: &[String]) -> Result<Vec<ToolServer>, String> {
    let existing = store::load_servers(store_path)?;
    let mut added = Vec::new();
    for (reference, _, server, already) in scan(hosts, opts, &existing) {
        if already || !references.contains(&reference) {
            continue;
        }
        added.push(store::add(store_path, server)?);
    }
    Ok(added)
}
