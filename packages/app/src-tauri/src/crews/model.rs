//! The crew file and its error type.

use serde::{Deserialize, Serialize};

/// A crew holds at most this many roles.
pub const MAX_ROLES: usize = 8;
/// The only placeholder a task may use: the folder the crew is started in.
pub const PLACEHOLDER: &str = "{folder}";
/// A crew file larger than this is not read.
pub const MAX_FILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crew {
    pub name: String,
    #[serde(default)]
    pub roles: Vec<Role>,
}

/// One agent of the crew. `task` may use `{folder}`; `parent` names another role of the crew (the
/// root role has none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub agent_type: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Why a crew was refused. `code` is stable; `message` is plain words for the person. `role` names
/// the role the problem belongs to (so the sheet can mark its row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrewError {
    pub code: &'static str,
    pub role: Option<String>,
    pub message: String,
}

impl CrewError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, role: None, message: message.into() }
    }

    pub fn for_role(code: &'static str, role: &str, message: impl Into<String>) -> Self {
        Self { code, role: Some(role.to_string()), message: message.into() }
    }
}

impl std::fmt::Display for CrewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Read a crew file's text. Shape only; `validate::validate` decides whether it may be used.
pub fn parse(text: &str) -> Result<Crew, CrewError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(CrewError::new("CREW_FILE_TOO_BIG", "This crew file is too big to read."));
    }
    toml::from_str::<Crew>(text).map_err(|e| CrewError::new("CREW_FILE_INVALID", format!("This crew file cannot be read: {}", e.message())))
}

/// The file text of a crew.
pub fn to_toml(crew: &Crew) -> Result<String, CrewError> {
    toml::to_string(crew).map_err(|e| CrewError::new("CREW_FILE_INVALID", format!("This crew cannot be saved: {e}")))
}
