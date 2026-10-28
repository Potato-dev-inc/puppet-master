//! What a path is for, judged from its name alone: a test file, a snapshot, or check config.
//!
//! This is the public face of the rule tables (`Rules::builtin()`) for callers that only have a
//! path, such as the Review Desk. It reads nothing from disk: no file text, no `test_markers`
//! (those need the file's content). Language names never appear here; every glob lives in the
//! tables.

use std::path::Path;

use super::model::Role;
use super::rules::Rules;

/// The role of a file by its path, or `None` when it is none of the three.
///
/// Order: a snapshot glob wins, then any ecosystem's test glob, then any ecosystem's config glob.
/// A path with a folder named in `never_dirs` (dependencies, build output, `.git`) is never a
/// role. Backslashes count as separators, so Windows paths work.
pub fn path_role(path: &str) -> Option<Role> {
    let rules = Rules::builtin();
    let path = path.replace('\\', "/");
    if in_never_dir(rules, &path) {
        return None;
    }
    if rules.snapshot_globs.is_match(&path) {
        return Some(Role::Snapshot);
    }
    if rules.ecosystems.iter().any(|e| e.test_globs.is_match(&path)) {
        return Some(Role::Test);
    }
    if rules.ecosystems.iter().any(|e| e.config.iter().any(|c| c.globs.is_match(&path))) {
        return Some(Role::Config);
    }
    None
}

/// True when a folder above the file is one of the never-protected folders.
fn in_never_dir(rules: &Rules, path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let folders = &parts[..parts.len().saturating_sub(1)];
    folders.iter().any(|folder| rules.never_dirs.iter().any(|never| never.as_str() == *folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_by_ecosystem() {
        assert_eq!(path_role("src/cart.test.ts"), Some(Role::Test));
        assert_eq!(path_role("tests/test_cart.py"), Some(Role::Test));
        assert_eq!(path_role("cart_test.go"), Some(Role::Test));
    }

    #[test]
    fn snapshot_files() {
        assert_eq!(path_role("__snapshots__/a.snap"), Some(Role::Snapshot));
    }

    #[test]
    fn check_config_files() {
        assert_eq!(path_role("jest.config.js"), Some(Role::Config));
        assert_eq!(path_role("pytest.ini"), Some(Role::Config));
    }

    #[test]
    fn ordinary_source_and_docs_have_no_role() {
        // Rust unit tests live in `src` and need file text (`test_markers`) to be seen, so the path alone says no.
        assert_eq!(path_role("src/lib.rs"), None);
        assert_eq!(path_role("docs/readme.md"), None);
    }

    #[test]
    fn never_dirs_have_no_role() {
        assert_eq!(path_role("node_modules/x/a.test.js"), None);
        assert_eq!(path_role("packages/app/node_modules/x/a.test.js"), None);
        assert_eq!(path_role("target/debug/tests/a.rs"), None);
    }

    #[test]
    fn windows_separators_are_normalised() {
        assert_eq!(path_role("src\\cart.test.ts"), Some(Role::Test));
        assert_eq!(path_role("tests\\test_cart.py"), Some(Role::Test));
        assert_eq!(path_role("node_modules\\x\\a.test.js"), None);
    }
}
