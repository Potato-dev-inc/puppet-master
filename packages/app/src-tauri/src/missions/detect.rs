//! The start sheet's check list: what this project already runs (DESIGN-MISSIONS 2.2). One small
//! detector until the ship check (F2) lands; then this file goes and the sheet reads the project's
//! checks.

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detected {
    pub name: String,
    pub command: Vec<String>,
    /// The file it was found in ("package.json").
    pub source: String,
    /// Switched on in the sheet by default.
    pub on: bool,
}

impl Detected {
    fn new(name: &str, command: &[&str], source: &str, on: bool) -> Detected {
        Detected { name: name.into(), command: command.iter().map(|s| s.to_string()).collect(), source: source.into(), on }
    }
}

/// npm's placeholder test script ("no test specified") is not a check.
fn real_script(script: &str) -> bool {
    !script.trim().is_empty() && !script.contains("no test specified")
}

pub fn detect(project: &Path) -> Vec<Detected> {
    let mut found = Vec::new();
    if let Some(pkg) = std::fs::read_to_string(project.join("package.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        let script = |name: &str| pkg.get("scripts").and_then(|s| s.get(name)).and_then(|v| v.as_str()).filter(|s| real_script(s)).is_some();
        if script("test") {
            found.push(Detected::new("Tests", &["npm", "test"], "package.json", true));
        }
        if script("typecheck") {
            found.push(Detected::new("Type check", &["npm", "run", "typecheck"], "package.json", true));
        } else if project.join("tsconfig.json").is_file() {
            found.push(Detected::new("Type check", &["npx", "tsc", "--noEmit"], "tsconfig.json", true));
        }
        if script("lint") {
            found.push(Detected::new("Lint", &["npm", "run", "lint"], "package.json", false));
        }
    } else if project.join("tsconfig.json").is_file() {
        found.push(Detected::new("Type check", &["npx", "tsc", "--noEmit"], "tsconfig.json", true));
    }
    if project.join("Cargo.toml").is_file() {
        found.push(Detected::new("Tests", &["cargo", "test"], "Cargo.toml", true));
    }
    if project.join("pyproject.toml").is_file() || project.join("pytest.ini").is_file() {
        let source = if project.join("pytest.ini").is_file() { "pytest.ini" } else { "pyproject.toml" };
        found.push(Detected::new("Tests", &["python", "-m", "pytest"], source, true));
    }
    if project.join("go.mod").is_file() {
        found.push(Detected::new("Tests", &["go", "test", "./..."], "go.mod", true));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_checks_a_node_project_runs() {
        let dir = std::env::temp_dir().join(format!("pm-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"test":"node --test","lint":"eslint ."}}"#).unwrap();
        std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
        let found = detect(&dir);
        let names: Vec<(&str, bool)> = found.iter().map(|d| (d.name.as_str(), d.on)).collect();
        assert_eq!(names, vec![("Tests", true), ("Type check", true), ("Lint", false)]);
        assert_eq!(found[1].command, vec!["npx", "tsc", "--noEmit"]);
    }

    #[test]
    fn the_npm_placeholder_is_not_a_check() {
        let dir = std::env::temp_dir().join(format!("pm-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#).unwrap();
        assert!(detect(&dir).is_empty());
    }
}
