//! Read librarian project IR written by `scripts/project-indexer.py`.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::event_log;

const IR_FILE: &str = "project-ir.json";
const META_FILE: &str = "project-ir.meta.json";
pub const OVERVIEW_CAP_BYTES: usize = 4096;
pub const DEFAULT_INDEXER_REL: &str = "scripts/project-indexer.py";
pub const LIBRARIAN_OPENCODE_MODES: &[&str] = &["opencode", "opencode_native", "prompt"];

#[derive(Debug, Clone, Deserialize)]
struct ProjectIrFile {
    overview: String,
    #[serde(default)]
    generated_at_ms: Option<i64>,
    #[serde(default)]
    generator: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectIrMetaFile {
    #[serde(default)]
    git_sha: Option<String>,
    #[serde(default)]
    generated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIrExcerpt {
    pub overview: String,
    pub git_sha: Option<String>,
    pub generated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProjectIrStatus {
    pub ir_exists: bool,
    pub git_sha: Option<String>,
    pub indexed_git_sha: Option<String>,
    pub stale: bool,
    pub generated_at_ms: Option<i64>,
    pub indexer_command: String,
}

pub fn ir_dir(project_root: &Path) -> PathBuf {
    event_log::project_storage_dir(project_root)
}

pub fn indexer_command(custom_path: Option<&str>) -> String {
    let raw = custom_path.map(str::trim).filter(|value| !value.is_empty());
    if raw.is_some_and(|value| LIBRARIAN_OPENCODE_MODES.contains(&value)) {
        return "read_librarian_prompt → write_terminal_input on opencode_native".into();
    }
    let script = raw.unwrap_or("opencode");
    if LIBRARIAN_OPENCODE_MODES.contains(&script) {
        return "read_librarian_prompt → write_terminal_input on opencode_native".into();
    }
    if script == DEFAULT_INDEXER_REL || script.ends_with("project-indexer.py") {
        return format!("python {DEFAULT_INDEXER_REL}");
    }
    format!("python {script}")
}

pub fn render_librarian_prompt(project_root: &Path) -> Result<String, String> {
    let script = project_root.join("scripts/project-indexer.py");
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }
    let output = std::process::Command::new("python")
        .arg(&script)
        .arg("--emit-prompt")
        .current_dir(project_root)
        .output()
        .map_err(|err| format!("run project-indexer.py --emit-prompt: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "project-indexer.py --emit-prompt exited with {}",
                output.status
            )
        } else {
            stderr
        });
    }
    String::from_utf8(output.stdout).map_err(|err| format!("librarian prompt not utf-8: {err}"))
}

pub fn read_overview_excerpt(project_root: &Path) -> Option<ProjectIrExcerpt> {
    let ir_path = ir_dir(project_root).join(IR_FILE);
    if !ir_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&ir_path).ok()?;
    let ir: ProjectIrFile = serde_json::from_str(&raw).ok()?;
    let meta_path = ir_dir(project_root).join(META_FILE);
    let meta: ProjectIrMetaFile = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(ProjectIrMetaFile {
            git_sha: None,
            generated_at_ms: None,
        });
    let overview = truncate_utf8(&ir.overview, OVERVIEW_CAP_BYTES);
    Some(ProjectIrExcerpt {
        overview,
        git_sha: meta.git_sha,
        generated_at_ms: ir.generated_at_ms.or(meta.generated_at_ms),
    })
}

pub fn status(project_root: &Path, custom_indexer_path: Option<&str>) -> ProjectIrStatus {
    let excerpt = read_overview_excerpt(project_root);
    let current_sha = current_git_sha(project_root);
    let indexed_sha = excerpt.as_ref().and_then(|e| e.git_sha.clone());
    let stale = match (&current_sha, &indexed_sha) {
        (Some(current), Some(indexed)) => current != indexed,
        (Some(_), None) => true,
        _ => false,
    };
    ProjectIrStatus {
        ir_exists: excerpt.is_some(),
        git_sha: current_sha,
        indexed_git_sha: indexed_sha,
        stale,
        generated_at_ms: excerpt.and_then(|e| e.generated_at_ms),
        indexer_command: indexer_command(custom_indexer_path),
    }
}

fn current_git_sha(project_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn truncate_utf8_respects_char_boundaries() {
        let text = "café".repeat(500);
        let truncated = truncate_utf8(&text, 100);
        assert!(truncated.ends_with('…'));
        assert!(truncated.len() <= 104);
    }

    #[test]
    fn reads_overview_excerpt_from_project_storage() {
        let dir = std::env::temp_dir().join(format!(
            "pm-project-ir-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let storage = dir.join(".puppet-master");
        fs::create_dir_all(&storage).expect("mkdir");
        fs::write(
            storage.join(IR_FILE),
            r#"{"overview":"hello project","generated_at_ms":123}"#,
        )
        .expect("write ir");
        fs::write(
            storage.join(META_FILE),
            r#"{"git_sha":"abc123","generated_at_ms":123}"#,
        )
        .expect("write meta");

        let excerpt = read_overview_excerpt(&dir).expect("excerpt");
        assert_eq!(excerpt.overview, "hello project");
        assert_eq!(excerpt.git_sha.as_deref(), Some("abc123"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn caps_overview_at_four_kb() {
        let dir = std::env::temp_dir().join(format!(
            "pm-project-ir-cap-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let storage = dir.join(".puppet-master");
        fs::create_dir_all(&storage).expect("mkdir");
        let big = "x".repeat(5000);
        fs::write(
            storage.join(IR_FILE),
            format!(r#"{{"overview":"{big}"}}"#),
        )
        .expect("write ir");

        let excerpt = read_overview_excerpt(&dir).expect("excerpt");
        assert!(excerpt.overview.len() <= OVERVIEW_CAP_BYTES + 4);

        let _ = fs::remove_dir_all(&dir);
    }
}
