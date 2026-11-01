//! Which languages the person's projects use, found from marker files only.

use std::path::{Path, PathBuf};

use super::detect::*;
use super::fakes_tests::Scratch;

fn touch(root: &Path, rel: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "x").unwrap();
}

fn project(scratch: &Scratch, name: &str, files: &[&str]) -> PathBuf {
    let dir = scratch.0.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for file in files {
        touch(&dir, file);
    }
    dir
}

fn langs(d: &Detected) -> Vec<&str> {
    d.languages.iter().map(String::as_str).collect()
}

#[test]
fn each_marker_file_names_its_language() {
    let s = Scratch::new("detect-markers");
    let all = [
        ("py", "pyproject.toml", "python"),
        ("py2", "requirements.txt", "python"),
        ("go", "go.mod", "go"),
        ("rs", "Cargo.toml", "rust"),
        ("mvn", "pom.xml", "java"),
        ("gradle", "build.gradle.kts", "java"),
        ("net", "App.csproj", "dotnet"),
    ];
    for (name, file, language) in all {
        let found = languages_in(&[project(&s, name, &[file])]);
        assert_eq!(langs(&found), vec![language], "{file}");
        assert_eq!(found.projects, 1);
    }
}

#[test]
fn a_package_json_alone_picks_no_extra_language_node_is_always_there() {
    let s = Scratch::new("detect-node");
    let found = languages_in(&[project(&s, "web", &["package.json", "package-lock.json"])]);
    assert!(found.languages.is_empty());
    assert_eq!(found.package_manager.as_deref(), Some("npm"));
}

#[test]
fn the_answer_keeps_the_recipes_order_whatever_the_order_of_the_projects() {
    let s = Scratch::new("detect-order");
    let a = project(&s, "a", &["Cargo.toml"]);
    let b = project(&s, "b", &["go.mod", "pyproject.toml"]);
    let found = languages_in(&[a, b]);
    assert_eq!(langs(&found), vec!["python", "go", "rust"]);
    assert_eq!(found.projects, 2);
}

#[test]
fn markers_in_package_folders_of_a_monorepo_count_but_deep_or_vendored_ones_do_not() {
    let s = Scratch::new("detect-mono");
    let mono = project(&s, "mono", &["packages/app/src-tauri/Cargo.toml", "services/api/go.mod"]);
    assert_eq!(langs(&languages_in(&[mono])), vec!["go", "rust"]);
    let vendored = project(&s, "vend", &["node_modules/x/pyproject.toml", "target/y/Cargo.toml", ".git/z/go.mod", "a/b/c/d/pom.xml"]);
    assert!(languages_in(&[vendored]).languages.is_empty());
}

#[test]
fn the_package_manager_is_the_one_most_projects_use_and_a_tie_goes_to_the_first() {
    let s = Scratch::new("detect-pm");
    let p1 = project(&s, "p1", &["pnpm-lock.yaml"]);
    let p2 = project(&s, "p2", &["pnpm-lock.yaml", "package-lock.json"]);
    let p3 = project(&s, "p3", &["yarn.lock"]);
    assert_eq!(languages_in(&[p1.clone(), p2, p3.clone()]).package_manager.as_deref(), Some("pnpm"));
    assert_eq!(languages_in(&[p1, p3]).package_manager.as_deref(), Some("pnpm"), "a tie: pnpm comes first");
    let bun = project(&s, "bun", &["bun.lockb"]);
    assert_eq!(languages_in(&[bun]).package_manager.as_deref(), Some("bun"));
    let none = project(&s, "none", &["README.md"]);
    assert_eq!(languages_in(&[none]).package_manager, None);
}

#[test]
fn a_missing_folder_counts_as_nothing_and_a_file_is_not_a_project() {
    let s = Scratch::new("detect-missing");
    let file = s.0.join("a-file.txt");
    std::fs::write(&file, "x").unwrap();
    let found = languages_in(&[s.0.join("nope"), file]);
    assert_eq!(found, Detected::default());
}

#[test]
fn nothing_is_written_or_opened() {
    // The scan only lists names: a marker that is a folder (not a file) is no marker.
    let s = Scratch::new("detect-dir-marker");
    let dir = s.0.join("p");
    std::fs::create_dir_all(dir.join("go.mod")).unwrap();
    assert!(languages_in(&[dir]).languages.is_empty());
}
