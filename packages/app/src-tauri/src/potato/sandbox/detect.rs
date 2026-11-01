//! Which programming languages the person's projects use, read from marker files. The wizard pre-checks these so the
//! person only confirms. Read only: this looks at names in a few folders and opens no file.
//!
//! A marker is a file name in a project folder, up to three levels down (a monorepo keeps `Cargo.toml` or `go.mod` in a
//! package folder). Folders that hold other people's code or build output (`node_modules`, `target`, `.git` ...) are skipped.
//! The language ids are the ones of the `coding` recipe's `languages` option; JavaScript needs no pick (Node.js is always
//! part of the coding tools), so it only decides which package manager is suggested.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// (recipe language id, marker file names, marker file ending). Order is the order of the answer.
const MARKERS: [(&str, &[&str], &str); 5] = [
    ("python", &["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"], ""),
    ("go", &["go.mod"], ""),
    ("rust", &["Cargo.toml"], ""),
    ("java", &["pom.xml", "build.gradle", "build.gradle.kts"], ""),
    ("dotnet", &[], ".csproj"),
];

/// Lock files that say which package manager a JavaScript project uses.
const LOCKS: [(&str, &str); 5] = [
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lockb", "bun"),
    ("bun.lock", "bun"),
    ("package-lock.json", "npm"),
];

const SKIP: [&str; 9] = ["node_modules", "target", "dist", "build", "vendor", "venv", "bin", "obj", "out"];
/// How many folders are listed per project, and how deep (the project folder is depth 0).
const MAX_FOLDERS: usize = 120;
const MAX_DEPTH: usize = 3;

/// What the projects said.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// Language ids of the `coding` recipe, in a fixed order.
    pub languages: Vec<String>,
    /// The package manager most projects use (`npm`, `pnpm`, `yarn`, `bun`), when any project has a lock file.
    pub package_manager: Option<String>,
    /// How many project folders were looked at.
    pub projects: usize,
}

#[derive(Default)]
struct Found {
    languages: Vec<&'static str>,
    managers: Vec<&'static str>,
}

fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIP.iter().any(|s| name.eq_ignore_ascii_case(s))
}

/// Lists the project folder and its subfolders level by level (so the shallow folders are always seen before the budget runs out).
fn look(project: &Path, found: &mut Found) {
    let mut level: Vec<PathBuf> = vec![project.to_path_buf()];
    let mut budget = MAX_FOLDERS;
    for depth in 0..=MAX_DEPTH {
        let mut next: Vec<PathBuf> = Vec::new();
        for dir in &level {
            if budget == 0 {
                return;
            }
            budget -= 1;
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() {
                    if depth < MAX_DEPTH && !skipped(&name) {
                        next.push(entry.path());
                    }
                    continue;
                }
                for (id, names, ending) in MARKERS {
                    let hit = names.iter().any(|m| name.eq_ignore_ascii_case(m)) || (!ending.is_empty() && name.to_lowercase().ends_with(ending));
                    if hit && !found.languages.contains(&id) {
                        found.languages.push(id);
                    }
                }
                if depth == 0 {
                    if let Some((_, manager)) = LOCKS.iter().find(|(lock, _)| name.eq_ignore_ascii_case(lock)) {
                        found.managers.push(manager);
                    }
                }
            }
        }
        next.sort();
        level = next;
    }
}

/// Looks at each project folder. A folder that is missing or unreadable counts as nothing.
pub fn languages_in(projects: &[PathBuf]) -> Detected {
    let mut languages: Vec<&'static str> = Vec::new();
    let mut votes: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut looked = 0;
    for project in projects {
        if !project.is_dir() {
            continue;
        }
        looked += 1;
        let mut found = Found::default();
        look(project, &mut found);
        for id in found.languages {
            if !languages.contains(&id) {
                languages.push(id);
            }
        }
        // One project, one vote (its first lock file in the LOCKS order).
        if let Some(first) = LOCKS.iter().map(|(_, m)| *m).find(|m| found.managers.contains(m)) {
            *votes.entry(first).or_default() += 1;
        }
    }
    // The answer keeps the recipe's order, not the order the projects were seen in.
    let ordered: Vec<String> = MARKERS.iter().map(|(id, _, _)| *id).filter(|id| languages.contains(id)).map(str::to_string).collect();
    // The most votes win; a tie goes to the manager that comes first in the LOCKS order.
    let package_manager = LOCKS
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| votes.contains_key(m))
        .max_by_key(|m| (votes[m], std::cmp::Reverse(LOCKS.iter().position(|(_, x)| x == m))))
        .map(str::to_string);
    Detected { languages: ordered, package_manager, projects: looked }
}
