//! The folder of crew files: `<app data>/crews/<slug>.toml`. Every function takes the folder, so tests
//! use a temporary one; `dir()` is the real one (it follows the app profile).

use super::model::{parse, to_toml, Crew, CrewError};
use super::validate::{normalized, validate, AgentState};
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_SLUG_CHARS: usize = 40;
const MAX_FILES: usize = 200;

pub fn dir() -> PathBuf {
    crate::app_paths::app_data_dir().join("crews")
}

/// One crew as the list shows it. `problem` is set when the file cannot be started (broken file, or an
/// agent that is gone); the row stays so the person can remove it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub slug: String,
    pub name: String,
    pub roles: usize,
    pub agents: Vec<String>,
    pub problem: Option<String>,
}

/// `Review team!` -> `review-team`. Letters and digits only, never empty.
pub fn slug_of(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(MAX_SLUG_CHARS).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "crew".into() } else { out }
}

/// A slug as `slug_of` makes it (and a number after it); anything else is not a file of ours.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty() && slug.len() <= MAX_SLUG_CHARS + 4 && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn path_of(dir: &Path, slug: &str) -> Result<PathBuf, CrewError> {
    if !valid_slug(slug) {
        return Err(CrewError::new("CREW_NOT_FOUND", "There is no such crew."));
    }
    Ok(dir.join(format!("{slug}.toml")))
}

pub fn load(dir: &Path, slug: &str) -> Result<Crew, CrewError> {
    let path = path_of(dir, slug)?;
    let text = std::fs::read_to_string(&path).map_err(|_| CrewError::new("CREW_NOT_FOUND", "There is no such crew."))?;
    parse(&text)
}

/// One crew as the list shows it: its roles' agents (each once) and the first problem, if any.
pub fn summary(slug: &str, crew: &Crew, agent_state: &dyn Fn(&str) -> AgentState) -> Summary {
    let problem = validate(crew, agent_state).first().map(|e| e.message.clone());
    let mut agents: Vec<String> = Vec::new();
    for r in &crew.roles {
        if !agents.contains(&r.agent_type) {
            agents.push(r.agent_type.clone());
        }
    }
    Summary { slug: slug.to_string(), name: crew.name.clone(), roles: crew.roles.len(), agents, problem }
}

/// A saved crew by slug, or a starter (`starter:<id>`, see `starters`).
pub fn load_any(dir: &Path, slug: &str) -> Result<Crew, CrewError> {
    match super::starters::lookup(slug) {
        Some(crew) => Ok(crew),
        None => load(dir, slug),
    }
}

/// Every crew file, by name.
pub fn list(dir: &Path, agent_state: &dyn Fn(&str) -> AgentState) -> Vec<Summary> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Summary> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let slug = path.file_stem()?.to_str()?.to_string();
            (path.extension().is_some_and(|x| x == "toml") && valid_slug(&slug)).then_some((slug, path))
        })
        .take(MAX_FILES)
        .map(|(slug, _)| match load(dir, &slug) {
            Ok(crew) => summary(&slug, &crew, agent_state),
            Err(e) => Summary { name: slug.clone(), slug, roles: 0, agents: Vec::new(), problem: Some(e.message) },
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// The slug the `n`th crew with this base slug can have: `base`, then `base-2` ... `base-99`.
fn slug_at(base: &str, n: usize) -> String {
    if n == 1 { base.to_string() } else { format!("{base}-{n}") }
}

/// Save `crew` (names trimmed first). A crew with the same name (ignoring case) is the one `replace`
/// overwrites; without `replace` it is refused with `CREW_NAME_TAKEN` and nothing is written. A new
/// name takes the first free slot (`base`, `base-2`, ...). A file that is not this crew is never
/// overwritten, so a full set of slots is refused (`CREW_NAME_FULL`) instead of reusing one.
/// Nothing is written when the crew is invalid.
pub fn save(dir: &Path, crew: &Crew, replace: bool, agent_state: &dyn Fn(&str) -> AgentState) -> Result<String, Vec<CrewError>> {
    let crew = normalized(crew);
    let errors = validate(&crew, agent_state);
    if !errors.is_empty() {
        return Err(errors);
    }
    let base = slug_of(&crew.name);
    let mut same_name: Option<String> = None;
    let mut free: Option<String> = None;
    for n in 1..100 {
        let slug = slug_at(&base, n);
        if !path_of(dir, &slug).map_err(|e| vec![e])?.exists() {
            free.get_or_insert(slug);
        } else if load(dir, &slug).is_ok_and(|existing| existing.name.eq_ignore_ascii_case(&crew.name)) {
            same_name = Some(slug);
            break;
        }
    }
    let slug = match (same_name, free) {
        (Some(_), _) if !replace => {
            return Err(vec![CrewError::new("CREW_NAME_TAKEN", "A crew with this name exists. Replace it?")]);
        }
        (Some(slug), _) | (None, Some(slug)) => slug,
        (None, None) => {
            return Err(vec![CrewError::new("CREW_NAME_FULL", "Too many crews already use this name. Rename this crew.")]);
        }
    };
    let path = path_of(dir, &slug).map_err(|e| vec![e])?;
    let text = to_toml(&crew).map_err(|e| vec![e])?;
    crate::secure_file::write_private(&path, &text).map_err(|e| vec![CrewError::new("CREW_WRITE_FAILED", format!("Could not save the crew: {e}"))])?;
    Ok(slug)
}

/// Delete one crew file (only ever a file of ours, found by slug).
pub fn remove(dir: &Path, slug: &str) -> Result<(), CrewError> {
    let path = path_of(dir, slug)?;
    std::fs::remove_file(path).map_err(|_| CrewError::new("CREW_NOT_FOUND", "There is no such crew."))
}
