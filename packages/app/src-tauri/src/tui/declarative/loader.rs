//! Load definition text into reports (parse, validate, fingerprint) and apply REG-8 precedence.
//!
//! Pure with respect to the registry: nothing is registered and no approval is consulted (T18).
//! A bad file never aborts the others; every problem is a `DefError` on its own report.

use std::path::{Path, PathBuf};

use super::fingerprint::fingerprint;
use super::schema::{parse_definition, DefError, Definition};
use super::sources::{scan_dir, source_name, Origin, RawFile, ScanIssue};
use super::validate::{validate, ValidationContext};

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
