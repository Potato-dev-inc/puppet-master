//! Load definition text into reports (parse, validate, fingerprint), apply REG-8 precedence, and
//! (`build`/`reload`) turn the approved ones into registrable profiles.
//!
//! Everything up to `resolve` is pure; `build` reads approvals; only `reload_into`/`reload` touch a
//! registry. A bad file never aborts the others; every problem is a `DefError` on its own report.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::approvals::{ApprovalState, Approvals};
use super::fingerprint::fingerprint;
use super::profile::{DeclarativeProfile, DefinitionOrigin};
use super::schema::{parse_definition, DefError, Definition};
use super::sources::{scan_dir, source_name, Origin, RawFile, ScanIssue};
use super::validate::{validate, Source, ValidationContext};
use crate::tui::registry::Registry;
use crate::tui::TuiProfile;

/// What one definition file turned into.
#[derive(Debug, Clone)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub origin: Origin,
    /// Canonical project path for a project definition (part of its fingerprint).
    pub project: Option<String>,
    /// The id as written, when the file parsed (also for invalid definitions, for reporting).
    pub id: Option<String>,
    /// Present only when the file parsed and validated clean.
    pub definition: Option<Definition>,
    pub fingerprint: Option<String>,
    pub errors: Vec<DefError>,
}

impl LoadedFile {
    pub fn is_valid(&self) -> bool {
        self.definition.is_some()
    }

    pub fn source_name(&self) -> &'static str {
        source_name(self.origin.source)
    }
}

/// Parse, validate and fingerprint one definition text. Used for scanned files and for the
/// embedded bundled definitions alike.
pub fn load_text(
    path: &Path,
    text: &str,
    raw_len: usize,
    origin: Origin,
    project: Option<&str>,
    code_ids: &[&str],
) -> LoadedFile {
    let mut file = LoadedFile {
        path: path.to_path_buf(),
        origin,
        project: project.map(str::to_string),
        id: None,
        definition: None,
        fingerprint: None,
        errors: Vec::new(),
    };
    let def = match parse_definition(text) {
        Ok(def) => def,
        Err(err) => {
            file.errors.push(err);
            return file;
        }
    };
    file.id = Some(def.agent.id.clone());
    let ctx = ValidationContext { source: origin.source, code_ids, raw_len };
    file.errors = validate(&def, &ctx);
    if file.errors.is_empty() {
        file.fingerprint = Some(fingerprint(&def, origin.source, project, path));
        file.definition = Some(def);
    }
    file
}

#[derive(Debug, Default)]
pub struct DirLoad {
    pub files: Vec<LoadedFile>,
    pub issues: Vec<ScanIssue>,
}

/// Scan one directory (SRC-6) and load every file in it.
pub fn load_dir(dir: &Path, origin: Origin, project: Option<&str>, code_ids: &[&str]) -> DirLoad {
    let scan = scan_dir(dir);
    let files = scan
        .files
        .iter()
        .map(|RawFile { path, text, raw_len }| load_text(path, text, *raw_len, origin, project, code_ids))
        .collect();
    DirLoad { files, issues: scan.issues }
}

/// A valid definition that lost to another with the same id (REG-8): reported, never dropped.
#[derive(Debug, Clone)]
pub struct Shadowed {
    pub file: LoadedFile,
    pub shadowed_by: PathBuf,
}

#[derive(Debug, Default)]
pub struct Resolution {
    /// One valid definition per id: the REG-8 winner.
    pub active: Vec<LoadedFile>,
    pub shadowed: Vec<Shadowed>,
    /// Files with errors; they never shadow anything.
    pub invalid: Vec<LoadedFile>,
}

/// REG-8: project > extra (list order) > user > bundled; ties inside one source go to the path
/// that sorts first, so the result does not depend on directory order. The caller passes project
/// files only for runs in that project (SRC-7), which keeps project shadowing run-local.
pub fn resolve(files: Vec<LoadedFile>) -> Resolution {
    let (mut valid, invalid): (Vec<_>, Vec<_>) = files.into_iter().partition(LoadedFile::is_valid);
    valid.sort_by(|a, b| {
        b.origin.priority().cmp(&a.origin.priority()).then_with(|| a.path.cmp(&b.path))
    });
    let mut out = Resolution { invalid, ..Resolution::default() };
    for file in valid {
        let winner = out.active.iter().find(|w| w.id == file.id).map(|w| w.path.clone());
        match winner {
            Some(shadowed_by) => out.shadowed.push(Shadowed { file, shadowed_by }),
            None => out.active.push(file),
        }
    }
    out
}

// ---- registration -------------------------------------------------------------------------

/// What a definition is doing right now (SPEC UI-3 wire names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefState {
    /// Registered and spawnable.
    Active,
    /// Valid, but the user has not approved this file (APR-1).
    NeedsApproval,
    /// Approved earlier; the fingerprint changed since (APR-4).
    ApprovalStale,
    /// Valid but another definition with the same id wins (REG-8).
    Shadowed,
    /// Failed parse or validation.
    Invalid,
}

impl DefState {
    pub fn as_str(self) -> &'static str {
        match self {
            DefState::Active => "active",
            DefState::NeedsApproval => "needs_approval",
            DefState::ApprovalStale => "approval_stale",
            DefState::Shadowed => "shadowed",
            DefState::Invalid => "invalid",
        }
    }
}

/// One row of `GET /agents/definitions` (T19 serialises it).
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub id: Option<String>,
    pub source: &'static str,
    pub state: DefState,
    pub approval: Option<ApprovalState>,
    pub fingerprint: Option<String>,
    pub errors: Vec<DefError>,
    pub shadowed_by: Option<PathBuf>,
    /// Display facts of the parsed definition, for `GET /agents/types` (absent when it did not parse).
    pub meta: Option<EntryMeta>,
    /// Approved, but the binary now resolves elsewhere than the path pinned at approval (SEC-4):
    /// the next spawn fails with `AGENT_DEF_BINARY_CHANGED` until the user approves again.
    pub binary_changed: bool,
}

/// What `GET /agents/types` shows about a declarative agent beyond the profile's own features.
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub label: String,
    pub icon: String,
    pub install_hint: Option<String>,
    pub login_hint: Option<String>,
    pub has_tui: bool,
    pub experimental: bool,
}

impl EntryMeta {
    fn of(def: &Definition) -> Self {
        let hint = def.binary.install_hint.trim();
        Self {
            label: def.agent.label.clone(),
            icon: def.agent.icon_or_default(),
            install_hint: (!hint.is_empty()).then(|| hint.to_string()),
            login_hint: Some(def.binary.login_hint.trim())
                .filter(|h| !h.is_empty())
                .map(str::to_string),
            has_tui: def.tui.is_some() || def.agent.kind == super::schema::Kind::Generic,
            experimental: def.agent.experimental || def.agent.kind == super::schema::Kind::Generic,
        }
    }
}

pub struct LoadInputs<'a> {
    /// Embedded bundled definitions: (file name, text). Trusted without approval (SRC-1).
    pub bundled: &'a [(&'a str, &'a str)],
    /// `<app data>/agents` (SRC-2); `None` skips the source.
    pub user_dir: Option<&'a Path>,
    /// Ids of the compiled-in profiles (VAL-1, REG-7).
    pub code_ids: &'a [&'a str],
    /// Consented project roots whose `.puppet-master/agents` is scanned (SRC-3). The caller
    /// passes only paths the user consented to; nothing else is ever read.
    pub projects: &'a [PathBuf],
    /// Consented extra directories with their position in the settings list (SRC-4).
    pub extra: &'a [(usize, PathBuf)],
}

/// `LoadInputs` backing storage for the real app: the settings decide which dirs exist at all.
pub struct OwnedInputs {
    pub user_dir: Option<PathBuf>,
    pub projects: Vec<PathBuf>,
    pub extra: Vec<(usize, PathBuf)>,
    pub code_ids: Vec<&'static str>,
}

impl OwnedInputs {
    pub fn from_settings(settings: &super::sources::SourceSettings, code_ids: Vec<&'static str>) -> Self {
        Self {
            user_dir: settings.sources.user.enabled.then(crate::app_paths::agents_dir),
            projects: settings.active_projects(),
            extra: settings.active_extra(),
            code_ids,
        }
    }

    /// The settings file and the process registry, as they are now.
    pub fn current() -> Self {
        let settings = super::sources::SourceSettings::load(&super::sources::settings_path());
        Self::from_settings(&settings, crate::tui::registry::registry().code_ids())
    }

    pub fn inputs(&self) -> LoadInputs<'_> {
        LoadInputs {
            bundled: &[],
            user_dir: self.user_dir.as_deref(),
            code_ids: &self.code_ids,
            projects: &self.projects,
            extra: &self.extra,
        }
    }
}

/// Every file of every enabled, consented source, unresolved (also what `approve` searches).
pub fn load_all(inputs: &LoadInputs<'_>) -> (Vec<LoadedFile>, Vec<ScanIssue>) {
    let mut files = Vec::new();
    let mut issues = Vec::new();
    for (name, text) in inputs.bundled {
        let path = PathBuf::from(format!("bundled/{name}"));
        files.push(load_text(&path, text, text.len(), Origin::new(Source::Bundled), None, inputs.code_ids));
    }
    let mut dirs: Vec<(PathBuf, Origin, Option<String>)> = Vec::new();
    for project in inputs.projects {
        let key = crate::standby::key::canonical_project(&project.to_string_lossy());
        dirs.push((super::sources::project_agents_dir(project), Origin::new(Source::Project), Some(key)));
    }
    for (index, dir) in inputs.extra {
        dirs.push((dir.clone(), Origin::extra(*index), None));
    }
    if let Some(dir) = inputs.user_dir {
        dirs.push((dir.to_path_buf(), Origin::new(Source::User), None));
    }
    for (dir, origin, project) in dirs {
        let loaded = load_dir(&dir, origin, project.as_deref(), inputs.code_ids);
        files.extend(loaded.files);
        issues.extend(loaded.issues);
    }
    (files, issues)
}

pub struct Built {
    pub entries: Vec<Entry>,
    /// Profiles for every `Active` entry, in load order.
    pub profiles: Vec<Arc<dyn TuiProfile>>,
    pub issues: Vec<ScanIssue>,
}

/// Load every enabled source, resolve precedence and approvals, and build the profiles that may be
/// registered. Pure: reads files, registers nothing.
pub fn build(inputs: &LoadInputs<'_>, approvals: &Approvals) -> Built {
    let (files, issues) = load_all(inputs);
    let resolution = resolve(files);
    let mut entries = Vec::new();
    let mut profiles: Vec<Arc<dyn TuiProfile>> = Vec::new();
    for file in resolution.active {
        let approval = match (file.origin.source, &file.fingerprint) {
            (Source::Bundled, _) => Some(ApprovalState::Approved),
            (_, Some(fingerprint)) => Some(approvals.state(&file.path, fingerprint)),
            _ => None,
        };
        let state = match approval {
            Some(ApprovalState::Approved) => DefState::Active,
            Some(ApprovalState::Stale) => DefState::ApprovalStale,
            _ => DefState::NeedsApproval,
        };
        let mut errors = file.errors.clone();
        let mut state = state;
        if state == DefState::Active {
            match file.definition.clone().map(DeclarativeProfile::new) {
                Some(Ok(profile)) => {
                    let profile = match &file.fingerprint {
                        Some(fingerprint) => profile.with_origin(DefinitionOrigin {
                            source: file.origin.source,
                            fingerprint: fingerprint.clone(),
                            path: file.path.clone(),
                            project: file.project.clone(),
                        }),
                        None => profile,
                    };
                    profiles.push(Arc::new(profile))
                }
                Some(Err(err)) => {
                    errors.push(err);
                    state = DefState::Invalid;
                }
                None => state = DefState::Invalid,
            }
        }
        let binary_changed = state == DefState::Active && file.origin.source != Source::Bundled && pinned_binary_moved(&file, approvals);
        entries.push(Entry {
            path: file.path.clone(),
            id: file.id.clone(),
            source: file.source_name(),
            state,
            approval,
            fingerprint: file.fingerprint.clone(),
            errors,
            shadowed_by: None,
            meta: file.definition.as_ref().map(EntryMeta::of),
            binary_changed,
        });
    }
    for shadowed in resolution.shadowed {
        entries.push(Entry {
            path: shadowed.file.path.clone(),
            id: shadowed.file.id.clone(),
            source: shadowed.file.source_name(),
            state: DefState::Shadowed,
            approval: None,
            fingerprint: shadowed.file.fingerprint.clone(),
            errors: Vec::new(),
            shadowed_by: Some(shadowed.shadowed_by),
            meta: shadowed.file.definition.as_ref().map(EntryMeta::of),
            binary_changed: false,
        });
    }
    for file in resolution.invalid {
        entries.push(Entry {
            path: file.path.clone(),
            id: file.id.clone(),
            source: file.source_name(),
            state: DefState::Invalid,
            approval: None,
            fingerprint: None,
            errors: file.errors,
            shadowed_by: None,
            meta: None,
            binary_changed: false,
        });
    }
    Built { entries, profiles, issues }
}

/// SEC-4 at list time: does the binary resolve to a different path than the approval pinned?
fn pinned_binary_moved(file: &LoadedFile, approvals: &Approvals) -> bool {
    let (Some(def), Some(record)) = (&file.definition, approvals.get(&file.path)) else { return false };
    let here = file.path.parent().unwrap_or_else(|| Path::new("."));
    match super::resolve::resolve_binary(&def.binary, here) {
        Ok(now) => {
            let canon = crate::standby::key::canonical_project;
            canon(&now.to_string_lossy()) != canon(&record.binary)
        }
        Err(_) => false,
    }
}

/// Build from the real app dirs and approvals file, then swap the result into `registry`.
pub fn reload_into(registry: &Registry, inputs: &LoadInputs<'_>, approvals_path: &Path) -> Built {
    let built = build(inputs, &Approvals::load(approvals_path));
    registry.set_declarative(built.profiles.clone());
    built
}

/// The rows of the last registry reload, for `GET /agents/types` (source, state, label).
static LAST: std::sync::RwLock<Vec<Entry>> = std::sync::RwLock::new(Vec::new());

/// The row of the last reload for agent `id`, if a definition with that id was loaded.
pub fn last_entry_for(id: &str) -> Option<Entry> {
    LAST.read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|e| e.id.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(id)) && e.state != DefState::Shadowed)
        .cloned()
}

/// Rows of the last reload (what the registry was built from).
pub fn last_entries() -> Vec<Entry> {
    LAST.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Remember `entries` as the rows of the last reload (the watcher swaps the registry itself).
pub fn store_last(entries: Vec<Entry>) {
    *LAST.write().unwrap_or_else(|e| e.into_inner()) = entries;
}

/// `reload_into` the process-wide registry and remember the rows (see `last_entry_for`).
pub fn reload_global(inputs: &LoadInputs<'_>, approvals_path: &Path) -> Built {
    let built = reload_into(crate::tui::registry::registry(), inputs, approvals_path);
    *LAST.write().unwrap_or_else(|e| e.into_inner()) = built.entries.clone();
    built
}

/// P4-2 "Add as generic agent": writes `<dir>/<id>.toml` for a bare command name. The file is only
/// written, never approved: it shows up as `unapproved` after the next reload. Returns the path.
pub fn write_generic(dir: &Path, command: &str, code_ids: &[&str]) -> Result<PathBuf, String> {
    let command = command.trim();
    let safe = !command.is_empty()
        && command.len() <= 64
        && command.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if !safe || command.starts_with('.') {
        return Err("AGENT_DEF_BAD_BINARY: enter a bare command name such as `mycoder` (letters, digits, . _ - +)".into());
    }
    let stem = command.rsplit_once('.').filter(|(_, ext)| ["exe", "cmd", "bat", "ps1", "com"].contains(ext)).map_or(command, |(s, _)| s);
    let mut id: String = stem.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if !id.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        id.insert(0, 'a');
    }
    id.truncate(32);
    let label: String = stem.chars().take(40).collect();
    let text = format!(
        "# Generic experimental agent added from Settings > Agents. It runs `{command}` in a visible pane
         # and reads the screen with shared heuristics only. Review it, then approve it.
         schema_version = 1

[agent]
id = \"{id}\"
label = \"{label}\"
kind = \"generic\"
experimental = true

         [binary]
names = [\"{command}\"]
install_hint = \"install {command} and make sure it is on PATH\"
"
    );
    let loaded = load_text(Path::new(&format!("{id}.toml")), &text, text.len(), Origin::new(Source::User), None, code_ids);
    if let Some(error) = loaded.errors.first() {
        return Err(format!("{}: {}", error.code, error.message));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the agents folder: {e}"))?;
    let path = dir.join(format!("{id}.toml"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => format!("AGENT_DEF_DUPLICATE_ID: {} already exists", path.display()),
            _ => format!("cannot write {}: {e}", path.display()),
        })?;
    std::io::Write::write_all(&mut file, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Startup / manual reload against the process-wide registry and the app data dir. Bundled
/// definitions arrive with T21; until then only user definitions can register.
pub fn reload() -> Built {
    let owned = OwnedInputs::current();
    let built = reload_global(&owned.inputs(), &super::approvals::default_path());
    for entry in &built.entries {
        tracing::info!(
            file = %entry.path.display(),
            id = entry.id.as_deref().unwrap_or(""),
            source = entry.source,
            state = entry.state.as_str(),
            errors = entry.errors.len(),
            "agent definition loaded"
        );
    }
    built
}

// ---------------------------------------------------------------------------------------------
// Sharing (P5-6): export / import one definition. Local files only; there is no network code here.
// ---------------------------------------------------------------------------------------------

/// What `export_definition` wrote.
#[derive(Debug, Clone)]
pub struct Exported {
    pub toml_path: PathBuf,
    /// `<target stem>.fixtures.zip`, written only when `<id>.fixtures/` exists next to the definition.
    pub fixtures_zip: Option<PathBuf>,
}

const MAX_FIXTURE_FILES: usize = 200;
const MAX_FIXTURE_BYTES: usize = 8 * 1024 * 1024;

/// Copy the user-dir definition `id` to `target` (a `.toml` file the user chose). If a sibling
/// folder `<id>.fixtures/` exists, its files are also written as a stored zip next to `target`.
/// Only the chosen target files are written.
pub fn export_definition(user_dir: &Path, id: &str, target: &Path) -> Result<Exported, String> {
    let (path, text) = scan_dir(user_dir)
        .files
        .into_iter()
        .find_map(|raw| (parse_definition(&raw.text).ok()?.agent.id == id).then_some((raw.path, raw.text)))
        .ok_or_else(|| format!("AGENT_DEF_NOT_FOUND: no definition with id {id} in {}", user_dir.display()))?;
    if target.extension().is_none_or(|e| !e.eq_ignore_ascii_case("toml")) {
        return Err("INVALID_ARGUMENT: choose a target file ending in .toml".into());
    }
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(target, &text).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let fixtures_dir = path.with_file_name(format!("{id}.fixtures"));
    let mut fixtures_zip = None;
    if fixtures_dir.is_dir() {
        let entries = collect_fixture_files(&fixtures_dir)?;
        if !entries.is_empty() {
            let stem = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| id.to_string());
            let zip_path = target.with_file_name(format!("{stem}.fixtures.zip"));
            std::fs::write(&zip_path, stored_zip(&entries))
                .map_err(|e| format!("cannot write {}: {e}", zip_path.display()))?;
            fixtures_zip = Some(zip_path);
        }
    }
    Ok(Exported { toml_path: target.to_path_buf(), fixtures_zip })
}

fn collect_fixture_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((current, prefix)) = stack.pop() {
        let mut items: Vec<_> = std::fs::read_dir(&current)
            .map_err(|e| format!("cannot read {}: {e}", current.display()))?
            .filter_map(Result::ok)
            .collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = format!("{prefix}{}", item.file_name().to_string_lossy());
            let Ok(meta) = std::fs::symlink_metadata(item.path()) else { continue };
            if meta.is_dir() {
                stack.push((item.path(), format!("{name}/")));
            } else if meta.is_file() {
                total += meta.len() as usize;
                if out.len() >= MAX_FIXTURE_FILES || total > MAX_FIXTURE_BYTES {
                    return Err("INVALID_ARGUMENT: the fixtures folder is too large to export".into());
                }
                out.push((name, std::fs::read(item.path()).map_err(|e| e.to_string())?));
            }
        }
    }
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A zip with every file stored (no compression), written without a dependency.
pub fn stored_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let (size, name_len) = (data.len() as u32, name.len() as u16);
        let header = |sig: u32, is_central: bool| {
            let mut h: Vec<u8> = Vec::new();
            h.extend(sig.to_le_bytes());
            if is_central {
                h.extend(20u16.to_le_bytes()); // made by
            }
            h.extend(20u16.to_le_bytes()); // needed
            h.extend(0x0800u16.to_le_bytes()); // utf-8 names
            h.extend(0u16.to_le_bytes()); // stored
            h.extend(0u16.to_le_bytes()); // time
            h.extend(0x21u16.to_le_bytes()); // date 1980-01-01
            h.extend(crc.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend(size.to_le_bytes());
            h.extend(name_len.to_le_bytes());
            h.extend(0u16.to_le_bytes()); // extra
            if is_central {
                h.extend([0u8; 2 + 2 + 2 + 4]); // comment, disk, internal attrs, external attrs
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(data);
        central.extend(header(0x0201_4b50, true));
    }
    let central_offset = out.len() as u32;
    let central_len = central.len() as u32;
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend(central_len.to_le_bytes());
    out.extend(central_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// Validate `source` (a `.toml` file) as a user definition and place it as `<user_dir>/<id>.toml`.
/// Nothing is approved here: approvals are keyed by file path + fingerprint, so a new file has none
/// and shows up as `unapproved` after the next reload. Never overwrites an existing file.
pub fn import_definition(user_dir: &Path, source: &Path, code_ids: &[&str]) -> Result<PathBuf, String> {
    if source.extension().is_none_or(|e| !e.eq_ignore_ascii_case("toml")) {
        return Err("INVALID_ARGUMENT: choose a definition file ending in .toml".into());
    }
    let meta = std::fs::metadata(source).map_err(|e| format!("AGENT_DEF_NOT_FOUND: cannot read {}: {e}", source.display()))?;
    let limit = super::validate::MAX_DEFINITION_BYTES;
    if !meta.is_file() || meta.len() as usize > limit {
        return Err(format!("AGENT_DEF_TOO_LARGE: a definition file is at most {limit} bytes"));
    }
    let text = std::fs::read_to_string(source).map_err(|e| format!("AGENT_DEF_INVALID_TOML: {e}"))?;
    let loaded = load_text(Path::new("import.toml"), &text, text.len(), Origin::new(Source::User), None, code_ids);
    if let Some(error) = loaded.errors.first() {
        return Err(format!("{}: {}", error.code, error.message));
    }
    let id = loaded.id.ok_or_else(|| "AGENT_DEF_INVALID_TOML: no agent id".to_string())?;
    std::fs::create_dir_all(user_dir).map_err(|e| format!("cannot create the agents folder: {e}"))?;
    let path = user_dir.join(format!("{id}.toml"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => format!("AGENT_DEF_DUPLICATE_ID: {} already exists", path.display()),
            _ => format!("cannot write {}: {e}", path.display()),
        })?;
    std::io::Write::write_all(&mut file, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}
