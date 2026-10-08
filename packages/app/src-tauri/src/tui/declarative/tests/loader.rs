use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{KILO, MINIMAL};
use crate::tui::declarative::fingerprint::fingerprint;
use crate::tui::declarative::loader::{load_dir, load_text, resolve, LoadedFile};
use crate::tui::declarative::schema::parse_definition;
use crate::tui::declarative::sources::{project_agents_dir, scan_dir, Origin, MAX_FILES_PER_DIR};
use crate::tui::declarative::validate::Source;

/// A unique temp tree. Cleanup removes only what the test recorded, one entry at a time (no
/// recursive delete).
struct Tree {
    root: PathBuf,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir()
            .join(format!("pm-loader-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&root).unwrap();
        Self { dirs: vec![root.clone()], root, files: Vec::new() }
    }

    fn dir(&mut self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        self.dirs.push(path.clone());
        path
    }

    fn file(&mut self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::write(&path, text).unwrap();
        self.files.push(path.clone());
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

fn with_id(id: &str) -> String {
    KILO.replace("id = \"kilo\"", &format!("id = \"{id}\""))
}

fn load_one(text: &str, origin: Origin, path: &str) -> LoadedFile {
    load_text(Path::new(path), text, text.len(), origin, None, &["claude"])
}

#[test]
fn non_recursive_and_capped() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    let sub = t.dir("agents/nested");
    for i in 0..=MAX_FILES_PER_DIR {
        t.file(&format!("agents/a{i:03}.toml"), MINIMAL);
    }
    t.file("agents/readme.txt", "ignored");
    t.file("agents/nested/deep.toml", MINIMAL);
    let scan = scan_dir(&dir);
    assert_eq!(scan.files.len(), MAX_FILES_PER_DIR);
    assert!(scan.files.iter().all(|f| !f.path.starts_with(&sub)));
    assert_eq!(scan.issues.len(), 1);
    assert_eq!(scan.issues[0].code, "AGENT_DEF_TOO_MANY_FILES");
    assert!(scan.issues[0].path.ends_with("a064.toml"));
}

#[test]
fn missing_directory_is_an_empty_scan() {
    let scan = scan_dir(&std::env::temp_dir().join("pm-loader-does-not-exist-xyz"));
    assert!(scan.files.is_empty() && scan.issues.is_empty());
}

#[test]
fn oversize_file_is_loaded_as_too_large_not_read_whole() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    let big = format!("{MINIMAL}\n# {}\n", "x".repeat(200_000));
    t.file("agents/big.toml", &big);
    let loaded = load_dir(&dir, Origin::new(Source::User), None, &[]);
    assert_eq!(loaded.files.len(), 1);
    assert!(loaded.files[0].errors.iter().any(|e| e.code == "AGENT_DEF_TOO_LARGE"));
    assert!(!loaded.files[0].is_valid());
}

#[test]
fn symlink_guard() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    let outside = t.file("outside.toml", MINIMAL);
    let inside = t.file("agents/real.toml", MINIMAL);
    let link = dir.join("link.toml");
    let inner = dir.join("inner.toml");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &link).is_ok() && std::os::unix::fs::symlink(&inside, &inner).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&outside, &link).is_ok()
        && std::os::windows::fs::symlink_file(&inside, &inner).is_ok();
    if !made {
        eprintln!("skipping: symlinks cannot be created here");
        return;
    }
    t.files.push(link.clone());
    t.files.push(inner.clone());
    let scan = scan_dir(&dir);
    let names: Vec<_> = scan.files.iter().map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(!names.contains(&"link.toml".to_string()), "{names:?}");
    assert!(names.contains(&"real.toml".to_string()));
    assert!(names.contains(&"inner.toml".to_string()), "a link that stays inside the directory is fine");
    assert!(scan.issues.iter().any(|i| i.code == "AGENT_DEF_SYMLINK_REFUSED" && i.path == link));
}

#[test]
fn invalid_file_reports_errors_and_does_not_stop_others() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    t.file("agents/bad.toml", "schema_version = 1\n[agent]\nid = 3\n");
    t.file("agents/good.toml", MINIMAL);
    let loaded = load_dir(&dir, Origin::new(Source::User), None, &[]);
    assert_eq!(loaded.files.len(), 2);
    let bad = loaded.files.iter().find(|f| f.path.ends_with("bad.toml")).unwrap();
    let good = loaded.files.iter().find(|f| f.path.ends_with("good.toml")).unwrap();
    assert!(!bad.is_valid() && !bad.errors.is_empty());
    assert!(good.is_valid());
    assert!(good.fingerprint.as_deref().unwrap().starts_with("sha256:"));
}

#[test]
fn code_id_is_reserved_not_shadowed() {
    let file = load_one(&with_id("claude"), Origin::new(Source::User), "u/claude.toml");
    assert!(file.errors.iter().any(|e| e.code == "AGENT_DEF_RESERVED_ID"));
    assert!(!file.is_valid());
    let resolved = resolve(vec![file]);
    assert!(resolved.active.is_empty() && resolved.shadowed.is_empty());
    assert_eq!(resolved.invalid.len(), 1);
}

#[test]
fn shadowing_follows_reg8_and_reports_losers() {
    let text = with_id("dup");
    let bundled = load_one(&text, Origin::new(Source::Bundled), "b/dup.toml");
    let user = load_one(&text, Origin::new(Source::User), "u/dup.toml");
    let extra0 = load_one(&text, Origin::extra(0), "e0/dup.toml");
    let extra1 = load_one(&text, Origin::extra(1), "e1/dup.toml");
    let project = load_one(&text, Origin::new(Source::Project), "p/dup.toml");

    let all = resolve(vec![bundled.clone(), user.clone(), extra1.clone(), project.clone(), extra0.clone()]);
    assert_eq!(all.active.len(), 1);
    assert_eq!(all.active[0].path, PathBuf::from("p/dup.toml"));
    assert_eq!(all.shadowed.len(), 4);
    assert!(all.shadowed.iter().all(|s| s.shadowed_by == PathBuf::from("p/dup.toml")));

    // without the project file (a run in another project, SRC-7) extra beats user beats bundled
    let none = resolve(vec![bundled.clone(), user.clone(), extra1, extra0]);
    assert_eq!(none.active[0].path, PathBuf::from("e0/dup.toml"));
    let low = resolve(vec![bundled, user]);
    assert_eq!(low.active[0].path, PathBuf::from("u/dup.toml"));
    assert_eq!(low.shadowed.len(), 1);
}

#[test]
fn invalid_definition_never_shadows() {
    let good = load_one(&with_id("dup"), Origin::new(Source::User), "u/dup.toml");
    let broken = load_one(&with_id("dup").replace("names = [\"kilo\", \"kilocode\"]", "names = []"), Origin::new(Source::Project), "p/dup.toml");
    assert!(!broken.is_valid());
    let resolved = resolve(vec![good, broken]);
    assert_eq!(resolved.active[0].path, PathBuf::from("u/dup.toml"));
    assert!(resolved.shadowed.is_empty());
    assert_eq!(resolved.invalid.len(), 1);
}

#[test]
fn project_dir_is_under_dot_puppet_master() {
    assert!(project_agents_dir(Path::new("proj")).ends_with(Path::new(".puppet-master").join("agents")));
}

mod fingerprint_cases {
    use super::*;

    fn fp(text: &str) -> String {
        let def = parse_definition(text).unwrap();
        fingerprint(&def, Source::User, None, Path::new("C:/agents/kilo.toml"))
    }

    #[test]
    fn stable_under_whitespace() {
        let noisy = KILO.replace("\n[", "\n\n# a comment\n[").replace(" = ", "   =   ");
        assert_eq!(fp(KILO), fp(&noisy));
    }

    #[test]
    fn stable_under_non_executable_edits() {
        let edited = KILO
            .replace("label = \"Kilo Code\"", "label = \"Renamed\"")
            .replace("icon = \"KC\"", "icon = \"ZZ\"")
            .replace("pick = \"last\"", "pick = \"first\"")
            .replace("smartness = 6", "smartness = 9");
        assert_ne!(edited, KILO);
        assert_eq!(fp(KILO), fp(&edited));
    }

    #[test]
    fn changes_with_argv() {
        assert_ne!(fp(KILO), fp(&KILO.replace("argv = [\"run\", \"--format\", \"json\"]", "argv = [\"run\", \"--format\", \"json\", \"--x\"]")));
        assert_ne!(fp(KILO), fp(&KILO.replace("args = [\"--auto\"]", "args = [\"--yolo\"]")));
        assert_ne!(fp(KILO), fp(&KILO.replace("task_separator = \"--\"", "task_flag = \"-p\"")));
    }

    #[test]
    fn changes_with_binary_and_env() {
        assert_ne!(fp(KILO), fp(&KILO.replace("names = [\"kilo\", \"kilocode\"]", "names = [\"kilo\"]")));
        assert_ne!(fp(KILO), fp(&KILO.replace("[features]", "[env]\nset = { A = \"1\" }\n\n[features]")));
        assert_ne!(fp(KILO), fp(&KILO.replace("[run_options]", "[agent_extra]\n[run_options]").replace("[agent_extra]\n", "")
            .replace("shim_native_globs = [", "allow_cmd_shim = true\nshim_native_globs = [")));
    }

    #[test]
    fn changes_with_source_project_and_path() {
        let def = parse_definition(KILO).unwrap();
        let base = fingerprint(&def, Source::User, None, Path::new("C:/agents/kilo.toml"));
        assert_ne!(base, fingerprint(&def, Source::Project, None, Path::new("C:/agents/kilo.toml")));
        assert_ne!(base, fingerprint(&def, Source::User, Some("C:/p"), Path::new("C:/agents/kilo.toml")));
        assert_ne!(base, fingerprint(&def, Source::User, None, Path::new("C:/other/kilo.toml")));
        assert_eq!(
            fingerprint(&def, Source::User, Some("C:\\p\\"), Path::new("C:/agents/kilo.toml")),
            fingerprint(&def, Source::User, Some("c:/p"), Path::new("C:/agents/kilo.toml")),
        );
    }
}
