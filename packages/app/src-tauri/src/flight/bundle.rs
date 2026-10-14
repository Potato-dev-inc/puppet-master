//! The diagnostic file ("Save a diagnostic file" in Settings > General).
//!
//! A zip of: a plain README, app and system versions, the settings with secrets removed, the tail of
//! the app log, and the newest frozen flight recordings of failed runs. Every file is made of
//! [`Redacted`] text (strict mode: secrets, long opaque runs and the user's folder name), so the
//! zip writer cannot be handed raw text. Nothing is uploaded: the file goes where the person saves it.
//!
//! `preview` lists what a save would contain; the person can leave any file out (`exclude`).

use super::{freeze, zip::ZipWriter};
use crate::redact::{self, compose, Part, Redacted};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

/// Newest recordings included.
const MAX_CASTS: usize = 5;
/// Log lines included.
const LOG_LINES: usize = 400;

/// One frozen recording read from the project folder.
pub struct CastFile {
    pub operation_id: String,
    pub modified_ms: u64,
    pub text: String,
}

/// Everything a bundle is made from. Gathered by [`gather`]; tests build it by hand.
pub struct Inputs {
    pub version: Value,
    pub settings: Value,
    pub log_lines: Vec<String>,
    pub casts: Vec<CastFile>,
    pub now_ms: u64,
}

pub struct BundleFile {
    pub path: String,
    /// Plain words for the preview.
    pub label: String,
    pub body: Redacted<String>,
}

pub struct Bundle {
    pub files: Vec<BundleFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PreviewEntry {
    pub path: String,
    pub label: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveReport {
    pub path: String,
    pub files: usize,
    pub bytes: u64,
}

const README: &str = "Puppet Master diagnostic file\n\
\n\
This file was saved on request. Puppet Master did not send it anywhere.\n\
\n\
What is inside:\n\
- version.json: app and system versions\n\
- settings.json: your settings, with keys, tokens and passwords removed\n\
- app-log.txt: the most recent lines of the app's own log\n\
- recordings/: terminal recordings of runs that failed (asciicast files; open them with any asciinema player)\n\
\n\
Passwords, API keys, tokens and your user folder name were masked before saving. \
Terminal text can still contain things only you would recognise, so look through it before you share it.\n";

/// Settings with every secret-looking field blanked.
pub fn strip_secret_fields(value: &mut Value) {
    fn secret_name(name: &str) -> bool {
        let n = name.to_ascii_lowercase();
        ["key", "token", "secret", "password", "passwd", "credential", "cookie", "auth", "bearer"]
            .iter()
            .any(|w| n.contains(w))
    }
    match value {
        Value::Object(map) => {
            for (name, v) in map.iter_mut() {
                if secret_name(name) && matches!(v, Value::String(_) | Value::Object(_) | Value::Array(_)) {
                    *v = Value::String("[removed]".into());
                } else {
                    strip_secret_fields(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_secret_fields),
        _ => {}
    }
}

fn ago(now_ms: u64, then_ms: u64) -> String {
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    match secs {
        0..=89 => "just now".into(),
        90..=5_399 => format!("{} minutes ago", (secs + 30) / 60),
        5_400..=129_599 => format!("{} hours ago", (secs + 1_800) / 3_600),
        _ => format!("{} days ago", (secs + 43_200) / 86_400),
    }
}

/// A frozen cast, line by line: each line is JSON, its strings are redacted again (the file may have
/// been written by an older build or edited), and the line is rebuilt.
fn cast_text(raw: &str) -> Redacted<String> {
    let lines: Vec<Redacted<String>> = raw
        .lines()
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(value) => redact::json_strict(value).to_text(),
            Err(_) => redact::strict(line),
        })
        .collect();
    let mut parts: Vec<Part<'_>> = Vec::with_capacity(lines.len() * 2);
    for line in &lines {
        parts.push(Part::Red(line));
        parts.push(Part::Lit("\n"));
    }
    compose(&parts)
}

pub fn build(inputs: Inputs) -> Bundle {
    let mut files: Vec<BundleFile> = Vec::new();
    let readme = redact::trusted_constant(README);
    files.push(BundleFile { path: "README.txt".into(), label: "A short note on what this file is".into(), body: readme });
    files.push(BundleFile {
        path: "version.json".into(),
        label: "App and system versions".into(),
        body: redact::json_strict(inputs.version).to_pretty_text(),
    });
    let mut settings = inputs.settings;
    strip_secret_fields(&mut settings);
    files.push(BundleFile {
        path: "settings.json".into(),
        label: "Your settings, with keys and tokens removed".into(),
        body: redact::json_strict(settings).to_pretty_text(),
    });
    let skip = inputs.log_lines.len().saturating_sub(LOG_LINES);
    let log: Vec<Redacted<String>> = inputs.log_lines.iter().skip(skip).map(|l| redact::strict(l)).collect();
    let mut parts: Vec<Part<'_>> = Vec::new();
    for line in &log {
        parts.push(Part::Red(line));
        parts.push(Part::Lit("\n"));
    }
    files.push(BundleFile {
        path: "app-log.txt".into(),
        label: format!("The last {} lines of the app's log", log.len()),
        body: compose(&parts),
    });
    let mut casts = inputs.casts;
    casts.sort_by_key(|c| std::cmp::Reverse(c.modified_ms));
    for cast in casts.into_iter().take(MAX_CASTS) {
        files.push(BundleFile {
            path: format!("recordings/{}.cast", cast.operation_id),
            label: format!("Recording of a run that failed, {}", ago(inputs.now_ms, cast.modified_ms)),
            body: cast_text(&cast.text),
        });
    }
    Bundle { files }
}

impl Bundle {
    pub fn preview(&self) -> Vec<PreviewEntry> {
        self.files
            .iter()
            .map(|f| PreviewEntry { path: f.path.clone(), label: f.label.clone(), bytes: f.body.len() })
            .collect()
    }

    /// The zip, without the files in `exclude` (by path). A manifest of what is inside is added.
    pub fn zip(&self, exclude: &[String]) -> std::io::Result<Vec<u8>> {
        let kept: Vec<&BundleFile> = self.files.iter().filter(|f| !exclude.contains(&f.path)).collect();
        let manifest = redact::json_strict(json!({
            "files": kept.iter().map(|f| json!({ "path": f.path, "what": f.label, "bytes": f.body.len() })).collect::<Vec<_>>(),
            "uploaded": false,
        }))
        .to_pretty_text();
        let mut zip = ZipWriter::new(Vec::new());
        zip.add("manifest.json", manifest.as_str().as_bytes())?;
        for file in kept {
            zip.add(&file.path, file.body.as_str().as_bytes())?;
        }
        zip.finish()
    }
}

/// Read the live app state into [`Inputs`]. `settings` is the public settings mirror.
pub fn gather(project: &str, settings: Value) -> Inputs {
    let info = crate::app_lifecycle::get_app_install_info();
    let version = json!({
        "app": "Puppet Master",
        "version": info.version,
        "packaged": info.is_packaged,
        "os": info.platform,
        "arch": std::env::consts::ARCH,
    });
    let casts = freeze::list_casts(project)
        .into_iter()
        .take(MAX_CASTS)
        .filter_map(|(operation_id, path, _, modified_ms)| {
            Some(CastFile { operation_id, modified_ms, text: std::fs::read_to_string(path).ok()? })
        })
        .collect();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Inputs { version, settings, log_lines: super::applog::tail(), casts, now_ms }
}

/// Write the zip to `dest` (via a temp file, so a failed save never leaves half a file).
pub fn save(bundle: &Bundle, exclude: &[String], dest: &Path) -> Result<SaveReport, String> {
    let bytes = bundle.zip(exclude).map_err(|e| format!("could not build the file: {e}"))?;
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let tmp = dest.with_extension("zip.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("could not save {}: {e}", dest.display()))?;
    if std::fs::rename(&tmp, dest).is_err() {
        let _ = std::fs::remove_file(dest);
        std::fs::rename(&tmp, dest).map_err(|e| format!("could not save {}: {e}", dest.display()))?;
    }
    let files = bundle.files.iter().filter(|f| !exclude.contains(&f.path)).count() + 1;
    Ok(SaveReport { path: dest.to_string_lossy().into_owned(), files, bytes: bytes.len() as u64 })
}
