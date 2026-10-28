//! The checks a crew passes before it is saved or started. Like the wizard's deny list, this is pure
//! text checking, and it never names an agent: whether an agent type exists comes from the caller
//! (the live registry, or a fake in tests).
//!
//! A task is typed to an agent as a prompt; it is never run through a shell. Even so, it must be
//! plain text: no control characters, a bounded length, and `{folder}` as its only placeholder.

use super::model::{Crew, CrewError, Role, DEFAULT_AGENT, MAX_ROLES, PLACEHOLDER};

pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_ROLE_NAME_CHARS: usize = 40;
pub const MAX_TASK_CHARS: usize = 4000;
const MAX_AGENT_CHARS: usize = 64;

/// What the registry says about an agent type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Ready,
    Unknown,
    /// Registered, but it cannot be started as a crew member (a plain shell, say); the reason.
    NotDispatchable(String),
}

/// Trim the names the way the sheet shows them. Applied before `validate` on every save.
pub fn normalized(crew: &Crew) -> Crew {
    Crew {
        name: crew.name.trim().to_string(),
        roles: crew
            .roles
            .iter()
            .map(|r| Role {
                name: r.name.trim().to_string(),
                agent_type: r.agent_type.trim().to_string(),
                task: r.task.trim().to_string(),
                parent: r.parent.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
            })
            .collect(),
    }
}

/// Every problem of `crew` (empty = fine).
pub fn validate(crew: &Crew, agent_state: &dyn Fn(&str) -> AgentState) -> Vec<CrewError> {
    let mut out = Vec::new();
    if let Some(why) = bad_name(&crew.name, MAX_NAME_CHARS) {
        out.push(CrewError::new("CREW_NAME", format!("The crew name {why}.")));
    }
    if crew.roles.is_empty() {
        out.push(CrewError::new("CREW_NO_ROLES", "A crew needs at least one role."));
    }
    if crew.roles.len() > MAX_ROLES {
        out.push(CrewError::new("CREW_TOO_MANY_ROLES", format!("A crew holds at most {MAX_ROLES} roles; this one has {}.", crew.roles.len())));
    }
    for (i, role) in crew.roles.iter().enumerate() {
        check_role(role, &mut out, agent_state);
        let earlier = crew.roles[..i].iter().any(|r| r.name.eq_ignore_ascii_case(&role.name));
        if earlier && !role.name.is_empty() {
            out.push(CrewError::for_role("CREW_DUPLICATE_ROLE", &role.name, format!("Two roles are called \"{}\". Give each role its own name.", role.name)));
        }
    }
    check_tree(crew, &mut out);
    out
}

fn check_role(role: &Role, out: &mut Vec<CrewError>, agent_state: &dyn Fn(&str) -> AgentState) {
    if let Some(why) = bad_name(&role.name, MAX_ROLE_NAME_CHARS) {
        out.push(CrewError::for_role("CREW_ROLE_NAME", &role.name, format!("A role name {why}.")));
    }
    // The default agent has no registry entry until the crew starts (`agents::resolve_defaults`).
    if role.agent_type != DEFAULT_AGENT {
        check_agent(role, out, agent_state);
    }
    if let Some(why) = bad_task(&role.task) {
        out.push(CrewError::for_role("CREW_TASK", &role.name, format!("The task of {} {why}.", role.name)));
    }
}

fn check_agent(role: &Role, out: &mut Vec<CrewError>, agent_state: &dyn Fn(&str) -> AgentState) {
    if !plain_agent_id(&role.agent_type) {
        out.push(CrewError::for_role("CREW_AGENT_UNKNOWN", &role.name, format!("\"{}\" is not an agent id.", role.agent_type)));
        return;
    }
    match agent_state(&role.agent_type) {
        AgentState::Ready => {}
        AgentState::Unknown => out.push(CrewError::for_role(
            "CREW_AGENT_UNKNOWN",
            &role.name,
            format!("The agent \"{}\" for {} is not set up here.", role.agent_type, role.name),
        )),
        AgentState::NotDispatchable(why) => out.push(CrewError::for_role("CREW_AGENT_NOT_DISPATCHABLE", &role.name, format!("{} cannot take a task: {why}", role.name))),
    }
}

/// One root, every parent a role of the crew, no loops.
fn check_tree(crew: &Crew, out: &mut Vec<CrewError>) {
    let index = |name: &str| crew.roles.iter().position(|r| r.name.eq_ignore_ascii_case(name));
    let mut roots = 0;
    for role in &crew.roles {
        match role.parent.as_deref() {
            None => roots += 1,
            Some(parent) if index(parent).is_none() => out.push(CrewError::for_role(
                "CREW_PARENT_UNKNOWN",
                &role.name,
                format!("{} is under \"{parent}\", which is not a role of this crew.", role.name),
            )),
            Some(_) => {}
        }
    }
    if !crew.roles.is_empty() && roots != 1 {
        out.push(CrewError::new("CREW_ROOTS", format!("A crew needs exactly one lead (a role with no parent); this one has {roots}.")));
    }
    for (i, role) in crew.roles.iter().enumerate() {
        if loops(crew, i, &index) {
            out.push(CrewError::for_role("CREW_CYCLE", &role.name, format!("{} is its own parent, directly or through other roles.", role.name)));
        }
    }
}

/// Walking up from `start` comes back to `start`.
fn loops(crew: &Crew, start: usize, index: &dyn Fn(&str) -> Option<usize>) -> bool {
    let mut at = start;
    for _ in 0..=crew.roles.len() {
        let Some(parent) = crew.roles[at].parent.as_deref().and_then(index) else { return false };
        if parent == start {
            return true;
        }
        at = parent;
    }
    false
}

fn bad_name(name: &str, max: usize) -> Option<&'static str> {
    let t = name.trim();
    if t.is_empty() {
        Some("cannot be empty")
    } else if t.chars().count() > max {
        Some("is too long")
    } else if t.chars().any(char::is_control) {
        Some("has characters that cannot be shown")
    } else {
        None
    }
}

/// Agent ids are plain: letters, digits, `_`, `-`, `.`.
fn plain_agent_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_AGENT_CHARS && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn bad_task(task: &str) -> Option<&'static str> {
    let t = task.trim();
    if t.is_empty() {
        return Some("is empty");
    }
    if t.chars().count() > MAX_TASK_CHARS {
        return Some("is too long");
    }
    if t.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Some("has characters that cannot be shown");
    }
    if unknown_placeholder(t).is_some() {
        return Some("uses a placeholder other than {folder}");
    }
    None
}

/// The first `{word}` that is not `{folder}`. Braces with other things inside (code, JSON) are text.
fn unknown_placeholder(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(len) = text[i + 1..].find('}') {
                let word = &text[i + 1..i + 1 + len];
                let plain = !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if plain && word.as_bytes()[0].is_ascii_alphabetic() && format!("{{{word}}}") != PLACEHOLDER {
                    return Some(word);
                }
            }
        }
        i += 1;
    }
    None
}

/// `{folder}` filled in.
pub fn fill_folder(task: &str, folder: &str) -> String {
    task.replace(PLACEHOLDER, folder)
}
