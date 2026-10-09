//! Declarative `[mcp_host]` writer (SPEC 5.15, HST-1..3): registers the Puppet Master MCP server in
//! another host's JSON / TOML / YAML config from a definition's `[mcp_host]` table.
//!
//! Only the one key the definition names is touched (HST-2); a `.bak` copy of the existing file
//! is taken before the first change; a second run with the same inputs changes nothing. The config
//! path must stay under the user's home or the project directory (HST-3) and the definition must
//! be approved. TOML and YAML are edited line-wise so comments and unrelated keys survive.

use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::tui::declarative::schema::{McpHostFormat, McpHostTable};

/// What replaces `{command}`, `{args}` and `{env}` in the entry template.
#[derive(Debug, Clone)]
pub struct HostLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl HostLaunch {
    /// The running app's own MCP launch spec.
    pub fn current() -> Self {
        let spec = crate::mcp_runtime::mcp_launch_spec();
        Self {
            command: spec.command,
            args: spec.args,
            env: vec![(
                crate::app_paths::BRIDGE_PORT_FILE_ENV.to_string(),
                crate::app_paths::bridge_port_file_env_value(),
            )],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostWriteResult {
    pub path: PathBuf,
    pub changed: bool,
    pub backup: Option<PathBuf>,
}

pub const PLACEHOLDERS: &[&str] = &["command", "args", "env"];

/// Dotted key path split into non-empty segments.
pub fn key_segments(key_path: &str) -> Result<Vec<&str>, String> {
    let segs: Vec<&str> = key_path.split('.').collect();
    let ok = |s: &&str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !segs.iter().all(ok) {
        return Err(format!("key_path {key_path:?} must be dotted segments of [A-Za-z0-9_-]"));
    }
    Ok(segs)
}

/// `~`-expand `config_path` and enforce HST-3: no `..`, and the result stays under `home` or `project`.
pub fn resolve_config_path(config_path: &str, home: &Path, project: Option<&Path>) -> Result<PathBuf, String> {
    let raw = config_path.trim();
    if raw.is_empty() {
        return Err("config_path is empty".into());
    }
    let expanded = if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else if let Some(p) = project {
        p.join(raw)
    } else {
        return Err(format!("relative config_path {raw:?} needs a project directory"));
    };
    if expanded.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("config_path {raw:?} contains `..` (HST-3)"));
    }
    if within(&expanded, home) || project.is_some_and(|p| within(&expanded, p)) {
        Ok(expanded)
    } else {
        Err(format!("config_path {} is outside the home and project directories (HST-3)", expanded.display()))
    }
}

/// Containment on real paths: the nearest existing ancestor of `path` is canonicalised so a
/// symlinked directory cannot lead out of `root`.
fn within(path: &Path, root: &Path) -> bool {
    let root_real = fs::canonicalize(root).unwrap_or_else(|_| root.components().collect());
    let mut probe = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let real = loop {
        if let Ok(real) = fs::canonicalize(&probe) {
            break Some(real);
        }
        match (probe.file_name().map(|n| n.to_os_string()), probe.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                probe = parent;
            }
            _ => break None,
        }
    };
    let Some(mut real) = real else { return false };
    for part in tail.iter().rev() {
        real.push(part);
    }
    real.starts_with(&root_real)
}

fn substitute(value: &Value, launch: &HostLaunch) -> Value {
    match value {
        Value::String(s) => match s.as_str() {
            "{args}" => json!(launch.args),
            "{env}" => Value::Object(launch.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
            _ => Value::String(s.replace("{command}", &launch.command)),
        },
        Value::Array(items) => Value::Array(
            items
                .iter()
                .flat_map(|item| match item {
                    Value::String(s) if s == "{args}" => launch.args.iter().map(|a| json!(a)).collect(),
                    other => vec![substitute(other, launch)],
                })
                .collect(),
        ),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), substitute(v, launch))).collect()),
        other => other.clone(),
    }
}

/// The entry with placeholders filled in, as JSON.
pub fn render_entry(table: &McpHostTable, launch: &HostLaunch) -> Result<Value, String> {
    let template = serde_json::to_value(&table.entry).map_err(|e| e.to_string())?;
    Ok(substitute(&template, launch))
}

/// Write the entry for `table`. `approved` must be true (HST-3); `.bak` is created before any change.
pub fn write_host_entry(
    table: &McpHostTable,
    launch: &HostLaunch,
    home: &Path,
    project: Option<&Path>,
    approved: bool,
) -> Result<HostWriteResult, String> {
    if !approved {
        return Err("definition is not approved; the [mcp_host] writer will not run (HST-3)".into());
    }
    let segs = key_segments(&table.key_path)?;
    let path = resolve_config_path(&table.config_path, home, project)?;
    let entry = render_entry(table, launch)?;
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    let before = existing.clone().unwrap_or_default();
    let after = match table.format {
        McpHostFormat::Json => edit_json(&before, &segs, &entry)?,
        McpHostFormat::Toml => edit_toml(&before, &segs, &entry)?,
        McpHostFormat::Yaml => edit_yaml(&before, &segs, &entry)?,
    };
    if existing.is_some() && after == before {
        return Ok(HostWriteResult { path, changed: false, backup: None });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let mut backup = None;
    if existing.is_some() {
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        fs::copy(&path, &bak).map_err(|e| format!("could not write {}: {e}", bak.display()))?;
        backup = Some(bak);
    }
    fs::write(&path, after).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(HostWriteResult { path, changed: true, backup })
}

fn edit_json(text: &str, segs: &[&str], entry: &Value) -> Result<String, String> {
    let mut doc: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text).map_err(|e| format!("existing file is not valid JSON, left untouched: {e}"))?
    };
    let mut cur = &mut doc;
    for seg in &segs[..segs.len() - 1] {
        let obj = cur.as_object_mut().ok_or_else(|| format!("parent of `{seg}` is not an object"))?;
        cur = obj.entry(seg.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    let last = segs[segs.len() - 1];
    let obj = cur.as_object_mut().ok_or_else(|| format!("parent of `{last}` is not an object"))?;
    if obj.get(last) == Some(entry) {
        return Ok(text.to_string());
    }
    obj.insert(last.to_string(), entry.clone());
    Ok(serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n")
}

/// Dotted name of a `[table]` header line (not `[[array]]`), quotes stripped.
fn toml_header(line: &str) -> Option<String> {
    let t = line.trim();
    let inner = t.strip_prefix('[')?;
    if inner.starts_with('[') {
        return None;
    }
    let inner = inner.split(']').next()?;
    Some(inner.split('.').map(|s| s.trim().trim_matches('"')).collect::<Vec<_>>().join("."))
}

fn edit_toml(text: &str, segs: &[&str], entry: &Value) -> Result<String, String> {
    if !text.trim().is_empty() {
        text.parse::<toml::Table>().map_err(|e| format!("existing file is not valid TOML, left untouched: {e}"))?;
    }
    let key = segs.join(".");
    let sub = format!("{key}.");
    let mut nested = entry.clone();
    for seg in segs.iter().rev() {
        nested = json!({ *seg: nested });
    }
    let new_table: toml::Table =
        serde_json::from_value(nested).map_err(|e| format!("entry is not TOML-representable: {e}"))?;
    let mut block = toml::to_string(&new_table).map_err(|e| e.to_string())?;
    if !block.ends_with('\n') {
        block.push('\n');
    }
    // Drop the existing sections that belong to the key (the table and its sub-tables).
    let mut kept: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            skipping = toml_header(line).is_some_and(|h| h == key || h.starts_with(&sub));
        }
        if !skipping {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&block);
    // A dotted/inline definition elsewhere would collide: refuse instead of writing a broken file.
    out.parse::<toml::Table>().map_err(|e| {
        format!("result would not be valid TOML (key defined in a form this writer cannot replace): {e}")
    })?;
    // Idempotence: compare after normalising the trailing newline.
    if out.trim_end() == text.trim_end() {
        Ok(text.to_string())
    } else {
        Ok(out)
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// Find `seg:` directly inside the mapping body `lines[start..end]` (children share the first
/// content line's indent). Returns (line index, exclusive end of the key's block).
fn find_child(lines: &[String], start: usize, end: usize, seg: &str) -> Option<(usize, usize)> {
    let child_indent = (start..end).find(|&i| !is_blank_or_comment(&lines[i])).map(|i| indent_of(&lines[i]))?;
    let prefix = format!("{seg}:");
    let i = (start..end).find(|&i| {
        !is_blank_or_comment(&lines[i]) && indent_of(&lines[i]) == child_indent && lines[i].trim_start().starts_with(&prefix)
    })?;
    let mut j = i + 1;
    while j < end && (is_blank_or_comment(&lines[j]) || indent_of(&lines[j]) > child_indent) {
        j += 1;
    }
    while j > i + 1 && is_blank_or_comment(&lines[j - 1]) {
        j -= 1;
    }
    Some((i, j))
}

fn edit_yaml(text: &str, segs: &[&str], entry: &Value) -> Result<String, String> {
    if text.contains('\t') {
        return Err("YAML file contains tabs; left untouched".into());
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let (mut start, mut end) = (0usize, lines.len());
    let mut parent_indent: Option<usize> = None;
    let mut level = 0usize;
    while level < segs.len() {
        let Some((i, j)) = find_child(&lines, start, end, segs[level]) else { break };
        let ind = indent_of(&lines[i]);
        if level == segs.len() - 1 {
            let new = vec![format!("{}{}: {}", " ".repeat(ind), segs[level], entry)];
            if lines[i..j] == new[..] {
                return Ok(text.to_string());
            }
            lines.splice(i..j, new);
            return Ok(lines.join("\n") + "\n");
        }
        if !lines[i].trim_end().ends_with(':') {
            return Err(format!("`{}` is not a plain mapping; left untouched", segs[level]));
        }
        level += 1;
        start = i + 1;
        end = j;
        parent_indent = Some(ind);
    }
    // Insert the missing parents and the entry at the end of the deepest existing mapping.
    let base = parent_indent.map_or(0, |p| p + 2);
    let mut new: Vec<String> = Vec::new();
    for (k, seg) in segs[level..segs.len() - 1].iter().enumerate() {
        new.push(format!("{}{}:", " ".repeat(base + 2 * k), seg));
    }
    let leaf = base + 2 * (segs.len() - 1 - level);
    // JSON flow syntax is valid YAML, so the entry is a one-line flow mapping.
    new.push(format!("{}{}: {}", " ".repeat(leaf), segs[segs.len() - 1], entry));
    let at = if parent_indent.is_some() {
        end
    } else {
        let mut a = lines.len();
        while a > 0 && lines[a - 1].trim().is_empty() {
            a -= 1;
        }
        a
    };
    lines.splice(at..at, new);
    Ok(lines.join("\n") + "\n")
}
