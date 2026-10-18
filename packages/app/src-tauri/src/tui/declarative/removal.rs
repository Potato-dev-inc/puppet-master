//! Remove / restore a definition from the user's own agents folder (Settings > Agents).
//!
//! Removing never deletes. "Remove" moves the definition file, its fixtures folder (`<stem>/`) and
//! `<stem>.wizard.json` into `<agents dir>/.removed/<id>-<timestamp>/` together with a small
//! `removal.json` (what the approval was, and a hash of the file as it was), then drops the approval.
//! "Restore" moves everything back, refusing when a file with that name exists again, and brings the
//! approval back only when the file is byte-identical to what was approved. Only "Delete for good"
//! (`purge_at` / `purge_all_at`) deletes, and only folders whose name has the removal shape
//! (`is_removed_id`). Links are never followed: a link is removed as a link, nothing behind it.
//!
//! Pure over explicit dirs and an approvals path, like `service.rs`. The caller reloads the registry.
//! Only direct children of the agents folder can be removed: a project or extra-directory definition
//! lives in the user's own folder and is never moved (it can only be switched off with Revoke).

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::approvals::{Approval, Approvals};
use super::loader::load_text;
use super::service::ServiceError;
use super::sources::Origin;
use super::validate::Source;
use crate::standby::key::canonical_project;

/// The folder under the agents dir that holds removed agents. The scan never reads it.
pub const REMOVED_DIR: &str = ".removed";
const META_FILE: &str = "removal.json";
const META_VERSION: u32 = 1;
const MAX_LISTED: usize = 50;

/// One removal at a time: the moves and the approvals file change together.
static GUARD: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    version: u32,
    id: String,
    label: String,
    file_name: String,
    removed_at: String,
    /// SHA-256 (hex) of the definition file's bytes when it was removed.
    bytes_sha256: String,
    /// The approval on record, kept only when it matched the file as it was.
    approval: Option<Approval>,
}

fn err(status: u16, code: &'static str, message: impl Into<String>) -> ServiceError {
    ServiceError::new(status, code, message)
}

fn io_err(what: &str, e: std::io::Error) -> ServiceError {
    err(500, "AGENT_DEF_REMOVE_FAILED", format!("{what}: {e}"))
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A file or folder name that is one plain path component, not hidden.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && Path::new(name).components().count() == 1
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// `20261016-123456` from `2026-10-16T12:34:56Z`.
fn stamp(now: &str) -> String {
    let digits: String = now.chars().filter(char::is_ascii_digit).collect();
    if digits.len() >= 14 {
        format!("{}-{}", &digits[..8], &digits[8..14])
    } else {
        "unknown".into()
    }
}

fn safe_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(40)
        .collect();
    if cleaned.is_empty() { "agent".into() } else { cleaned }
}

/// The file the caller named, proven to be a direct child of `agents_dir`. The caller may pass the
/// path as listed or a bare file name; `..`, other folders and symlinks are refused.
fn resolve_own_file(agents_dir: &Path, file: &str) -> Result<PathBuf, ServiceError> {
    let file = file.trim();
    let given = Path::new(file);
    if file.is_empty() || given.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(err(400, "AGENT_DEF_PATH_REFUSED", "that file path is not allowed"));
    }
    let candidate = if given.is_absolute() {
        given.to_path_buf()
    } else if given.components().count() == 1 {
        agents_dir.join(given)
    } else {
        return Err(err(400, "AGENT_DEF_PATH_REFUSED", "that file path is not allowed"));
    };
    let name = candidate
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_plain_name(n) && n.to_ascii_lowercase().ends_with(".toml"))
        .ok_or_else(|| err(400, "AGENT_DEF_PATH_REFUSED", "only a .toml definition file can be removed"))?
        .to_string();
    let agents = fs::canonicalize(agents_dir)
        .map_err(|_| err(404, "AGENT_DEF_NOT_FOUND", "your agents folder does not exist yet"))?;
    let parent = candidate.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(agents_dir);
    let parent_real = fs::canonicalize(parent)
        .map_err(|_| err(404, "AGENT_DEF_NOT_FOUND", format!("no definition file {file}")))?;
    if parent_real != agents {
        return Err(err(
            400,
            "AGENT_DEF_NOT_OWN_FOLDER",
            format!(
                "This agent comes from {}. Remove it there, or turn it off with Revoke.",
                parent_real.display().to_string().trim_start_matches(r"\\?\")
            ),
        ));
    }
    let real = agents_dir.join(&name);
    match fs::symlink_metadata(&real) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(err(400, "AGENT_DEF_SYMLINK_REFUSED", "a link cannot be removed; it may point outside the agents folder"))
        }
        Ok(m) if m.is_file() => Ok(real),
        _ => Err(err(404, "AGENT_DEF_NOT_FOUND", format!("no definition file {name}"))),
    }
}

/// Move `from` to `to`, remembering it so a later failure can put everything back.
fn move_one(moved: &mut Vec<(PathBuf, PathBuf)>, from: &Path, to: &Path) -> Result<(), ServiceError> {
    fs::rename(from, to).map_err(|e| {
        io_err(&format!("could not move {}", from.file_name().unwrap_or_default().to_string_lossy()), e)
    })?;
    moved.push((from.to_path_buf(), to.to_path_buf()));
    Ok(())
}

fn roll_back(moved: &[(PathBuf, PathBuf)]) {
    for (from, to) in moved.iter().rev() {
        let _ = fs::rename(to, from);
    }
}

/// Move the definition at `file` (and its fixtures folder, wizard record and approval) out of the
/// agents folder into `.removed/<id>-<timestamp>/`.
pub fn remove_at(
    agents_dir: &Path,
    approvals_path: &Path,
    file: &str,
    code_ids: &[&str],
    now: &str,
) -> Result<Value, ServiceError> {
    let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let path = resolve_own_file(agents_dir, file)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let bytes = fs::read(&path).map_err(|e| io_err("could not read the definition", e))?;
    let (id, label, fingerprint) = match std::str::from_utf8(&bytes) {
        Ok(text) => {
            let loaded = load_text(&path, text, bytes.len(), Origin::new(Source::User), None, code_ids);
            let label = loaded.definition.as_ref().map(|d| d.agent.label.clone());
            (loaded.id.clone(), label, loaded.fingerprint.clone())
        }
        Err(_) => (None, None, None),
    };
    let id = id.unwrap_or_else(|| stem.clone());
    let label = label.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| id.clone());

    let mut approvals = Approvals::load(approvals_path);
    let approval = approvals
        .get(&path)
        .filter(|record| fingerprint.as_deref() == Some(record.fingerprint.as_str()))
        .cloned();

    let removed_root = agents_dir.join(REMOVED_DIR);
    fs::create_dir_all(&removed_root).map_err(|e| io_err("could not create the .removed folder", e))?;
    let base = format!("{}-{}", safe_id(&id), stamp(now));
    let (removed_id, folder) = (0..100)
        .find_map(|n| {
            let candidate = if n == 0 { base.clone() } else { format!("{base}-{}", n + 1) };
            let dir = removed_root.join(&candidate);
            fs::create_dir(&dir).ok().map(|()| (candidate, dir))
        })
        .ok_or_else(|| err(500, "AGENT_DEF_REMOVE_FAILED", "could not create a folder for the removed agent"))?;

    let meta = Meta {
        version: META_VERSION,
        id: id.clone(),
        label: label.clone(),
        file_name: name.clone(),
        removed_at: now.to_string(),
        bytes_sha256: sha_hex(&bytes),
        approval,
    };
    let abandon = |folder: &Path| {
        let _ = fs::remove_file(folder.join(META_FILE));
        let _ = fs::remove_dir(folder);
    };
    let meta_text = serde_json::to_string_pretty(&meta).unwrap_or_default();
    if let Err(e) = fs::write(folder.join(META_FILE), meta_text) {
        abandon(&folder);
        return Err(io_err("could not write the removal record", e));
    }

    let fixtures = agents_dir.join(&stem);
    let wizard = agents_dir.join(format!("{stem}.wizard.json"));
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut had_fixtures = false;
    let mut had_wizard = false;
    let mut work = || -> Result<(), ServiceError> {
        if exists(&fixtures) {
            move_one(&mut moved, &fixtures, &folder.join(&stem))?;
            had_fixtures = true;
        }
        if exists(&wizard) {
            move_one(&mut moved, &wizard, &folder.join(format!("{stem}.wizard.json")))?;
            had_wizard = true;
        }
        move_one(&mut moved, &path, &folder.join(&name))?;
        let had_approval = approvals.remove_path(&path);
        if had_approval {
            approvals.save(approvals_path).map_err(|e| err(503, "AGENT_DEF_STORE_FAILED", e))?;
        }
        Ok(())
    };
    if let Err(e) = work() {
        roll_back(&moved);
        abandon(&folder);
        return Err(e);
    }
    Ok(json!({
        "ok": true,
        "id": id,
        "label": label,
        "removed_id": removed_id,
        "file": path.to_string_lossy(),
        "fixtures": had_fixtures,
        "wizard": had_wizard,
        "was_approved": meta.approval.is_some(),
    }))
}

fn read_meta(folder: &Path) -> Option<Meta> {
    let meta: Meta = serde_json::from_str(&fs::read_to_string(folder.join(META_FILE)).ok()?).ok()?;
    (meta.version == META_VERSION && is_plain_name(&meta.file_name) && meta.file_name.to_ascii_lowercase().ends_with(".toml"))
        .then_some(meta)
}

/// `{removed: [...], total}`, newest first: what `.removed/` holds that can be restored. `total` counts
/// every restorable entry; `removed` is capped at `MAX_LISTED`.
pub fn list_removed_at(agents_dir: &Path) -> Value {
    let mut rows: Vec<(String, Meta, String)> = Vec::new();
    if let Ok(entries) = fs::read_dir(agents_dir.join(REMOVED_DIR)) {
        for entry in entries.filter_map(Result::ok) {
            let Some(folder_name) = entry.file_name().to_str().map(str::to_string) else { continue };
            let is_dir = fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_dir());
            if !is_dir || !is_plain_name(&folder_name) {
                continue;
            }
            if let Some(meta) = read_meta(&entry.path()) {
                rows.push((meta.removed_at.clone(), meta, folder_name));
            }
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)));
    let total = rows.len();
    let removed: Vec<Value> = rows
        .into_iter()
        .take(MAX_LISTED)
        .map(|(_, meta, removed_id)| {
            let stem = Path::new(&meta.file_name).file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            let folder = agents_dir.join(REMOVED_DIR).join(&removed_id);
            json!({
                "removed_id": removed_id,
                "id": meta.id,
                "label": meta.label,
                "file_name": meta.file_name,
                "removed_at": meta.removed_at,
                "fixtures": folder.join(&stem).is_dir(),
                "wizard": folder.join(format!("{stem}.wizard.json")).is_file(),
                "was_approved": meta.approval.is_some(),
            })
        })
        .collect();
    json!({"removed": removed, "total": total})
}

/// Put a removed agent back. Refuses when the file, its fixtures folder or its wizard record exists
/// again; nothing is overwritten. The approval returns only for a byte-identical file.
pub fn restore_at(
    agents_dir: &Path,
    approvals_path: &Path,
    removed_id: &str,
    code_ids: &[&str],
) -> Result<Value, ServiceError> {
    let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let removed_id = removed_id.trim();
    if !is_plain_name(removed_id) {
        return Err(err(400, "AGENT_DEF_PATH_REFUSED", "that removed-agent name is not allowed"));
    }
    let folder = agents_dir.join(REMOVED_DIR).join(removed_id);
    if !fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir()) {
        return Err(err(404, "AGENT_DEF_NOT_FOUND", "that removed agent is not in the .removed folder any more"));
    }
    let meta = read_meta(&folder)
        .ok_or_else(|| err(404, "AGENT_DEF_NOT_FOUND", "the removal record is missing or unreadable"))?;
    let name = meta.file_name.clone();
    let stem = Path::new(&name).file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    let wizard_name = format!("{stem}.wizard.json");
    let parts: Vec<(PathBuf, PathBuf, String)> = [
        (folder.join(&stem), agents_dir.join(&stem), format!("A folder named {stem}")),
        (folder.join(&wizard_name), agents_dir.join(&wizard_name), format!("A file named {wizard_name}")),
        (folder.join(&name), agents_dir.join(&name), format!("A file named {name}")),
    ]
    .into_iter()
    .filter(|(from, _, _)| exists(from))
    .collect();
    if !parts.iter().any(|(_, to, _)| to.file_name().is_some_and(|n| n == name.as_str())) {
        return Err(err(404, "AGENT_DEF_NOT_FOUND", format!("{name} is missing from the removed folder")));
    }
    for (_, to, what) in &parts {
        if exists(to) {
            return Err(err(
                409,
                "AGENT_DEF_RESTORE_CLASH",
                format!("{what} is already in your agents folder. Rename or move it, then restore again."),
            ));
        }
    }
    fs::create_dir_all(agents_dir).map_err(|e| io_err("could not create the agents folder", e))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to, _) in &parts {
        if let Err(e) = move_one(&mut moved, from, to) {
            roll_back(&moved);
            return Err(e);
        }
    }
    let path = agents_dir.join(&name);
    let bytes = fs::read(&path).unwrap_or_default();
    let loaded = std::str::from_utf8(&bytes)
        .ok()
        .map(|text| load_text(&path, text, bytes.len(), Origin::new(Source::User), None, code_ids));
    let valid = loaded.as_ref().is_some_and(|l| l.is_valid());
    let identical = sha_hex(&bytes) == meta.bytes_sha256;
    let mut approval_restored = false;
    if let (true, Some(record), Some(loaded)) = (identical, &meta.approval, &loaded) {
        let same_path = canonical_project(&record.path) == canonical_project(&path.to_string_lossy());
        if same_path && loaded.fingerprint.as_deref() == Some(record.fingerprint.as_str()) {
            let mut approvals = Approvals::load(approvals_path);
            approvals.approve(record.clone());
            match approvals.save(approvals_path) {
                Ok(()) => approval_restored = true,
                Err(e) => tracing::warn!(%e, "restored agent kept without its approval"),
            }
        }
    }
    let _ = fs::remove_file(folder.join(META_FILE));
    let _ = fs::remove_dir(&folder);
    Ok(json!({
        "ok": true,
        "id": meta.id,
        "label": meta.label,
        "file": path.to_string_lossy(),
        "valid": valid,
        "approval_restored": approval_restored,
        "needs_approval": valid && !approval_restored,
    }))
}

/// The shape `remove_at` gives a removed folder's name: `<safe id>-<YYYYMMDD-HHMMSS | unknown>[-<n>]`.
/// Purge deletes only names of this shape, so a stray folder in `.removed/` is never touched.
fn is_removed_id(name: &str) -> bool {
    static SHAPE: OnceLock<regex::Regex> = OnceLock::new();
    SHAPE
        .get_or_init(|| regex::Regex::new(r"^[A-Za-z0-9_-]{1,40}-(\d{8}-\d{6}|unknown)(-\d{1,3})?$").expect("valid pattern"))
        .is_match(name)
}

/// A symbolic link or a Windows junction. These are never followed.
fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Removes a link itself (a directory link needs `remove_dir`), never what it points to.
fn remove_link_or_file(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

/// Deletes `dir` and everything under it. A link met on the way is removed as a link and not followed,
/// so nothing outside `dir` can be reached through it.
fn remove_tree(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() && !is_link(&meta) {
            remove_tree(&path)?;
        } else {
            remove_link_or_file(&path)?;
        }
    }
    fs::remove_dir(dir)
}

/// The `.removed` folder when it is a real folder. `None` when it does not exist yet; a link named
/// `.removed` is refused, so nothing is deleted through it.
fn real_removed_root(agents_dir: &Path) -> Result<Option<PathBuf>, ServiceError> {
    let root = agents_dir.join(REMOVED_DIR);
    match fs::symlink_metadata(&root) {
        Err(_) => Ok(None),
        Ok(meta) if meta.is_dir() && !is_link(&meta) => Ok(Some(root)),
        Ok(_) => Err(err(400, "AGENT_DEF_PURGE_FAILED", "the .removed folder is a link, so nothing was deleted")),
    }
}

/// Deletes one entry directly under `root` when its name has the removal shape. A link of that name is
/// removed as a link. A plain file or an unknown name is left alone. `Ok(false)` when nothing was there.
fn purge_entry(root: &Path, name: &str) -> std::io::Result<bool> {
    if !is_plain_name(name) || !is_removed_id(name) {
        return Ok(false);
    }
    let path = root.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if is_link(&meta) {
        remove_link_or_file(&path)?;
        return Ok(true);
    }
    if !meta.is_dir() {
        return Ok(false);
    }
    remove_tree(&path)?;
    Ok(true)
}

fn purge_failed(e: std::io::Error) -> ServiceError {
    err(500, "AGENT_DEF_PURGE_FAILED", format!("could not delete the removed agent: {e}"))
}

/// "Delete for good": permanently deletes ONE entry in `.removed/`. Nothing else changes. The agents
/// folder, the approvals and the running panes are not touched.
pub fn purge_at(agents_dir: &Path, removed_id: &str) -> Result<Value, ServiceError> {
    let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let removed_id = removed_id.trim();
    if !is_plain_name(removed_id) || !is_removed_id(removed_id) {
        return Err(err(400, "AGENT_DEF_PATH_REFUSED", "that removed-agent name is not allowed"));
    }
    let not_found = || err(404, "AGENT_DEF_NOT_FOUND", "that removed agent is not in the .removed folder any more");
    let root = real_removed_root(agents_dir)?.ok_or_else(not_found)?;
    if !purge_entry(&root, removed_id).map_err(purge_failed)? {
        return Err(not_found());
    }
    Ok(json!({"ok": true, "removed_id": removed_id, "deleted": 1}))
}

/// "Clear all": permanently deletes every removal entry in `.removed/`. Names without the removal shape
/// are left alone. One entry that cannot be deleted does not stop the others; `failed` counts them.
pub fn purge_all_at(agents_dir: &Path) -> Result<Value, ServiceError> {
    let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let Some(root) = real_removed_root(agents_dir)? else {
        return Ok(json!({"ok": true, "deleted": 0, "failed": 0}));
    };
    let names: Vec<String> = fs::read_dir(&root)
        .map_err(purge_failed)?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let (mut deleted, mut failed) = (0usize, 0usize);
    for name in names {
        match purge_entry(&root, &name) {
            Ok(true) => deleted += 1,
            Ok(false) => {}
            Err(e) => {
                failed += 1;
                tracing::warn!(%e, "could not delete a removed agent");
            }
        }
    }
    Ok(json!({"ok": failed == 0, "deleted": deleted, "failed": failed}))
}
