//! Add a shipped catalog agent to the agents folder with a Store approval, and remove it again.
//!
//! `add_in` and `remove_in` take the catalog, the folders and the registry as arguments, so the tests run
//! against temp folders and a test catalog. `add` and `remove` are the thin wrappers the app calls.
//!
//! Add refuses before it writes anything: an unknown id, a built-in agent, a definition with a setting a Store
//! definition may not carry (check rule 4), a program that is not on PATH, a short name the person did not
//! confirm, and any file or fixtures folder that already holds this id. Then the shipped text is written with
//! `create_new`, approved by `approve_as_store` (which re-reads the file and compares its bytes) and the
//! registry is reloaded. If anything fails after the first write, this call takes its own writes back.
//!
//! Remove deletes only a definition whose bytes equal the shipped copy, through `removal::remove_at` (so it
//! can be put back). A file the person edited is never touched.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::catalog::{self, CatalogEntry};
use super::check::forbidden_in_store;
use crate::tui::declarative::approvals::{self, now_rfc3339};
use crate::tui::declarative::loader::{reload_into, LoadInputs, OwnedInputs};
use crate::tui::declarative::removal;
use crate::tui::declarative::resolve::resolve_binary;
use crate::tui::declarative::schema::parse_definition;
use crate::tui::declarative::service::approve_as_store;
use crate::tui::declarative::sources::scan_dir;
use crate::tui::registry::Registry;

#[cfg(test)]
mod tests;

/// Add the shipped agent `id` to the app's agents folder (the app's own catalog, folders and registry).
pub fn add(id: &str, confirmed_found: bool) -> Result<Value, String> {
    let owned = OwnedInputs::current();
    add_in(
        catalog::all(),
        &catalog::catalog_version(),
        &owned.inputs(),
        &approvals::default_path(),
        crate::tui::registry::registry(),
        id,
        confirmed_found,
    )
}

/// Remove the shipped agent `id` from the app's agents folder when the file is still the shipped copy.
pub fn remove(id: &str) -> Result<Value, String> {
    let owned = OwnedInputs::current();
    remove_in(catalog::all(), &owned.inputs(), &approvals::default_path(), crate::tui::registry::registry(), id)
}

/// Add `id` from `catalog` (whose version is `catalog8`) into `inputs.user_dir`, approve it as the Store and
/// reload `registry`. Returns the approval's `{ok, id, state}`.
pub fn add_in(
    catalog: &[CatalogEntry],
    catalog8: &str,
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    registry: &Registry,
    id: &str,
    confirmed_found: bool,
) -> Result<Value, String> {
    let (entry, text) = shipped(catalog, id)?;
    let def = parse_definition(text).map_err(|e| format!("STORE_DEFINITION_INVALID: {}", e.message))?;
    if let Some(reason) = forbidden_in_store(&def) {
        return Err(format!("STORE_FORBIDDEN_SETTING: {id} cannot be added: {reason}."));
    }
    let agents_dir = agents_dir(inputs)?;
    let label = def.agent.label.clone();
    if resolve_binary(&def.binary, agents_dir).is_err() {
        return Err(format!("ADD_PROGRAM_MISSING: {label} is not on this PC. Install it first, then add it again."));
    }
    if entry.entry.store.ambiguous_name && !confirmed_found {
        return Err(format!("ADD_AMBIGUOUS_NOT_CONFIRMED: a program named {label} was found, but the name is shared. Confirm it is the agent first."));
    }
    if taken(agents_dir, id) {
        return Err(format!("AGENT_DEF_DUPLICATE_ID: {id} is already in your agents folder. Puppet Master never overwrites a file."));
    }
    let fixtures = fixtures_of(entry)?;

    fs::create_dir_all(agents_dir).map_err(|e| format!("cannot create the agents folder: {e}"))?;
    let toml_path = agents_dir.join(format!("{id}.toml"));
    let fixtures_dir = agents_dir.join(format!("{id}.fixtures"));
    let mut made = Made::default();
    let outcome = write_files(&toml_path, text, &fixtures_dir, &fixtures, &mut made).and_then(|()| {
        approve_as_store(inputs, approvals_path, &toml_path.to_string_lossy(), text, catalog8)
            .map_err(|e| format!("{}: {}", e.code, e.message))
    });
    match outcome {
        Ok(value) => {
            // An unapproved file is never registered, so the reload comes after the approval.
            reload_into(registry, inputs, approvals_path);
            Ok(value)
        }
        Err(err) => {
            made.undo();
            Err(err)
        }
    }
}

/// Remove the shipped agent `id` from `inputs.user_dir` when its definition is still byte-identical to the
/// shipped copy. Fixture files are removed only when they too are shipped copies; a fixture the person
/// changed or added is kept, and so is its folder.
pub fn remove_in(
    catalog: &[CatalogEntry],
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    registry: &Registry,
    id: &str,
) -> Result<Value, String> {
    let (entry, text) = shipped(catalog, id)?;
    let agents_dir = agents_dir(inputs)?;
    let file = format!("{id}.toml");
    let on_disk = fs::read(agents_dir.join(&file))
        .map_err(|_| format!("AGENT_DEF_NOT_FOUND: {file} is not in your agents folder."))?;
    if on_disk != text.as_bytes() {
        return Err(format!("REMOVE_NOT_STORE_FILE: {file} was edited or is your own file. Remove it in Settings > Agents."));
    }
    let removed = removal::remove_at(agents_dir, approvals_path, &file, inputs.code_ids, &now_rfc3339())
        .map_err(|e| format!("{}: {}", e.code, e.message))?;

    let fixtures_dir = agents_dir.join(format!("{id}.fixtures"));
    let mut kept = 0usize;
    for (name, body) in &entry.fixtures {
        let path = fixtures_dir.join(name);
        match fs::read(&path) {
            Ok(bytes) if bytes == body.as_bytes() => {
                if fs::remove_file(&path).is_err() {
                    kept += 1;
                }
            }
            Ok(_) => kept += 1,
            Err(_) => {}
        }
    }
    // Only an empty folder goes; anything the person put there stays.
    let _ = fs::remove_dir(&fixtures_dir);
    reload_into(registry, inputs, approvals_path);
    Ok(json!({
        "ok": true,
        "id": id,
        "removed_id": removed.get("removed_id").cloned().unwrap_or(Value::Null),
        "fixtures_kept": kept,
    }))
}

/// The shipped entry and its definition text. A built-in agent has no file, and an unknown id has nothing.
fn shipped<'a>(catalog: &'a [CatalogEntry], id: &str) -> Result<(&'a CatalogEntry, &'a str), String> {
    let entry = catalog
        .iter()
        .find(|e| e.entry.store.id == id)
        .ok_or_else(|| format!("STORE_UNKNOWN_ID: no agent named {id} is in the Agent Store."))?;
    match entry.definition_text.as_deref() {
        Some(text) if !entry.entry.store.builtin => Ok((entry, text)),
        _ => Err(format!("STORE_BUILTIN: {id} is built into Puppet Master. It has no file to add or remove.")),
    }
}

fn agents_dir<'a>(inputs: &LoadInputs<'a>) -> Result<&'a Path, String> {
    inputs.user_dir.ok_or_else(|| "AGENT_DEF_NOT_FOUND: there is no agents folder to add to.".to_string())
}

/// True when a file or fixtures folder already holds this id, under any file name.
fn taken(agents_dir: &Path, id: &str) -> bool {
    agents_dir.join(format!("{id}.toml")).exists()
        || agents_dir.join(format!("{id}.fixtures")).exists()
        || scan_dir(agents_dir).files.iter().any(|raw| parse_definition(&raw.text).is_ok_and(|def| def.agent.id == id))
}

/// The fixture files of an entry. A name that could leave the fixtures folder is refused.
fn fixtures_of(entry: &CatalogEntry) -> Result<Vec<(String, String)>, String> {
    for (name, _) in &entry.fixtures {
        let plain = !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']);
        if !plain {
            return Err(format!("STORE_FIXTURE_NAME: a fixture file name is not plain: {name}"));
        }
    }
    Ok(entry.fixtures.clone())
}

/// Writes the definition, then its fixtures (the definition claims the id first). Every file and folder that
/// was created is recorded in `made` at once, so a later failure can take it back.
fn write_files(
    toml_path: &Path,
    text: &str,
    fixtures_dir: &Path,
    fixtures: &[(String, String)],
    made: &mut Made,
) -> Result<(), String> {
    create_new(toml_path, text, made)?;
    if fixtures.is_empty() {
        return Ok(());
    }
    fs::create_dir(fixtures_dir).map_err(|e| format!("cannot create the fixtures folder: {e}"))?;
    made.dirs.push(fixtures_dir.to_path_buf());
    for (name, body) in fixtures {
        create_new(&fixtures_dir.join(name), body, made)?;
    }
    Ok(())
}

fn create_new(path: &Path, text: &str, made: &mut Made) -> Result<(), String> {
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| match e.kind() {
        ErrorKind::AlreadyExists => format!("AGENT_DEF_DUPLICATE_ID: {name} already exists"),
        _ => format!("cannot write {name}: {e}"),
    })?;
    made.files.push(path.to_path_buf());
    file.write_all(text.as_bytes()).map_err(|e| format!("cannot write {name}: {e}"))
}

/// What one add wrote, so it can be taken back.
#[derive(Default)]
struct Made {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Made {
    /// Removes what was written, newest first. Only a folder that is empty again goes.
    fn undo(&self) {
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}
