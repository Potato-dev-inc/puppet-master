//! `install = "project_file"` (§5.2): PM's entries in a project-local hook file (Codex
//! `.codex/hooks.json`).
//!
//! Rules:
//! - inside the project only (symlinks resolved, no `..`), never a shared or global file;
//! - **a file git tracks is never written** (`BlockedTracked`): the team shares it, PM stays out;
//! - PM's own groups are found by their command (`render::is_pm_group`); every other group keeps
//!   its place and content, and PM's groups go at the END of each event's list (Codex keys trust by
//!   position, probe P-C2);
//! - a second run with the same inputs writes nothing;
//! - a `.pm-bak` copy is taken before PM first changes a file the user had;
//! - a file PM created is hidden from git in `info/exclude` (the user's `.gitignore` is never touched);
//! - every file PM wrote is listed in `<hooks dir>/installs.json`, so turning hooks off removes them.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::agent_hooks::render;
use crate::tui::declarative::hooks::HooksTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// PM's entries were written (`created`: the file did not exist before).
    Written { created: bool },
    /// Already exactly right: nothing written.
    Unchanged,
    /// Git tracks the file: PM writes nothing.
    BlockedTracked,
    /// The existing file is not JSON PM can read: left alone.
    BlockedUnreadable,
    /// The path would leave the project.
    BlockedOutside,
    Failed(String),
}

impl Outcome {
    pub fn installed(&self) -> bool {
        matches!(self, Outcome::Written { .. } | Outcome::Unchanged)
    }
}

/// Install (or refresh) PM's entries for `agent` in `project`.
pub fn ensure(scheme: &HooksTable, exe: &str, agent: &str, project: &Path, hooks_dir: &Path) -> Outcome {
    let Some(file) = &scheme.project_file else { return Outcome::Failed("no project_file".into()) };
    let Some(target) = contained(project, &file.path) else { return Outcome::BlockedOutside };
    if git_tracks(project, &file.path) {
        return Outcome::BlockedTracked;
    }
    let existed = target.is_file();
    let mut doc: Value = if existed {
        match std::fs::read_to_string(&target).ok().and_then(|t| serde_json::from_str::<Value>(strip_bom(&t)).ok()) {
            Some(v @ Value::Object(_)) => v,
            _ => return Outcome::BlockedUnreadable,
        }
    } else {
        json!({})
    };
    let before = doc.clone();
    merge(&mut doc, &file.root_key, &render::groups(scheme, exe, agent), agent);
    if existed && doc == before {
        return Outcome::Unchanged;
    }
    if existed {
        let bak = target.with_extension("json.pm-bak");
        if !bak.exists() {
            let _ = std::fs::copy(&target, &bak);
        }
    }
    let Ok(text) = serde_json::to_string_pretty(&doc) else { return Outcome::Failed("render".into()) };
    if let Some(dir) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return Outcome::Failed(e.to_string());
        }
    }
    if let Err(e) = std::fs::write(&target, format!("{text}\n")) {
        return Outcome::Failed(e.to_string());
    }
    if !existed && file.exclude_from_git {
        crate::git_exclude::exclude_file(project, &file.path);
    }
    record_install(hooks_dir, project, &file.path, agent, !existed);
    Outcome::Written { created: !existed }
}

/// Put PM's groups (`pm`: event -> groups) into `doc[root_key]`: drop PM's old groups, keep every
/// other group where it is, append PM's at the end of each event's list.
pub fn merge(doc: &mut Value, root_key: &str, pm: &Map<String, Value>, agent: &str) {
    let root = doc.as_object_mut().expect("object").entry(root_key.to_string()).or_insert_with(|| json!({}));
    if !root.is_object() {
        *root = json!({});
    }
    let events = root.as_object_mut().expect("object");
    let names: Vec<String> = events.keys().cloned().collect();
    for name in names {
        if let Some(Value::Array(list)) = events.get_mut(&name) {
            list.retain(|group| !render::is_pm_group(group, agent));
            if list.is_empty() && !pm.contains_key(&name) {
                events.remove(&name);
            }
        }
    }
    for (name, groups) in pm {
        let list = events.entry(name.clone()).or_insert_with(|| json!([]));
        if !list.is_array() {
            *list = json!([]);
        }
        if let (Value::Array(list), Value::Array(groups)) = (list, groups) {
            list.extend(groups.iter().cloned());
        }
    }
}

/// Remove PM's entries for `agent` from one file; delete the file when PM created it and nothing
/// else is left. Never restores a backup over the user's later edits.
pub fn remove(project: &Path, rel: &str, root_key: &str, agent: &str, created_by_pm: bool) -> bool {
    let Some(target) = contained(project, rel) else { return false };
    let Some(mut doc) = std::fs::read_to_string(&target).ok().and_then(|t| serde_json::from_str::<Value>(strip_bom(&t)).ok()) else {
        return false;
    };
    if !doc.is_object() {
        return false;
    }
    let before = doc.clone();
    merge(&mut doc, root_key, &Map::new(), agent);
    let empty_root = doc.get(root_key).and_then(Value::as_object).is_some_and(Map::is_empty);
    if empty_root {
        doc.as_object_mut().expect("object").remove(root_key);
    }
    if created_by_pm && doc.as_object().is_some_and(Map::is_empty) {
        let _ = std::fs::remove_file(&target);
        crate::git_exclude::unexclude_file(project, rel);
        return true;
    }
    if doc == before {
        return false;
    }
    serde_json::to_string_pretty(&doc).ok().is_some_and(|t| std::fs::write(&target, format!("{t}\n")).is_ok())
}

/// `project/rel` when it stays inside `project` (symlinks of existing parents resolved).
pub fn contained(project: &Path, rel: &str) -> Option<PathBuf> {
    let rel_norm = rel.replace('\\', "/");
    if rel_norm.split('/').any(|s| s == "..") || Path::new(rel).is_absolute() || rel_norm.starts_with('~') {
        return None;
    }
    let root = project.canonicalize().ok()?;
    let target = root.join(rel);
    let mut probe = target.parent().map(Path::to_path_buf);
    while let Some(dir) = probe {
        if dir.exists() {
            let real = dir.canonicalize().ok()?;
            return real.starts_with(&root).then_some(target);
        }
        probe = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Does git track `rel` in `project`? Outside a repo, or without git, it does not.
pub fn git_tracks(project: &Path, rel: &str) -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(project).args(["ls-files", "--error-unmatch", "--"]).arg(rel);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.status().is_ok_and(|s| s.success())
}

/// `<hooks dir>/installs.json`: `[{project, file, agent, created}]`.
pub fn installs(hooks_dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(hooks_dir.join("installs.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
}

fn record_install(hooks_dir: &Path, project: &Path, rel: &str, agent: &str, created: bool) {
    let mut list = installs(hooks_dir);
    let project = project.to_string_lossy().into_owned();
    if list.iter().any(|v| v["project"] == project.as_str() && v["file"] == rel && v["agent"] == agent) {
        return;
    }
    list.push(json!({ "project": project, "file": rel, "agent": agent, "created": created }));
    let _ = std::fs::create_dir_all(hooks_dir);
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(hooks_dir.join("installs.json"), text);
    }
}

/// Turning hooks off: remove PM's entries from every recorded project, then forget the list.
pub fn uninstall_all(hooks_dir: &Path, root_key_for: impl Fn(&str) -> Option<String>) -> usize {
    let mut removed = 0;
    for item in installs(hooks_dir) {
        let (Some(project), Some(file), Some(agent)) = (item["project"].as_str(), item["file"].as_str(), item["agent"].as_str()) else {
            continue;
        };
        let Some(root_key) = root_key_for(agent) else { continue };
        if remove(Path::new(project), file, &root_key, agent, item["created"].as_bool() == Some(true)) {
            removed += 1;
        }
    }
    let _ = std::fs::remove_file(hooks_dir.join("installs.json"));
    removed
}
