//! The filters of a search ("Search all chats" chips and typed shortcuts): one agent, one folder, a start time and
//! who spoke. Each one becomes a bound SQL parameter (`?N`), never text in the SQL, so a folder name with a quote
//! or a SQL fragment is only a value.

use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

/// Who spoke: the person (`user` rows), the agent (`assistant` rows) or a tool call (`tool` rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    You,
    Agent,
    Tool,
}

impl Role {
    /// The `kind` the index stores for this role.
    fn kind(self) -> &'static str {
        match self {
            Role::You => "user",
            Role::Agent => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// The optional filters of one search. Empty fields (and empty strings) mean "any".
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filters {
    /// The agent id the index stores (exact).
    pub agent: Option<String>,
    /// The folder name the index stores (exact, not the path).
    pub folder: Option<String>,
    /// Only items at or after this time (milliseconds since the epoch).
    pub since_ms: Option<i64>,
    pub role: Option<Role>,
}

impl Filters {
    /// Whether no filter is set.
    pub fn is_empty(&self) -> bool {
        self.agent().is_none() && self.folder().is_none() && self.since_ms.is_none() && self.role.is_none()
    }

    fn agent(&self) -> Option<&str> {
        self.agent.as_deref().filter(|s| !s.is_empty())
    }

    fn folder(&self) -> Option<&str> {
        self.folder.as_deref().filter(|s| !s.is_empty())
    }
}

/// Adds one value to the bound parameters and returns its placeholder (`?N`, numbered from 1 in the order added).
pub fn bind(values: &mut Vec<Value>, value: Value) -> String {
    values.push(value);
    format!("?{}", values.len())
}

/// The `AND` clauses for the filters set (`i` is the items table alias), with every value bound into `values`.
pub fn clauses(filters: &Filters, values: &mut Vec<Value>) -> String {
    let mut sql = String::new();
    if let Some(agent) = filters.agent() {
        let at = bind(values, agent.to_string().into());
        sql.push_str(&format!(" AND i.agent = {at}"));
    }
    if let Some(folder) = filters.folder() {
        let at = bind(values, folder.to_string().into());
        sql.push_str(&format!(" AND i.folder = {at}"));
    }
    if let Some(since) = filters.since_ms {
        let at = bind(values, since.into());
        sql.push_str(&format!(" AND i.at_ms >= {at}"));
    }
    if let Some(role) = filters.role {
        let at = bind(values, role.kind().to_string().into());
        sql.push_str(&format!(" AND i.kind = {at}"));
    }
    sql
}

/// The agents and folders the index holds, for the filter menus (sorted, bounded).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Facets {
    pub agents: Vec<String>,
    pub folders: Vec<String>,
}

/// Most agents and most folders listed in the menus.
pub const MAX_FACETS: usize = 200;
