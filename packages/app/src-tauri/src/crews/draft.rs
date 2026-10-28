//! "Save this team": a live pane and the helpers under it become a draft crew. Each pane is a role:
//! its agent type, its first task as the template (the folder it worked in becomes `{folder}`, then
//! the text goes through redaction, because a crew file may be shared), and its parent's role.
//! The person edits the draft in a sheet before anything is saved.

use super::model::{Crew, CrewError, Role, MAX_ROLES, PLACEHOLDER};
use super::validate::{MAX_ROLE_NAME_CHARS, MAX_TASK_CHARS};

/// One live pane, as far as a draft needs it.
#[derive(Debug, Clone)]
pub struct TreePane {
    pub id: String,
    pub agent_type: String,
    pub title: Option<String>,
    /// Folders this pane's text may name as "the project": its working folder, and the project a copy
    /// came from.
    pub folders: Vec<String>,
    pub parent: Option<String>,
}

/// What `draft` needs from outside: redaction, an agent's display label, and the task of a pane's first
/// run (`None` for a pane nobody gave a task through a run).
pub struct Env<'a> {
    pub first_task_of: &'a dyn Fn(&str) -> Option<String>,
    pub redact: &'a dyn Fn(&str) -> String,
    pub label_of: &'a dyn Fn(&str) -> String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Draft {
    pub crew: Crew,
    /// Panes left out because a crew holds at most `MAX_ROLES` roles.
    pub omitted: usize,
}

pub fn draft(panes: &[TreePane], root_id: &str, env: &Env<'_>) -> Result<Draft, CrewError> {
    let root = panes
        .iter()
        .find(|p| p.id == root_id)
        .ok_or_else(|| CrewError::new("CREW_PANE_GONE", "That agent's pane is closed."))?;
    let (tree, omitted) = collect(panes, root_id);
    let folders: Vec<String> = root.folders.clone();
    let mut roles: Vec<Role> = Vec::new();
    let mut names: Vec<(String, String)> = Vec::new(); // (pane id, role name)
    for pane in &tree {
        let name = unique_name(&role_name(pane, env), &roles);
        let parent = pane.parent.as_ref().filter(|_| pane.id != root_id).and_then(|p| names.iter().find(|(id, _)| id == p)).map(|(_, n)| n.clone());
        let own: Vec<String> = pane.folders.iter().chain(folders.iter()).cloned().collect();
        roles.push(Role { name: name.clone(), agent_type: pane.agent_type.clone(), task: task_template((env.first_task_of)(&pane.id).as_deref(), &own, env), parent });
        names.push((pane.id.clone(), name));
    }
    Ok(Draft { crew: Crew { name: crew_name(&roles), roles }, omitted })
}

/// The root and everything under it, breadth first, at most `MAX_ROLES`; and how many were left out.
fn collect<'a>(panes: &'a [TreePane], root_id: &str) -> (Vec<&'a TreePane>, usize) {
    let mut seen: Vec<&str> = vec![root_id];
    let mut order: Vec<&TreePane> = panes.iter().filter(|p| p.id == root_id).collect();
    let mut at = 0;
    while at < order.len() {
        let id = order[at].id.as_str();
        for child in panes.iter().filter(|p| p.parent.as_deref() == Some(id)) {
            if !seen.contains(&child.id.as_str()) {
                seen.push(&child.id);
                order.push(child);
            }
        }
        at += 1;
    }
    let omitted = order.len().saturating_sub(MAX_ROLES);
    order.truncate(MAX_ROLES);
    (order, omitted)
}

fn role_name(pane: &TreePane, env: &Env<'_>) -> String {
    let title = pane.title.as_deref().map(|t| (env.redact)(t)).map(|t| clean_line(&t)).unwrap_or_default();
    let name = if title.is_empty() { (env.label_of)(&pane.agent_type) } else { title };
    name.chars().take(MAX_ROLE_NAME_CHARS - 3).collect::<String>().trim().to_string()
}

fn clean_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unique_name(base: &str, taken: &[Role]) -> String {
    let base = if base.is_empty() { "Agent" } else { base };
    let mut name = base.to_string();
    let mut n = 2;
    while taken.iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
        name = format!("{base} {n}");
        n += 1;
    }
    name
}

fn crew_name(roles: &[Role]) -> String {
    match roles.first() {
        Some(root) => format!("{} team", root.name).chars().take(60).collect(),
        None => "My crew".into(),
    }
}

/// The task with each folder replaced by `{folder}`, redacted, and cut to the task limit.
fn task_template(task: Option<&str>, folders: &[String], env: &Env<'_>) -> String {
    let Some(task) = task else { return String::new() };
    let mut text = task.to_string();
    for variant in variants(folders) {
        text = replace_folder(&text, &variant);
    }
    let redacted = (env.redact)(&text);
    redacted.trim().chars().take(MAX_TASK_CHARS).collect()
}

/// Each folder as written, and with the other slash; longest first so a folder inside another folder
/// is not cut in half.
fn variants(folders: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for folder in folders {
        let f = folder.trim().trim_end_matches(['\\', '/']);
        if f.len() < 3 {
            continue; // `C:` or `/` would match far too much
        }
        for v in [f.to_string(), f.replace('\\', "/"), f.replace('/', "\\")] {
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.len()));
    out
}

/// Replace `folder` (ignoring ASCII case) where it stands alone: not followed by a letter, digit,
/// `_` or `-` (so `C:\work\shop2` is not read as `C:\work\shop` + `2`).
fn replace_folder(text: &str, folder: &str) -> String {
    let (hay, needle) = (text.to_ascii_lowercase(), folder.to_ascii_lowercase());
    let mut out = String::new();
    let mut from = 0;
    while let Some(found) = hay[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        let glued = text[end..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        out.push_str(&text[from..start]);
        out.push_str(if glued { &text[start..end] } else { PLACEHOLDER });
        from = end;
    }
    out.push_str(&text[from..]);
    out
}
