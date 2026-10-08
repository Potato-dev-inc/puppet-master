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
}

/// What `GET /agents/types` shows about a declarative agent beyond the profile's own features.
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub label: String,
    pub icon: String,
    pub install_hint: Option<String>,
    pub has_tui: bool,
}

impl EntryMeta {
    fn of(def: &Definition) -> Self {
        let hint = def.binary.install_hint.trim();
        Self {
            label: def.agent.label.clone(),
            icon: def.agent.icon_or_default(),
            install_hint: (!hint.is_empty()).then(|| hint.to_string()),
            has_tui: def.tui.is_some(),
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
    let mut files = Vec::new();
    let mut issues = Vec::new();
    for (name, text) in inputs.bundled {
        let path = PathBuf::from(format!("bundled/{name}"));
        files.push(load_text(&path, text, text.len(), Origin::new(Source::Bundled), None, inputs.code_ids));
    }
    if let Some(dir) = inputs.user_dir {
        let loaded = load_dir(dir, Origin::new(Source::User), None, inputs.code_ids);
        files.extend(loaded.files);
        issues.extend(loaded.issues);
    }
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
        });
    }
    Built { entries, profiles, issues }
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

/// `reload_into` the process-wide registry and remember the rows (see `last_entry_for`).
pub fn reload_global(inputs: &LoadInputs<'_>, approvals_path: &Path) -> Built {
    let built = reload_into(crate::tui::registry::registry(), inputs, approvals_path);
    *LAST.write().unwrap_or_else(|e| e.into_inner()) = built.entries.clone();
    built
}

/// Startup / manual reload against the process-wide registry and the app data dir. Bundled
/// definitions arrive with T21; until then only user definitions can register.
pub fn reload() -> Built {
    let registry = crate::tui::registry::registry();
    let code_ids = registry.code_ids();
    let user_dir = crate::app_paths::agents_dir();
    let inputs = LoadInputs { bundled: &[], user_dir: Some(&user_dir), code_ids: &code_ids };
    let built = reload_global(&inputs, &super::approvals::default_path());
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
