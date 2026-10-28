//! Collision Alert: which changed files never count, and safe relative paths.

use crate::push::triggers::sanitize;

use super::model::MAX_PATH_CHARS;

/// Longest relative path kept (bytes). Longer ones are dropped, not cut.
const MAX_REL_BYTES: usize = 400;

/// Folders that never count, at any depth (matched case-insensitively).
const NOISE_FOLDERS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".puppet-master",
    ".git",
    "__pycache__",
    ".venv",
    "coverage",
];

/// File names that never count, wherever they are (matched case-insensitively).
const NOISE_NAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "poetry.lock",
    "composer.lock",
    "Gemfile.lock",
    "go.sum",
    ".DS_Store",
    "Thumbs.db",
];

/// File name endings that never count (lower case).
const NOISE_SUFFIXES: &[&str] = &[".min.js", ".min.css", ".map", ".log", ".tmp", ".swp"];

/// A repo-relative path from git or the file system, if it is safe to show and compare.
///
/// The path goes through `turn_summary::blame::normalise` (no absolute path, no drive, no `..`).
/// It is dropped when it holds any control character or a hidden / bidirectional mark, or when it
/// is longer than 400 bytes.
pub fn clean_rel(raw: &str) -> Option<String> {
    if raw.chars().any(|c| c.is_control()) || sanitize(raw) != raw {
        return None;
    }
    let rel = crate::turn_summary::blame::normalise(raw)?;
    (rel.len() <= MAX_REL_BYTES).then_some(rel)
}

/// True for a file that is never worth an alert: a folder of build or tool output, a lock file, a
/// log, a map, a minified bundle, an editor swap file. Matched on the whole relative path.
pub fn is_noise(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    if folders.iter().any(|f| any_eq(NOISE_FOLDERS, f)) {
        return true;
    }
    if any_eq(NOISE_NAMES, name) {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    NOISE_SUFFIXES.iter().any(|suffix| lower.ends_with(*suffix))
}

/// A path for a card or a log line: control and hidden marks removed, one line, cut to
/// `MAX_PATH_CHARS` characters with `...` at the end when cut.
pub fn display(rel: &str) -> String {
    let flat: String = sanitize(rel)
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_PATH_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(MAX_PATH_CHARS - 3).collect();
    cut.push_str("...");
    cut
}

fn any_eq(list: &[&str], text: &str) -> bool {
    list.iter().any(|item| item.eq_ignore_ascii_case(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_rel_normalises_slashes_and_dots() {
        assert_eq!(clean_rel("src\\auth.ts").as_deref(), Some("src/auth.ts"));
        assert_eq!(clean_rel("./src//auth.ts").as_deref(), Some("src/auth.ts"));
        assert_eq!(clean_rel("  src/a.ts  ").as_deref(), Some("src/a.ts"));
    }

    #[test]
    fn clean_rel_refuses_escapes_absolute_and_drive_paths() {
        assert_eq!(clean_rel("../secret.txt"), None);
        assert_eq!(clean_rel("src/../../x"), None);
        assert_eq!(clean_rel("/etc/passwd"), None);
        assert_eq!(clean_rel("\\\\server\\share\\x"), None);
        assert_eq!(clean_rel("C:/Users/you/x.txt"), None);
        assert_eq!(clean_rel("C:\\Users\\you\\x.txt"), None);
        assert_eq!(clean_rel(""), None);
        assert_eq!(clean_rel("."), None);
    }

    #[test]
    fn clean_rel_drops_control_and_bidi_names() {
        assert_eq!(clean_rel("a\nb"), None);
        assert_eq!(clean_rel("a\rb"), None);
        assert_eq!(clean_rel("a\tb.txt"), None);
        assert_eq!(clean_rel("evil\u{202e}txt"), None);
        assert_eq!(clean_rel("zero\u{200b}width.txt"), None);
        assert_eq!(clean_rel("bom\u{feff}.txt"), None);
        assert_eq!(clean_rel("del\u{7f}.txt"), None);
    }

    #[test]
    fn clean_rel_keeps_plain_unicode_names() {
        assert_eq!(clean_rel("docs/café.md").as_deref(), Some("docs/café.md"));
    }

    #[test]
    fn clean_rel_length_limit_is_400_bytes() {
        let exact = format!("{}.txt", "a".repeat(400 - 4));
        assert_eq!(exact.len(), 400);
        assert_eq!(clean_rel(&exact).as_deref(), Some(exact.as_str()));
        let over = format!("{}.txt", "a".repeat(400 - 3));
        assert_eq!(clean_rel(&over), None);
    }

    #[test]
    fn noise_folders_count_at_any_depth() {
        assert!(is_noise("node_modules/left-pad/index.js"));
        assert!(is_noise("packages/app/node_modules/x.js"));
        assert!(is_noise("target/debug/app.exe"));
        assert!(is_noise("crates/a/target/x"));
        assert!(is_noise("dist/bundle.js"));
        assert!(is_noise("build/out.txt"));
        assert!(is_noise(".next/cache/x"));
        assert!(is_noise(".puppet-master/worktrees/x/src/a.rs"));
        assert!(is_noise(".git/config"));
        assert!(is_noise("pkg/__pycache__/m.pyc"));
        assert!(is_noise(".venv/lib/x.py"));
        assert!(is_noise("coverage/index.html"));
    }

    #[test]
    fn noise_lock_files_and_names() {
        assert!(is_noise("package-lock.json"));
        assert!(is_noise("app/yarn.lock"));
        assert!(is_noise("pnpm-lock.yaml"));
        assert!(is_noise("src-tauri/Cargo.lock"));
        assert!(is_noise("poetry.lock"));
        assert!(is_noise("composer.lock"));
        assert!(is_noise("Gemfile.lock"));
        assert!(is_noise("go.sum"));
        assert!(is_noise("photos/.DS_Store"));
        assert!(is_noise("Thumbs.db"));
    }

    #[test]
    fn noise_suffixes() {
        assert!(is_noise("public/app.min.js"));
        assert!(is_noise("public/app.min.css"));
        assert!(is_noise("public/app.js.map"));
        assert!(is_noise("run.log"));
        assert!(is_noise("scratch.tmp"));
        assert!(is_noise("src/.auth.ts.swp"));
    }

    #[test]
    fn noise_is_case_insensitive() {
        assert!(is_noise("PACKAGE-LOCK.JSON"));
        assert!(is_noise("Target/x.rs"));
        assert!(is_noise("App.MIN.JS"));
        assert!(is_noise("NODE_MODULES/x"));
    }

    #[test]
    fn ordinary_files_are_not_noise() {
        assert!(!is_noise("src/auth.ts"));
        assert!(!is_noise("src/targets.rs"));
        assert!(!is_noise("dist.ts"));
        assert!(!is_noise("src/build.rs"));
        assert!(!is_noise("docs/logging.md"));
        assert!(!is_noise("src/map.ts"));
        assert!(!is_noise("package.json"));
        assert!(!is_noise(""));
    }

    #[test]
    fn display_keeps_short_paths() {
        assert_eq!(display("src/auth.ts"), "src/auth.ts");
    }

    #[test]
    fn display_cuts_to_120_with_dots() {
        let long = format!("{}.ts", "d/".repeat(100));
        let shown = display(&long);
        assert_eq!(shown.chars().count(), MAX_PATH_CHARS);
        assert!(shown.ends_with("..."));
        assert!(shown.starts_with("d/d/"));
        let exact = "x".repeat(MAX_PATH_CHARS);
        assert_eq!(display(&exact), exact);
    }

    #[test]
    fn display_removes_hidden_marks_and_line_breaks() {
        assert_eq!(display("evil\u{202e}txt"), "eviltxt");
        assert_eq!(display("a\nb"), "a b");
    }
}
