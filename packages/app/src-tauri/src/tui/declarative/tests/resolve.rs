use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::KILO;
use crate::tui::declarative::resolve::{resolve_binary, resolve_in, searchable_dirs, Platform, ResolveError};
use crate::tui::declarative::schema::BinaryTable;
use crate::tui::declarative::parse_definition;

/// A unique temp tree. Cleanup removes only the files and directories the test recorded, one at a
/// time, deepest first (no recursive delete).
struct Tree {
    root: PathBuf,
    files: Vec<PathBuf>,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("pm-resolve-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&root).unwrap();
        Self { root, files: Vec::new() }
    }

    fn file(&mut self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        self.files.push(path.clone());
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for file in &self.files {
            let _ = fs::remove_file(file);
            let mut parent = file.parent();
            while let Some(p) = parent {
                if p == self.root.parent().unwrap() {
                    break;
                }
                dirs.push(p.to_path_buf());
                parent = p.parent();
            }
        }
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        dirs.dedup();
        for dir in dirs {
            let _ = fs::remove_dir(dir);
        }
    }
}

fn kilo_binary() -> BinaryTable {
    parse_definition(KILO).unwrap().binary
}

const NATIVE: &str = "node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64/bin/kilo.exe";

fn names(dirs: &[&Path]) -> Vec<PathBuf> {
    dirs.iter().map(|d| d.to_path_buf()).collect()
}

#[test]
fn windows_exe_beats_shim() {
    let mut t = Tree::new();
    let npm = t.dir("npm");
    let bin = t.dir("bin");
    t.file("npm/kilo.cmd");
    t.file(NATIVE.replace("node_modules", "npm/node_modules").as_str());
    let exe = t.file("bin/kilo.exe");
    // a real kilo.exe on PATH wins over a shim in an earlier directory
    let found = resolve_in(&kilo_binary(), &names(&[&npm, &bin]), Platform::Windows).unwrap();
    assert_eq!(found, exe);
}

#[test]
fn shim_resolves_to_native_through_glob() {
    let mut t = Tree::new();
    let npm = t.dir("npm");
    t.file("npm/kilo.cmd");
    let native = t.file(&format!("npm/{NATIVE}"));
    // the baseline package also matches `cli-windows-x64*`; the plain one sorts first
    t.file("npm/node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64-baseline/bin/kilo.exe");
    let found = resolve_in(&kilo_binary(), &names(&[&npm]), Platform::Windows).unwrap();
    assert_eq!(found, native);
}

#[test]
fn shim_only_is_refused() {
    let mut t = Tree::new();
    let npm = t.dir("npm");
    let shim = t.file("npm/kilo.cmd");
    let err = resolve_in(&kilo_binary(), &names(&[&npm]), Platform::Windows).unwrap_err();
    assert_eq!(err.code(), "AGENT_DEF_SHIM_NEEDS_NATIVE");
    assert_eq!(err, ResolveError::ShimNeedsNative { shim });

    // a .ps1 shim is never spawnable, even with allow_cmd_shim
    let mut binary = kilo_binary();
    binary.windows_extensions = vec!["ps1".into()];
    binary.allow_cmd_shim = true;
    t.file("npm/kilo.ps1");
    let err = resolve_in(&binary, &names(&[&npm]), Platform::Windows).unwrap_err();
    assert_eq!(err.code(), "AGENT_DEF_SHIM_NEEDS_NATIVE");
}

#[test]
fn allow_cmd_shim_accepts_the_shim() {
    let mut t = Tree::new();
    let npm = t.dir("npm");
    let shim = t.file("npm/kilo.cmd");
    let mut binary = kilo_binary();
    binary.allow_cmd_shim = true;
    assert_eq!(resolve_in(&binary, &names(&[&npm]), Platform::Windows).unwrap(), shim);
}

#[test]
fn not_found_names_the_candidates() {
    let mut t = Tree::new();
    let empty = t.dir("empty");
    t.file("empty/unrelated.exe");
    let err = resolve_in(&kilo_binary(), &names(&[&empty]), Platform::Windows).unwrap_err();
    assert_eq!(err.code(), "AGENT_NOT_INSTALLED");
    assert_eq!(err, ResolveError::NotFound { tried: vec!["kilo".into(), "kilocode".into()] });
}

#[test]
fn second_name_is_tried_in_order() {
    let mut t = Tree::new();
    let bin = t.dir("bin");
    let alt = t.file("bin/kilocode.exe");
    assert_eq!(resolve_in(&kilo_binary(), &names(&[&bin]), Platform::Windows).unwrap(), alt);
}

#[test]
fn cwd_not_searched() {
    let mut t = Tree::new();
    let trusted = t.dir("trusted");
    let planted_cwd = t.dir("cwd");
    let project = t.dir("project");
    t.file("cwd/kilo.exe");
    t.file("project/kilo.exe");
    let real = t.file("trusted/kilo.exe");
    let path_var = std::env::join_paths([&planted_cwd, &project, &trusted, Path::new("."), Path::new("")]).unwrap();
    let dirs = searchable_dirs(&path_var, &[project.clone(), planted_cwd.clone()]);
    assert_eq!(dirs, vec![trusted.clone()], "cwd, project, relative and empty entries are dropped");
    assert_eq!(resolve_in(&kilo_binary(), &dirs, Platform::Windows).unwrap(), real);

    // only planted copies exist: nothing is found
    let only_planted = searchable_dirs(&std::env::join_paths([&planted_cwd, &project]).unwrap(), &[project.clone(), planted_cwd.clone()]);
    assert!(resolve_in(&kilo_binary(), &only_planted, Platform::Windows).is_err());
}

#[test]
fn resolve_binary_skips_the_project_directory() {
    // The project dir holds a planted binary; PATH is the process PATH, so the planted copy must
    // never be returned regardless of what is installed.
    let mut t = Tree::new();
    let project = t.dir("project");
    let planted = t.file("project/pm-definitely-not-installed.exe");
    let binary = BinaryTable {
        names: vec!["pm-definitely-not-installed".into()],
        windows_extensions: vec!["exe".into()],
        ..kilo_binary()
    };
    let result = resolve_binary(&binary, &project);
    assert_ne!(result.ok(), Some(planted));
}

#[cfg(unix)]
#[test]
fn unix_needs_the_execute_bit() {
    use std::os::unix::fs::PermissionsExt;
    let mut t = Tree::new();
    let bin = t.dir("bin");
    let tool = t.file("bin/kilo");
    assert_eq!(resolve_in(&kilo_binary(), &names(&[&bin]), Platform::Unix).unwrap(), tool);
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(resolve_in(&kilo_binary(), &names(&[&bin]), Platform::Unix).is_err());
}

/// Live check on a machine with Kilo installed through npm: `cargo test --lib -- --ignored real_kilo`.
#[test]
#[ignore = "needs a real npm install of @kilocode/cli on PATH"]
fn real_kilo_exe_is_found() {
    let project = std::env::temp_dir();
    let found = resolve_binary(&kilo_binary(), &project).expect("kilo resolves");
    println!("resolved kilo: {}", found.display());
    assert!(found.is_file());
    if cfg!(windows) {
        assert!(found.to_string_lossy().to_ascii_lowercase().ends_with("kilo.exe"));
    }
}
