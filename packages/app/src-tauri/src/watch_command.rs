//! Shell command line for background operation watching (pm-watch / `puppet-master watch`).
//!
//! The command must work from any directory, so it names the CLI by absolute path instead of
//! relying on `npx puppet-master`, which only resolves inside the repo.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at app startup so packaged builds can find the bundled CLI.
pub fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

const BUNDLE_NAME: &str = "puppet-master-cli.bundle.mjs";

/// Candidate locations for a standalone watcher script, most specific first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(explicit) = std::env::var_os("PUPPET_MASTER_CLI") {
        out.push(PathBuf::from(explicit));
    }
    if let Some(dir) = RESOURCE_DIR.get() {
        out.push(dir.join(BUNDLE_NAME));
    }
    if let Ok(exe) = std::env::current_exe() {
        // resources/bin/<exe> (staged MCP binary) and dist/<exe> (dev copies).
        for ancestor in exe.ancestors().skip(1).take(4) {
            out.push(ancestor.join(BUNDLE_NAME));
            out.push(ancestor.join("resources").join(BUNDLE_NAME));
        }
    }
    // Dev checkout: the workspace CLI build next to this crate.
    out.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("cli")
            .join("dist")
            .join("index.js"),
    );
    out
}

fn find_cli_entry() -> Option<PathBuf> {
    candidates().into_iter().find(|path| path.is_file())
}

pub fn format_watch_command(operation_id: &str, project_path: Option<&str>) -> String {
    let entry = find_cli_entry().map(|found| {
        stable_copy(&found, &crate::app_paths::app_data_dir().join("cli")).unwrap_or(found)
    });
    format_with_entry(entry.as_deref(), operation_id, project_path)
}

/// Copy the bundled watcher into a per-user directory so the command does not point into a build
/// output folder (`target/debug/...`) that `cargo clean`, a rebuild or another machine removes.
/// Only the self-contained bundle is copied; the workspace `dist/index.js` needs its siblings.
fn stable_copy(source: &Path, dest_dir: &Path) -> Option<PathBuf> {
    if source.file_name()? != BUNDLE_NAME || source.starts_with(dest_dir) {
        return None;
    }
    let dest = dest_dir.join(BUNDLE_NAME);
    let same = match (std::fs::metadata(source), std::fs::metadata(&dest)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && b.modified().ok() >= a.modified().ok(),
        _ => false,
    };
    if !same {
        std::fs::create_dir_all(dest_dir).ok()?;
        let tmp = dest_dir.join(format!("{BUNDLE_NAME}.tmp"));
        std::fs::copy(source, &tmp).ok()?;
        std::fs::rename(&tmp, &dest).ok()?;
    }
    Some(dest)
}

fn format_with_entry(entry: Option<&Path>, operation_id: &str, project_path: Option<&str>) -> String {
    let id = shell_quote(operation_id);
    let mut cmd = match entry {
        Some(path) => {
            // Forward slashes work on every platform and avoid backslash-escaping differences
            // between PowerShell, cmd and bash.
            let script = path
                .canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .replace('\\', "/");
            format!("node {} watch {id}", shell_quote(&script))
        }
        None => format!("npx puppet-master watch {id}"),
    };
    if let Some(path) = project_path.filter(|p| !p.trim().is_empty()) {
        cmd.push_str(&format!(" --project-path {}", shell_quote(path)));
    }
    cmd
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".into();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '\\' | ':'))
        && !value.contains(' ')
    {
        return value.to_string();
    }
    format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_command_includes_project_when_set() {
        let cmd = format_with_entry(None, "op-1", Some(r"C:\Users\me\Desktop\pm-trial"));
        assert!(cmd.contains("npx puppet-master watch"));
        assert!(cmd.contains("op-1"));
        assert!(cmd.contains("--project-path"));
        assert!(cmd.contains("pm-trial"));
    }

    #[test]
    fn watch_command_uses_an_absolute_node_path_when_the_cli_is_found() {
        let dir = std::env::temp_dir().join(format!("pm-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("index.js");
        std::fs::write(&entry, "").unwrap();
        let cmd = format_with_entry(Some(&entry), "op-1", None);
        assert!(cmd.starts_with("node "), "{cmd}");
        assert!(!cmd.contains("npx"), "{cmd}");
        assert!(cmd.contains("index.js watch op-1"), "{cmd}");
        assert!(!cmd.contains('\\') || cmd.contains('"'), "{cmd}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dev_checkout_resolves_the_workspace_cli_build() {
        let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cli/dist/index.js");
        if cli.is_file() {
            assert!(find_cli_entry().is_some());
        }
    }

    #[test]
    fn bundle_is_copied_to_a_stable_dir_even_when_paths_contain_spaces() {
        let base = std::env::temp_dir().join(format!("pm watch stable {}", std::process::id()));
        let build = base.join("target").join("debug").join("resources");
        let stable = base.join("app data").join("cli");
        std::fs::create_dir_all(&build).unwrap();
        let source = build.join(BUNDLE_NAME);
        std::fs::write(&source, "console.log('v1')").unwrap();

        let copied = stable_copy(&source, &stable).expect("copied");
        assert!(copied.starts_with(&stable));
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "console.log('v1')");

        // Survives `cargo clean`: the command keeps working after the build output is gone.
        std::fs::remove_dir_all(base.join("target")).unwrap();
        assert!(copied.is_file());

        // Quoted correctly for a path with spaces, from any cwd.
        let cmd = format_with_entry(Some(&copied), "op-1", Some("C:/my project"));
        assert!(cmd.starts_with("node \""), "{cmd}");
        assert!(cmd.contains("app data/cli/"), "{cmd}");
        assert!(cmd.contains("--project-path \"C:/my project\""), "{cmd}");

        // A rebuilt bundle replaces the stale copy; the workspace dist/index.js is never copied.
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(&source, "console.log('v2 longer')").unwrap();
        let again = stable_copy(&source, &stable).unwrap();
        assert_eq!(std::fs::read_to_string(again).unwrap(), "console.log('v2 longer')");
        assert!(stable_copy(&base.join("index.js"), &stable).is_none());
        let _ = std::fs::remove_dir_all(base);
    }
}
