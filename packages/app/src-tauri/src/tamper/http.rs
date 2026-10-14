//! `GET /tamper/check?project=<folder>&since=<snapshot>[&total=<n>&skipped=<n>]`.
//!
//! Read-only: it compares the folder with a snapshot a mission took earlier and returns the
//! findings. It cannot create snapshots and cannot change files, so a paired phone with `watch` may
//! call it. A folder without a saved snapshot of that name answers 404, so the route cannot be used
//! to read arbitrary folders.

use std::path::Path;

use serde_json::{json, Value};

use super::model::{Report, RunCounts};
use super::signals::CheckOptions;
use super::store::TamperError;

fn fail(error: TamperError) -> (u16, Value) {
    let (status, code) = match &error {
        TamperError::NotFound(_) => (404, "TAMPER_SNAPSHOT_NOT_FOUND"),
        TamperError::BadId(_) | TamperError::BadProject(_) => (400, "INVALID_ARGUMENT"),
        TamperError::Corrupt(_) | TamperError::Io(_) => (500, "TAMPER_CHECK_FAILED"),
    };
    (status, json!({"code": code, "error": error.to_string()}))
}

/// The shared body of the route and the Tauri command.
pub fn check_project(project: &str, since: &str, counts: Option<RunCounts>) -> Result<Report, TamperError> {
    if project.trim().is_empty() {
        return Err(TamperError::BadProject(project.to_string()));
    }
    let root = Path::new(project);
    let opts = CheckOptions { counts, ..CheckOptions::default() };
    super::check_saved(root, since, &opts)
}

/// `None` when the request is not a tamper route. `param` reads one decoded query parameter.
pub fn respond(
    method: &str,
    segments: &[&str],
    param: &dyn Fn(&str) -> Option<String>,
) -> Option<Result<(u16, Value), (u16, Value)>> {
    if method != "GET" || segments != ["tamper", "check"] {
        return None;
    }
    let (Some(project), Some(since)) = (param("project"), param("since")) else {
        return Some(Err((
            400,
            json!({"code": "INVALID_ARGUMENT", "error": "Say which project and which snapshot: /tamper/check?project=<folder>&since=<snapshot>."}),
        )));
    };
    let number = |name: &str| param(name).and_then(|v| v.parse::<u32>().ok());
    let counts = match (number("total"), number("skipped")) {
        (None, None) => None,
        (total, skipped) => Some(RunCounts { total, skipped, ..RunCounts::default() }),
    };
    Some(check_project(&project, &since, counts).map(|r| (200, serde_json::to_value(r).unwrap_or_default())).map_err(fail))
}
