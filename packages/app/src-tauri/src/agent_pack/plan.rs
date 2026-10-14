//! Preview / apply / undo of the pack for each detected agent.
//!
//! Two things are installed per agent, each only when the agent declares where it lives:
//!
//! - the skills (`TuiProfile::skills_host`): `<skills dir>/<skill>/SKILL.md`, four small folders;
//! - Puppet Master's own MCP entry (`TuiProfile::mcp_host`): added ONLY when the agent's config has
//!   no entry under that key yet. An entry that is already there is the user's (or the app's
//!   earlier registration) and is never rewritten.
//!
//! Agents that read the same folder or the same config file share one write.

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::host::SkillsHostTable;
use super::record::{backups_dir, sha_hex, Entry, Record};
use super::{assets, MARKER_PREFIX};
use crate::mcp_install::declarative::{key_segments, render_entry, resolve_config_path, HostLaunch};
use crate::toolbelt::edit::{self, Op};
use crate::tui::declarative::schema::McpHostTable;

/// One agent and what it declares.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub mcp: Option<McpHostTable>,
    pub skills: Option<SkillsHostTable>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub home: PathBuf,
    /// Where the install record and the backups live (the app data folder).
    pub state_dir: PathBuf,
    /// Launch spec for Puppet Master's own MCP entry.
    pub launch: HostLaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    /// The file does not exist yet.
    Add,
    /// An older copy of the pack file (it carries the marker).
    Update,
    /// A different file at that path that is not ours; it is backed up first.
    Replace,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub name: String,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillsStep {
    /// `ready` (changes pending / written), `unchanged`, `skipped`, `error`.
    pub status: &'static str,
    pub path: String,
    /// Other agents that read the same folder.
    pub shared_with: Vec<String>,
    pub files: Vec<FileChange>,
    pub reason: Option<String>,
    pub written: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpStep {
    pub status: &'static str,
    pub path: String,
    pub format: String,
    pub shared_with: Vec<String>,
    /// `add` or `unchanged` (an entry is already there).
    pub change: FileKind,
    pub reason: Option<String>,
    /// Changed lines only.
    pub diff: String,
    pub written: bool,
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRow {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub skills: Option<SkillsStep>,
    pub mcp: Option<McpStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Unsupported {
    pub id: String,
    pub label: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackReport {
    pub dry_run: bool,
    pub agents: Vec<AgentRow>,
    pub unsupported: Vec<Unsupported>,
    /// How many files and config entries an Undo would put back.
    pub recorded: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct UndoReport {
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    pub skipped: Vec<Skipped>,
}

// ---------------------------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------------------------

struct FilePlan {
    name: String,
    path: PathBuf,
    kind: FileKind,
    text: String,
    existing: Option<String>,
}

struct SkillGroup {
    agents: Vec<usize>,
    installed: bool,
    dir: Result<PathBuf, String>,
    plans: Vec<FilePlan>,
    status: &'static str,
    reason: Option<String>,
    written: bool,
}

struct McpGroup {
    agents: Vec<usize>,
    installed: bool,
    path: Result<PathBuf, String>,
    table: McpHostTable,
    before: Option<String>,
    after: String,
    change: FileKind,
    status: &'static str,
    reason: Option<String>,
    diff: String,
    written: bool,
    backup: Option<String>,
}

fn is_ours(text: &str) -> bool {
    text.lines().any(|l| l.contains(MARKER_PREFIX))
}

fn plan_skill_group(dir: Result<PathBuf, String>, installed: bool, agents: Vec<usize>) -> SkillGroup {
    let mut group = SkillGroup { agents, installed, dir: dir.clone(), plans: Vec::new(), status: "unchanged", reason: None, written: false };
    let dir = match dir {
        Ok(d) => d,
        Err(e) => {
            group.status = "error";
            group.reason = Some(e);
            return group;
        }
    };
    if !installed && !dir.exists() {
        group.status = "skipped";
        group.reason = Some("Not installed on this computer.".into());
        return group;
    }
    for (name, text) in assets::files() {
        let path = dir.join(&name);
        let existing = match fs::read_to_string(&path) {
            Ok(t) => Some(t.replace("\r\n", "\n")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                group.status = "error";
                group.reason = Some(format!("could not read {}: {e}", path.display()));
                return group;
            }
        };
        let kind = match &existing {
            None => FileKind::Add,
            Some(t) if *t == text => FileKind::Unchanged,
            Some(t) if is_ours(t) => FileKind::Update,
            Some(_) => FileKind::Replace,
        };
        group.plans.push(FilePlan { name, path, kind, text, existing });
    }
    if group.plans.iter().any(|p| p.kind != FileKind::Unchanged) {
        group.status = "ready";
    }
    group
}

fn diff_lines(before: &str, after: &str) -> String {
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
    out.join("\n")
}

fn plan_mcp_group(table: McpHostTable, path: Result<PathBuf, String>, installed: bool, agents: Vec<usize>, launch: &HostLaunch) -> McpGroup {
    let mut g = McpGroup {
        agents,
        installed,
        path: path.clone(),
        table: table.clone(),
        before: None,
        after: String::new(),
        change: FileKind::Unchanged,
        status: "unchanged",
        reason: None,
        diff: String::new(),
        written: false,
        backup: None,
    };
    let fail = |mut g: McpGroup, why: String| {
        g.status = "error";
        g.reason = Some(why);
        g
    };
    let path = match path {
        Ok(p) => p,
        Err(e) => return fail(g, e),
    };
    let segs = match key_segments(&table.key_path) {
        Ok(s) => s,
        Err(e) => return fail(g, e),
    };
    let (parent, key) = segs.split_at(segs.len() - 1);
    let existing_text = match fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return fail(g, format!("could not read {}: {e}", path.display())),
    };
    if existing_text.is_none() && !installed {
        g.status = "skipped";
        g.reason = Some("Not installed on this computer, and it has no config file yet.".into());
        return g;
    }
    let before = existing_text.clone().unwrap_or_default();
    let members = match edit::members(table.format, &before, parent) {
        Ok(m) => m,
        Err(e) => return fail(g, format!("{e}; left untouched")),
    };
    g.before = existing_text;
    if members.iter().any(|(k, _)| k == key[0]) {
        return g; // already registered: not ours to rewrite
    }
    let value = match render_entry(&table, launch) {
        Ok(v) => v,
        Err(e) => return fail(g, e),
    };
    let after = match edit::apply(table.format, &before, parent, &[Op::Set { key: key[0].to_string(), value }]) {
        Ok(t) => t,
        Err(e) => return fail(g, e),
    };
    if after == before {
        return g;
    }
    g.change = FileKind::Add;
    g.status = "ready";
    g.diff = diff_lines(&before, &after);
    g.after = after;
    g
}

fn labels(specs: &[AgentSpec], idx: &[usize], except: usize) -> Vec<String> {
    idx.iter().filter(|&&i| i != except).map(|&i| specs[i].label.clone()).collect()
}

struct Plan {
    skills: Vec<SkillGroup>,
    mcp: Vec<McpGroup>,
}

fn build(specs: &[AgentSpec], opts: &Options) -> Plan {
    let mut skills: Vec<SkillGroup> = Vec::new();
    let mut mcp: Vec<McpGroup> = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        if let Some(h) = &s.skills {
            let dir = resolve_config_path(&h.dir, &opts.home, None);
            match skills.iter_mut().find(|g| matches!((&g.dir, &dir), (Ok(a), Ok(b)) if a == b)) {
                Some(g) => {
                    g.agents.push(i);
                    g.installed |= s.installed;
                }
                None => skills.push(plan_skill_group(dir, s.installed, vec![i])),
            }
        }
        if let Some(t) = &s.mcp {
            let path = resolve_config_path(&t.config_path, &opts.home, None);
            match mcp.iter_mut().find(|g| matches!((&g.path, &path), (Ok(a), Ok(b)) if a == b)) {
                Some(g) => {
                    g.agents.push(i);
                    g.installed |= s.installed;
                }
                None => mcp.push(plan_mcp_group(t.clone(), path, s.installed, vec![i], &opts.launch)),
            }
        }
    }
    // `installed` of a shared group can change after the first agent: re-plan skipped groups.
    for g in skills.iter_mut() {
        if g.status == "skipped" && g.installed {
            let dir = g.dir.clone();
            *g = plan_skill_group(dir, true, std::mem::take(&mut g.agents));
        }
    }
    for g in mcp.iter_mut() {
        if g.status == "skipped" && g.installed {
            *g = plan_mcp_group(g.table.clone(), g.path.clone(), true, std::mem::take(&mut g.agents), &opts.launch);
        }
    }
    Plan { skills, mcp }
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

/// Directories above `path` that do not exist yet, shallowest first.
fn missing_dirs(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = path.parent();
    while let Some(dir) = cur {
        if dir.exists() {
            break;
        }
        out.push(dir.to_path_buf());
        cur = dir.parent();
    }
    out.reverse();
    out
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let tmp = PathBuf::from(format!("{}.pm-tmp", path.display()));
    fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn backup_of(state_dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let dir = backups_dir(state_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let name = format!("{}.bak", &sha_hex(&path.display().to_string())[..16]);
    let dest = dir.join(name);
    fs::copy(path, &dest).map_err(|e| format!("could not back up {}: {e}", path.display()))?;
    Ok(dest)
}

fn path_str(p: &Path) -> String {
    p.display().to_string()
}

fn apply_skill_group(g: &mut SkillGroup, opts: &Options, rec: &mut Record) {
    for p in g.plans.iter().filter(|p| p.kind != FileKind::Unchanged) {
        let probe = Entry::File { path: path_str(&p.path), created: false, backup: None, created_dirs: vec![], written_sha: String::new() };
        let known = rec.find(&probe).is_some();
        let created_dirs = if p.existing.is_none() { missing_dirs(&p.path) } else { Vec::new() };
        let backup = if p.kind == FileKind::Replace && !known {
            match backup_of(&opts.state_dir, &p.path) {
                Ok(b) => Some(path_str(&b)),
                Err(e) => {
                    g.status = "error";
                    g.reason = Some(e);
                    return;
                }
            }
        } else {
            None
        };
        if let Err(e) = write_atomic(&p.path, &p.text) {
            g.status = "error";
            g.reason = Some(e);
            return;
        }
        rec.upsert(Entry::File {
            path: path_str(&p.path),
            created: p.existing.is_none(),
            backup,
            created_dirs: created_dirs.iter().map(|d| path_str(d)).collect(),
            written_sha: sha_hex(&p.text),
        });
        let _ = rec.save(&opts.state_dir);
    }
    g.written = true;
}

fn apply_mcp_group(g: &mut McpGroup, opts: &Options, rec: &mut Record) {
    let Ok(path) = g.path.clone() else { return };
    let Ok(segs) = key_segments(&g.table.key_path) else { return };
    let existed = g.before.is_some();
    let created_dirs = if existed { Vec::new() } else { missing_dirs(&path) };
    if existed {
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        if !bak.exists() {
            match fs::copy(&path, &bak) {
                Ok(_) => g.backup = Some(path_str(&bak)),
                Err(e) => {
                    g.status = "error";
                    g.reason = Some(format!("could not write {}: {e}", bak.display()));
                    return;
                }
            }
        }
    }
    if let Err(e) = write_atomic(&path, &g.after) {
        g.status = "error";
        g.reason = Some(e);
        return;
    }
    let written = render_entry(&g.table, &opts.launch).unwrap_or(Value::Null);
    rec.upsert(Entry::Mcp {
        path: path_str(&path),
        format: format!("{:?}", g.table.format).to_lowercase(),
        key_path: segs.join("."),
        file_created: !existed,
        created_dirs: created_dirs.iter().map(|d| path_str(d)).collect(),
        backup: g.backup.clone(),
        written,
        written_file_sha: sha_hex(&g.after),
    });
    let _ = rec.save(&opts.state_dir);
    g.written = true;
}

// ---------------------------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------------------------

/// Plan the pack for every agent in `specs`; with `apply`, write it and record what changed.
pub fn run(specs: &[AgentSpec], opts: &Options, apply: bool) -> Result<PackReport, String> {
    let mut plan = build(specs, opts);
    let mut rec = Record::load(&opts.state_dir)?;
    if apply {
        for g in plan.skills.iter_mut().filter(|g| g.status == "ready") {
            apply_skill_group(g, opts, &mut rec);
        }
        for g in plan.mcp.iter_mut().filter(|g| g.status == "ready") {
            apply_mcp_group(g, opts, &mut rec);
        }
    }
    Ok(report(specs, &plan, &rec, !apply))
}

fn report(specs: &[AgentSpec], plan: &Plan, rec: &Record, dry_run: bool) -> PackReport {
    let mut agents = Vec::new();
    let mut unsupported = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        let skills = plan.skills.iter().find(|g| g.agents.contains(&i)).map(|g| SkillsStep {
            status: g.status,
            path: g.dir.as_ref().map(|p| path_str(p)).unwrap_or_default(),
            shared_with: labels(specs, &g.agents, i),
            files: g.plans.iter().map(|p| FileChange { name: p.name.clone(), kind: p.kind }).collect(),
            reason: g.reason.clone(),
            written: g.written,
        });
        let mcp = plan.mcp.iter().find(|g| g.agents.contains(&i)).map(|g| McpStep {
            status: g.status,
            path: g.path.as_ref().map(|p| path_str(p)).unwrap_or_default(),
            format: format!("{:?}", g.table.format).to_lowercase(),
            shared_with: labels(specs, &g.agents, i),
            change: g.change,
            reason: g.reason.clone(),
            diff: g.diff.clone(),
            written: g.written,
            backup: g.backup.clone(),
        });
        if skills.is_none() && mcp.is_none() {
            unsupported.push(Unsupported { id: s.id.clone(), label: s.label.clone(), installed: s.installed });
            continue;
        }
        agents.push(AgentRow { id: s.id.clone(), label: s.label.clone(), installed: s.installed, skills, mcp });
    }
    PackReport { dry_run, agents, unsupported, recorded: rec.entries.len() }
}

/// Remove directories apply created, deepest first, each only if it is empty by now.
fn remove_dirs_if_empty(mut dirs: Vec<String>) {
    dirs.sort_by_key(|d| std::cmp::Reverse(Path::new(d).components().count()));
    dirs.dedup();
    for d in dirs {
        let _ = fs::remove_dir(d); // fails (and is ignored) when something else lives there
    }
}

fn format_of(name: &str) -> crate::tui::declarative::schema::McpHostFormat {
    use crate::tui::declarative::schema::McpHostFormat as F;
    match name {
        "json" => F::Json,
        "toml" => F::Toml,
        _ => F::Yaml,
    }
}

fn skip(report: &mut UndoReport, path: &str, reason: impl Into<String>) {
    report.skipped.push(Skipped { path: path.to_string(), reason: reason.into() });
}

/// Put back everything `apply` changed. A file or entry that changed since apply is left alone.
pub fn undo(opts: &Options) -> Result<UndoReport, String> {
    let mut rec = Record::load(&opts.state_dir)?;
    let mut report = UndoReport::default();
    let mut dirs: Vec<String> = Vec::new();
    for entry in rec.entries.clone() {
        let handled = match &entry {
            Entry::File { path, created, backup, created_dirs, written_sha } => {
                undo_file(&mut report, path, *created, backup.as_deref(), created_dirs, written_sha, &mut dirs)
            }
            Entry::Mcp { path, format, key_path, file_created, created_dirs, backup, written, written_file_sha } => undo_mcp(
                &mut report,
                (path, format, key_path),
                (*file_created, created_dirs, backup.as_deref()),
                (written, written_file_sha),
                &mut dirs,
            ),
        };
        if handled {
            rec.remove(&entry);
        }
    }
    remove_dirs_if_empty(dirs);
    rec.save(&opts.state_dir)?;
    Ok(report)
}

/// True when the entry is dealt with (done, gone or deliberately left); false keeps it for a retry.
fn undo_file(report: &mut UndoReport, path: &str, created: bool, backup: Option<&str>, created_dirs: &[String], written_sha: &str, dirs: &mut Vec<String>) -> bool {
    let p = PathBuf::from(path);
    let Ok(current) = fs::read_to_string(&p) else { return true }; // already gone
    if sha_hex(&current) != written_sha {
        skip(report, path, "You changed this file after it was installed, so it was left as it is.");
        return true;
    }
    match (created, backup) {
        (false, Some(b)) => match fs::copy(b, &p) {
            Ok(_) => {
                let _ = fs::remove_file(b);
                report.restored.push(path.to_string());
                true
            }
            Err(e) => {
                skip(report, path, format!("could not restore the original: {e}"));
                false
            }
        },
        _ => match fs::remove_file(&p) {
            Ok(()) => {
                dirs.extend(created_dirs.iter().cloned());
                report.removed.push(path.to_string());
                true
            }
            Err(e) => {
                skip(report, path, format!("could not remove it: {e}"));
                false
            }
        },
    }
}

fn undo_mcp(
    report: &mut UndoReport,
    (path, format, key_path): (&str, &str, &str),
    (file_created, created_dirs, backup): (bool, &[String], Option<&str>),
    (written, written_file_sha): (&Value, &str),
    dirs: &mut Vec<String>,
) -> bool {
    let p = PathBuf::from(path);
    let Ok(current) = fs::read_to_string(&p) else { return true };
    let untouched = sha_hex(&current) == written_file_sha;
    if file_created && untouched {
        return match fs::remove_file(&p) {
            Ok(()) => {
                dirs.extend(created_dirs.iter().cloned());
                report.removed.push(path.to_string());
                true
            }
            Err(e) => {
                skip(report, path, format!("could not remove it: {e}"));
                false
            }
        };
    }
    if untouched {
        if let Some(b) = backup.filter(|b| Path::new(b).exists()) {
            // Nothing else changed the file since apply: put the original back exactly.
            return match fs::copy(b, &p) {
                Ok(_) => {
                    let _ = fs::remove_file(b);
                    report.restored.push(path.to_string());
                    true
                }
                Err(e) => {
                    skip(report, path, format!("could not restore the original: {e}"));
                    false
                }
            };
        }
    }
    // The file changed since: take out only our key, and only if it is still what we wrote.
    let fmt = format_of(format);
    let segs: Vec<&str> = key_path.split('.').collect();
    let (parent, key) = segs.split_at(segs.len() - 1);
    let members = edit::members(fmt, &current, parent).unwrap_or_default();
    match members.iter().find(|(k, _)| k == key[0]) {
        None => {} // the entry is already gone
        Some((_, v)) if v == written => {
            match edit::apply(fmt, &current, parent, &[Op::Remove { key: key[0].to_string() }]).and_then(|t| write_atomic(&p, &t)) {
                Ok(()) => report.removed.push(format!("{path} ({key_path})")),
                Err(e) => {
                    skip(report, path, e);
                    return false;
                }
            }
        }
        Some(_) => skip(report, path, format!("{key_path} was changed after it was added, so it was left as it is.")),
    }
    true
}

/// How many recorded changes an Undo would handle.
pub fn recorded(state_dir: &Path) -> usize {
    Record::load(state_dir).map(|r| r.entries.len()).unwrap_or(0)
}
