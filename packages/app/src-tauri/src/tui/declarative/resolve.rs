//! Binary resolution for definitions (SPEC 7.3, SEC-3). Scans `PATH` explicitly and never the
//! current or project directory. On Windows a native `.exe`/`.com` wins; an npm `.cmd`/`.ps1`
//! shim is followed to the native binary through `shim_native_globs`; a shim with no native
//! binary is refused (`AGENT_DEF_SHIM_NEEDS_NATIVE`) unless the definition opts in with
//! `allow_cmd_shim` (which the validator only permits for stdin task delivery).

use std::path::{Path, PathBuf};

use super::schema::BinaryTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Unix,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(windows) { Platform::Windows } else { Platform::Unix }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Nothing runnable on `PATH`.
    NotFound { tried: Vec<String> },
    /// Only a `.cmd`/`.bat`/`.ps1` shim exists and no native binary could be reached from it.
    ShimNeedsNative { shim: PathBuf },
}

impl ResolveError {
    /// SPEC 7.4 / run-time code.
    pub fn code(&self) -> &'static str {
        match self {
            ResolveError::NotFound { .. } => "AGENT_NOT_INSTALLED",
            ResolveError::ShimNeedsNative { .. } => "AGENT_DEF_SHIM_NEEDS_NATIVE",
        }
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound { tried } => write!(f, "none of [{}] was found on PATH", tried.join(", ")),
            ResolveError::ShimNeedsNative { shim } => write!(
                f,
                "{} is a script shim and no native binary was found next to it (cmd.exe quoting is unsafe for task text)",
                shim.display()
            ),
        }
    }
}

/// `PATH` entries that may be searched: absolute, existing and not one of `exclude`
/// (the current and project directories). Relative and empty entries resolve against the cwd and
/// are dropped.
pub fn searchable_dirs(path_var: &std::ffi::OsStr, exclude: &[PathBuf]) -> Vec<PathBuf> {
    let excluded: Vec<PathBuf> = exclude.iter().filter_map(|p| canonical(p)).collect();
    std::env::split_paths(path_var)
        .filter(|dir| dir.is_absolute())
        .filter(|dir| match canonical(dir) {
            Some(real) => !excluded.contains(&real),
            None => false,
        })
        .collect()
}

fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Resolve against `PATH` from the environment, excluding the process cwd and `project_dir`.
pub fn resolve_binary(binary: &BinaryTable, project_dir: &Path) -> Result<PathBuf, ResolveError> {
    resolve_in(binary, &path_dirs(project_dir), Platform::current())
}

/// The folders `resolve_binary` searches: the process `PATH` without the current and `project_dir` folders.
pub fn path_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut exclude = vec![project_dir.to_path_buf()];
    if let Ok(cwd) = std::env::current_dir() {
        exclude.push(cwd);
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    searchable_dirs(&path_var, &exclude)
}

/// Resolve against an explicit directory list (already filtered by `searchable_dirs`).
pub fn resolve_in(binary: &BinaryTable, dirs: &[PathBuf], platform: Platform) -> Result<PathBuf, ResolveError> {
    let mut refused: Option<PathBuf> = None;
    for name in &binary.names {
        match platform {
            Platform::Unix => {
                if let Some(hit) = dirs.iter().map(|d| d.join(name)).find(|p| is_executable(p)) {
                    return Ok(hit);
                }
            }
            Platform::Windows => {
                for ext in &binary.windows_extensions {
                    let file = format!("{name}.{ext}");
                    for dir in dirs {
                        let hit = dir.join(&file);
                        if !hit.is_file() {
                            continue;
                        }
                        match ext.to_ascii_lowercase().as_str() {
                            "exe" | "com" => return Ok(hit),
                            _ => match native_from_shim(&hit, &binary.shim_native_globs) {
                                Some(native) => return Ok(native),
                                None if binary.allow_cmd_shim && ext != "ps1" => return Ok(hit),
                                None => {
                                    refused.get_or_insert(hit);
                                }
                            },
                        }
                    }
                }
            }
        }
    }
    match refused {
        Some(shim) => Err(ResolveError::ShimNeedsNative { shim }),
        None => Err(ResolveError::NotFound { tried: binary.names.clone() }),
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// First existing `.exe` matching a `shim_native_globs` entry, resolved relative to the shim's
/// directory. Matches are sorted so the result is deterministic and must stay inside that
/// directory after symlink resolution.
fn native_from_shim(shim: &Path, globs: &[String]) -> Option<PathBuf> {
    let dir = shim.parent()?;
    let root = canonical(dir)?;
    let escaped_dir = glob::Pattern::escape(&dir.to_string_lossy());
    for pattern in globs {
        let full = format!("{escaped_dir}{}{}", std::path::MAIN_SEPARATOR, pattern.replace('\\', "/"));
        let Ok(paths) = glob::glob(&full) else { continue };
        let mut matches: Vec<PathBuf> = paths.filter_map(Result::ok).collect();
        matches.sort();
        for candidate in matches {
            let is_exe = candidate.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            if !is_exe || !candidate.is_file() {
                continue;
            }
            if canonical(&candidate).is_some_and(|real| real.starts_with(&root)) {
                return Some(candidate);
            }
        }
    }
    None
}
