//! The read-only viewer for one old chat (`open_chat`) and the plan to continue it in a pane (`resume_plan`).
//! Both are pure: every input is passed in (the store, the environment, the index row, the home folder, the profile's
//! resume arguments), so a test can run them on a temp folder. Nothing here writes anything, and nothing names an agent.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{valid_session_id, HistoryEnv, ReadError, ReadLimits, SessionHistory};
use crate::chat_index::ImportedChatRow;
use crate::redact;
use crate::redact::Scrub;
use crate::session_chat::{home_to_tilde, ChatItem};

/// Most items the viewer returns for one chat. A longer chat is cut and marked `cut_short`.
pub const VIEW_MAX_ITEMS: usize = 6000;

const GONE: &str = "The original chat file is gone. Search still has its text.";
const NOT_IN_INDEX: &str = "This chat is not in the search index.";
const NO_ID: &str = "This chat's id is not valid.";
const NO_RESUME: &str = "This agent has no way to continue an old chat.";
const NO_FOLDER: &str = "The folder of this chat is not known.";

/// The header of an opened chat. Taken from the index row, redacted again before it is returned.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMeta {
    pub title: String,
    pub agent: String,
    /// The folder NAME only.
    pub folder: String,
    pub updated_ms: i64,
    pub cut_short: bool,
}

/// One old chat for the read-only view. `items` are the `ChatItem` JSON rows, redacted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenedChat {
    pub meta: ChatMeta,
    pub items: Vec<serde_json::Value>,
}

/// What it takes to continue an old chat in a new pane. `cwd` is the folder with `~` expanded; `extra_args` are the
/// agent's own resume arguments (see `SessionHistory::resume_args`). `reason` is a plain sentence when `ok` is false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePlan {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub agent_type: String,
    pub cwd: String,
    pub extra_args: Vec<String>,
}

/// Reads one old chat for the read-only view. The chat is looked up again by its session id (no file path is stored).
/// Items are redacted before they are returned: secrets masked (`redact::json`), e-mail addresses masked, the home
/// folder shown as `~`. The header comes from the index row. A chat past `VIEW_MAX_ITEMS` is cut and marked.
pub fn open_chat(
    history: &dyn SessionHistory,
    env: &HistoryEnv,
    row: &ImportedChatRow,
    session: &str,
) -> Result<OpenedChat, String> {
    if session != row.session {
        return Err(NOT_IN_INDEX.to_string());
    }
    let home = env.home().map(|home| home.to_string_lossy().into_owned());
    let limits = ReadLimits { max_items: VIEW_MAX_ITEMS, ..ReadLimits::default() };
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;
    let outcome = history.read(env, session, &limits, &mut |chunk: Vec<ChatItem>| {
        let room = VIEW_MAX_ITEMS - items.len();
        if chunk.len() > room {
            truncated = true;
        }
        items.extend(chunk.iter().take(room).filter_map(|item| view_value(item, home.as_deref())));
        !truncated
    });
    let outcome = outcome.map_err(read_error_text)?;
    let home = home.as_deref();
    Ok(OpenedChat {
        meta: ChatMeta {
            title: tidy_text(&row.title, home),
            agent: row.agent.clone(),
            folder: tidy_text(&row.folder, home),
            updated_ms: row.updated_ms,
            cut_short: row.cut_short || outcome.cut_short || truncated,
        },
        items,
    })
}

/// The plan to continue an old chat in a new pane. `profile_resume` is the profile's `resume_args(session)`, already
/// filled in (`None` = the agent has no resume option). Refuses with a plain reason when the id is not valid, the agent
/// has no resume option, the folder is not known, or the folder is gone. Only `Path::is_dir` is checked on disk.
pub fn resume_plan(
    profile_resume: Option<Vec<String>>,
    row: &ImportedChatRow,
    home: Option<&Path>,
    agent_type: &str,
) -> ResumePlan {
    let refused = |reason: String| ResumePlan {
        ok: false,
        reason: Some(reason),
        agent_type: agent_type.to_string(),
        cwd: String::new(),
        extra_args: Vec::new(),
    };
    if !valid_session_id(&row.session) {
        return refused(NO_ID.to_string());
    }
    let Some(extra_args) = profile_resume else {
        return refused(NO_RESUME.to_string());
    };
    let Some(path) = expand_cwd(&row.cwd, home) else {
        return refused(NO_FOLDER.to_string());
    };
    if !path.is_dir() {
        return refused(gone_reason(&row.folder));
    }
    ResumePlan {
        ok: true,
        reason: None,
        agent_type: agent_type.to_string(),
        cwd: path.to_string_lossy().into_owned(),
        extra_args,
    }
}

/// A plain sentence for a read that failed. No path and no chat text.
fn read_error_text(err: ReadError) -> String {
    match err {
        ReadError::Missing => GONE.to_string(),
        ReadError::TooBig => "This chat file is too big to open.".to_string(),
        ReadError::Unreadable => "The chat file could not be read.".to_string(),
        ReadError::Format => "This chat is in a format Puppet Master cannot read yet.".to_string(),
    }
}

fn gone_reason(folder: &str) -> String {
    if folder.is_empty() {
        "The folder is gone.".to_string()
    } else {
        format!("The folder is gone: {folder}.")
    }
}

/// One item as the viewer shows it: redacted, e-mails masked, home folder as `~`. `None` only if it cannot be
/// serialised, which a `ChatItem` never is.
fn view_value(item: &ChatItem, home: Option<&str>) -> Option<serde_json::Value> {
    let value = serde_json::to_value(item).ok()?;
    let mut value = redact::json(value).into_inner();
    value.scrub(&mut |text| tidy(text, home));
    Some(value)
}

/// Header text: redacted, e-mails masked, home folder as `~`.
fn tidy_text(text: &str, home: Option<&str>) -> String {
    tidy(&redact::text(text).into_inner(), home)
}

/// Masks e-mail addresses and shows the home folder as `~`. Redaction happens before this (secrets first, as in
/// `export_text::cut_paths`), so a secret inside an address or a path is already gone.
fn tidy(text: &str, home: Option<&str>) -> String {
    home_to_tilde(&redact::mask_emails(text), home)
}

/// The folder a `cwd` names, with a leading `~` replaced by the home folder. `None` when there is no folder, when `~`
/// has no home to expand to, or when the path climbs with `..`.
fn expand_cwd(cwd: &str, home: Option<&Path>) -> Option<PathBuf> {
    let cwd = cwd.trim();
    if cwd.is_empty() || cwd.split(['/', '\\']).any(|part| part == "..") {
        return None;
    }
    let tail = if cwd == "~" { Some("") } else { cwd.strip_prefix("~/").or_else(|| cwd.strip_prefix("~\\")) };
    match tail {
        Some(tail) => {
            let mut path = home?.to_path_buf();
            for part in tail.split(['/', '\\']).filter(|part| !part.is_empty()) {
                path.push(part);
            }
            Some(path)
        }
        None => Some(PathBuf::from(cwd)),
    }
}
