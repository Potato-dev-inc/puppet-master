//! "Start crew": the order roles start in, and the loop that starts them one at a time. The loop only
//! knows a `Starter` (the real one is `live::LiveStarter`), so the order and the stop-on-failure rule
//! are tested with a fake. A started pane is a normal pane: whatever question its agent asks (trust,
//! permission) waits as a card and is never answered here.

use super::model::{Crew, CrewError, Role};
use super::validate::fill_folder;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Mutex;

/// Slugs of the crews that are starting right now. A start can take minutes (one role at a time), so
/// a second click on the same crew must not start it again: it is refused with `CREW_STARTING`.
static STARTING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Holds the "starting" flag of one crew slug while it starts; the flag is released when it drops.
#[derive(Debug)]
pub struct StartGuard {
    slug: String,
}

impl StartGuard {
    pub fn claim(slug: &str) -> Result<Self, CrewError> {
        let mut starting = STARTING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !starting.insert(slug.to_string()) {
            return Err(CrewError::new("CREW_STARTING", "This crew is already starting."));
        }
        Ok(Self { slug: slug.to_string() })
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        STARTING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&self.slug);
    }
}

/// One role about to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub role: Role,
    /// The task with `{folder}` filled in.
    pub task: String,
    pub folder: String,
    /// The pane the role's parent got when it started (`None` for the root).
    pub parent_pane: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartFailure {
    pub code: String,
    pub message: String,
}

pub trait Starter {
    /// Start the pane; returns its id.
    fn start(&mut self, step: &Step) -> Result<String, StartFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Started {
    pub role: String,
    pub agent_type: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failed {
    pub role: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub started: Vec<Started>,
    pub failed: Option<Failed>,
    /// Roles that were not tried because an earlier one failed.
    pub not_started: Vec<String>,
}

/// Role indexes, parents before children: the root first, then each level in the order of the file.
/// Roles that cannot be reached from a root (a bad file) are left out; `validate` refuses those.
pub fn start_order(crew: &Crew) -> Vec<usize> {
    let at = |name: &str| crew.roles.iter().position(|r| r.name.eq_ignore_ascii_case(name));
    let mut order: Vec<usize> = crew.roles.iter().enumerate().filter(|(_, r)| r.parent.is_none()).map(|(i, _)| i).collect();
    let mut next = 0;
    while next < order.len() {
        let me = order[next];
        for (i, role) in crew.roles.iter().enumerate() {
            let child_of_me = role.parent.as_deref().and_then(at) == Some(me);
            if child_of_me && !order.contains(&i) {
                order.push(i);
            }
        }
        next += 1;
    }
    order
}

/// Start the crew in `folder`, one role at a time. The first failure stops the run: the roles after it
/// are listed as not started, and the panes already started stay open.
pub fn run(crew: &Crew, folder: &str, starter: &mut dyn Starter) -> Outcome {
    let order = start_order(crew);
    let mut out = Outcome { started: Vec::new(), failed: None, not_started: Vec::new() };
    let mut panes: Vec<Option<String>> = vec![None; crew.roles.len()];
    for (n, &i) in order.iter().enumerate() {
        let role = &crew.roles[i];
        let parent_pane = role
            .parent
            .as_deref()
            .and_then(|p| crew.roles.iter().position(|r| r.name.eq_ignore_ascii_case(p)))
            .and_then(|p| panes[p].clone());
        let step = Step { role: role.clone(), task: fill_folder(&role.task, folder), folder: folder.to_string(), parent_pane };
        match starter.start(&step) {
            Ok(pane_id) => {
                panes[i] = Some(pane_id.clone());
                out.started.push(Started { role: role.name.clone(), agent_type: role.agent_type.clone(), pane_id });
            }
            Err(failure) => {
                out.failed = Some(Failed { role: role.name.clone(), code: failure.code, message: failure.message });
                out.not_started = order[n + 1..].iter().map(|&j| crew.roles[j].name.clone()).collect();
                break;
            }
        }
    }
    out
}
