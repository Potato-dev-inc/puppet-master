//! Which agent a role runs. A starter role says `{default}`: it takes the person's default agent when the
//! crew starts. The Start sheet can change any role's agent before that; `with_agents` applies the choice.
//! Nothing here names an agent: the default comes from the Ask bar's settings, then the list of agents
//! that can take a role.

use super::model::{Crew, CrewError, DEFAULT_AGENT};
use serde::Serialize;
use std::collections::BTreeMap;

/// An agent a role can run: dispatchable and installed on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentChoice {
    pub id: String,
    pub label: String,
}

/// What the Start sheet offers: the agents that can take a role, and the one `{default}` stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choices {
    pub agents: Vec<AgentChoice>,
    pub default: Option<String>,
}

/// The agent `{default}` stands for: the Ask bar's default agent if it can take a role, else the agent
/// used last, else the first agent of the list. Matching ignores case; the list's own id is returned.
pub fn pick_default(preferred: Option<&str>, last: Option<&str>, available: &[AgentChoice]) -> Option<String> {
    let find = |id: &str| available.iter().find(|a| a.id.eq_ignore_ascii_case(id)).map(|a| a.id.clone());
    preferred
        .and_then(find)
        .or_else(|| last.and_then(find))
        .or_else(|| available.first().map(|a| a.id.clone()))
}

/// Every `{default}` role gets `default`. With no agent to give, the crew cannot start
/// (`CREW_NO_DEFAULT_AGENT`, naming the role).
pub fn resolve_defaults(crew: &Crew, default: Option<&str>) -> Result<Crew, CrewError> {
    let mut out = crew.clone();
    for role in out.roles.iter_mut().filter(|r| r.agent_type == DEFAULT_AGENT) {
        let Some(agent) = default else {
            return Err(CrewError::for_role(
                "CREW_NO_DEFAULT_AGENT",
                &role.name,
                format!("{} needs an agent, and no agent is set up on this computer.", role.name),
            ));
        };
        role.agent_type = agent.to_string();
    }
    Ok(out)
}

/// The Start sheet's choices: role name (any case) -> agent id. Roles that are not named keep their agent.
pub fn with_agents(crew: &Crew, chosen: &BTreeMap<String, String>) -> Crew {
    let mut out = crew.clone();
    for role in &mut out.roles {
        let name = role.name.trim();
        if let Some(agent) = chosen.iter().find(|(role_name, _)| role_name.trim().eq_ignore_ascii_case(name)).map(|(_, agent)| agent) {
            role.agent_type = agent.trim().to_string();
        }
    }
    out
}
