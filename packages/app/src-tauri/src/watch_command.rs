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

/// Target shell for the printed command. `Default` is a form that is valid in both PowerShell and
/// Git Bash: forward-slash paths, single-quoted when quoting is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellHint {
    #[default]
    Default,
    Bash,
    PowerShell,
    Cmd,
}

impl ShellHint {
    /// Parses `bash` (also `sh`, `zsh`, `git-bash`), `powershell` (also `pwsh`), `cmd`, or
    /// `default`; anything else yields `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bash" | "sh" | "zsh" | "git-bash" | "gitbash" => Some(Self::Bash),
            "powershell" | "pwsh" | "ps" => Some(Self::PowerShell),
            "cmd" | "cmd.exe" => Some(Self::Cmd),
            "default" | "auto" => Some(Self::Default),
            _ => None,
        }
    }

    /// Hint from the `PUPPET_MASTER_WATCH_SHELL` environment variable, when set and valid.
    pub fn from_env() -> Self {
        std::env::var("PUPPET_MASTER_WATCH_SHELL")
            .ok()
            .and_then(|value| Self::parse(&value))
            .unwrap_or_default()
    }
}

pub fn format_watch_command(operation_id: &str, project_path: Option<&str>) -> String {
    format_watch_command_for(operation_id, project_path, ShellHint::from_env())
}

pub fn format_watch_command_for(
    operation_id: &str,
    project_path: Option<&str>,
    shell: ShellHint,
) -> String {
    let entry = find_cli_entry().map(|found| {
        stable_copy(&found, &crate::app_paths::app_data_dir().join("cli")).unwrap_or(found)
    });
    format_with_entry(entry.as_deref(), operation_id, project_path, shell)
}

/// Re-renders an already formatted watch command for `shell`, keeping its project path (read back
/// from the existing `--project-path`, else `project_fallback`). Used where a caller-supplied
/// `shell` hint is known but the command was produced by the app.
pub fn reformat_for_shell(
    existing: &str,
    operation_id: &str,
    project_fallback: Option<&str>,
    shell: ShellHint,
) -> String {
    let project = existing_project_path(existing);
    let project = project.as_deref().or(project_fallback);
    // The MCP shim runs outside the app, so its own CLI lookup finds only the workspace
    // `cli/dist/index.js` (stale or absent). The app already chose (and stabilised) the entry:
    // keep it and only change the quoting.
    match existing_entry(existing) {
        Some(entry) => format_with_entry(Some(&entry), operation_id, project, shell),
        None => format_watch_command_for(operation_id, project, shell),
    }
}

/// The script path of an already formatted `node <script> watch ...` command.
fn existing_entry(existing: &str) -> Option<PathBuf> {
    let rest = existing.trim_start().strip_prefix("node ")?.trim_start();
    let (script, _) = match rest.chars().next()? {
        quote @ ('\'' | '"') => {
            let inner = &rest[1..];
            let end = inner.find(quote)?;
            (inner[..end].to_string(), &inner[end + 1..])
        }
        _ => {
            let end = rest.find(" watch ")?;
            (rest[..end].to_string(), &rest[end..])
        }
    };
    (!script.is_empty()).then(|| PathBuf::from(script))
}

fn existing_project_path(existing: &str) -> Option<String> {
    const MARKER: &str = " --project-path ";
    existing.find(MARKER).map(|at| {
        let raw = existing[at + MARKER.len()..].trim();
        let unquoted = match (raw.chars().next(), raw.chars().last()) {
            (Some('\''), Some('\'')) | (Some('"'), Some('"')) if raw.len() >= 2 => {
                &raw[1..raw.len() - 1]
            }
            _ => raw,
        };
        unquoted.replace("'\\''", "'").replace("''", "'")
    })
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

fn format_with_entry(
    entry: Option<&Path>,
    operation_id: &str,
    project_path: Option<&str>,
    shell: ShellHint,
) -> String {
    let id = shell_quote(operation_id, shell);
    let mut cmd = match entry {
        Some(path) => {
            let script = path
                .canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string();
            format!("node {} watch {id}", shell_quote(&portable_path(&script), shell))
        }
        None => format!("npx puppet-master watch {id}"),
    };
    if let Some(path) = project_path.filter(|p| !p.trim().is_empty()) {
        cmd.push_str(&format!(
            " --project-path {}",
            shell_quote(&portable_path(path), shell)
        ));
    }
    cmd
}

/// Windows paths (`C:\x`, `\\server\share`) use forward slashes: node, PowerShell, cmd and bash
/// all accept them, and bash would otherwise eat the backslashes. Other paths are left alone
/// since a backslash is a legal filename character on POSIX.
fn portable_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || path.starts_with(r"\\") {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

fn shell_quote(value: &str, shell: ShellHint) -> String {
    let plain = !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '@' | '+' | '=')
        });
    if plain {
        return value.to_string();
    }
    match shell {
        ShellHint::Bash => format!("'{}'", value.replace('\'', r"'\''")),
        ShellHint::PowerShell => format!("'{}'", value.replace('\'', "''")),
        ShellHint::Cmd => format!("\"{}\"", value.replace('"', "\"\"")),
        // Single quotes are literal in both PowerShell and Git Bash. A value that itself holds a
        // single quote cannot be written the same way in both, so double-quote it (correct
        // unless it also contains `$` or a backtick).
        ShellHint::Default if value.contains('\'') => {
            format!("\"{}\"", value.replace('"', "\\\""))
        }
        ShellHint::Default => format!("'{value}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_command_includes_project_when_set() {
        let cmd = format_with_entry(
            None,
            "op-1",
            Some(r"C:\Users\me\Desktop\pm-trial"),
            ShellHint::Default,
        );
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
        let cmd = format_with_entry(Some(&entry), "op-1", None, ShellHint::Default);
        assert!(cmd.starts_with("node "), "{cmd}");
        assert!(!cmd.contains("npx"), "{cmd}");
        assert!(cmd.contains("index.js watch op-1"), "{cmd}");
        assert!(!cmd.contains('\\') || cmd.contains('"'), "{cmd}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn backslash_and_spaced_windows_paths_become_shell_safe_in_every_form() {
        let project = r"C:\Users\Ren Pc\My Project";
        let default = format_with_entry(None, "op-1", Some(project), ShellHint::Default);
        assert!(
            default.ends_with("--project-path 'C:/Users/Ren Pc/My Project'"),
            "{default}"
        );
        assert!(!default.contains('\\'), "{default}");
        let bash = format_with_entry(None, "op-1", Some(project), ShellHint::Bash);
        assert!(bash.ends_with("--project-path 'C:/Users/Ren Pc/My Project'"), "{bash}");
        let pwsh = format_with_entry(None, "op-1", Some(project), ShellHint::PowerShell);
        assert!(pwsh.ends_with("--project-path 'C:/Users/Ren Pc/My Project'"), "{pwsh}");
        let cmd = format_with_entry(None, "op-1", Some(project), ShellHint::Cmd);
        assert!(cmd.ends_with("--project-path \"C:/Users/Ren Pc/My Project\""), "{cmd}");
        let unc = format_with_entry(None, "op-1", Some(r"\\srv\share\a b"), ShellHint::Default);
        assert!(unc.ends_with("--project-path '//srv/share/a b'"), "{unc}");
    }

    #[test]
    fn plain_values_stay_unquoted_and_posix_backslashes_are_preserved() {
        let cmd = format_with_entry(None, "op-1", Some("/home/me/proj"), ShellHint::Bash);
        assert_eq!(cmd, "npx puppet-master watch op-1 --project-path /home/me/proj");
        let odd = format_with_entry(None, "op-1", Some(r"/tmp/a\b c"), ShellHint::Bash);
        assert!(odd.ends_with(r"--project-path '/tmp/a\b c'"), "{odd}");
    }

    #[test]
    fn single_quotes_in_values_are_escaped_per_shell() {
        let value = "it's here";
        assert_eq!(shell_quote(value, ShellHint::Bash), r"'it'\''s here'");
        assert_eq!(shell_quote(value, ShellHint::PowerShell), "'it''s here'");
        assert_eq!(shell_quote(value, ShellHint::Cmd), "\"it's here\"");
        assert_eq!(shell_quote(value, ShellHint::Default), "\"it's here\"");
        assert_eq!(shell_quote("", ShellHint::Default), "''");
    }

    #[test]
    fn reformat_keeps_the_project_path_across_shells() {
        let original = format_with_entry(None, "op-9", Some(r"C:\a b\it's"), ShellHint::Default);
        // Only the pure path-recovery helper is exercised here: the public wrapper also resolves
        // (and copies) the CLI bundle into the real per-user app-data directory.
        let project = existing_project_path(&original).expect("project path");
        assert_eq!(project, "C:/a b/it's");
        let bash = format_with_entry(None, "op-9", Some(&project), ShellHint::Bash);
        assert_eq!(bash, r"npx puppet-master watch op-9 --project-path 'C:/a b/it'\''s'");
        let cmd = format_with_entry(None, "op-9", Some(&project), ShellHint::Cmd);
        assert!(cmd.ends_with("--project-path \"C:/a b/it's\""), "{cmd}");
        assert_eq!(existing_project_path("npx puppet-master watch op-9"), None);
        let pwsh_quoted = format_with_entry(None, "op-9", Some("D:/x y"), ShellHint::PowerShell);
        assert_eq!(existing_project_path(&pwsh_quoted).as_deref(), Some("D:/x y"));
    }

    #[test]
    fn reformat_keeps_the_entry_chosen_by_the_app() {
        let bundle = "C:/Users/me/AppData/Roaming/app/cli/puppet-master-cli.bundle.mjs";
        let original = format!("node {bundle} watch op-3 --project-path /p");
        assert_eq!(existing_entry(&original), Some(PathBuf::from(bundle)));
        let spaced = "node 'C:/Users/Ren Pc/cli/x.mjs' watch op-3";
        assert_eq!(
            existing_entry(spaced),
            Some(PathBuf::from("C:/Users/Ren Pc/cli/x.mjs"))
        );
        assert_eq!(existing_entry("npx puppet-master watch op-3"), None);
        let out = reformat_for_shell(&original, "op-3", None, ShellHint::Bash);
        assert!(out.starts_with(&format!("node {bundle} watch op-3")), "{out}");
        assert!(!out.contains("dist/index.js"), "{out}");
    }

    #[test]
    fn shell_hint_parses_common_names() {
        assert_eq!(ShellHint::parse("Git-Bash"), Some(ShellHint::Bash));
        assert_eq!(ShellHint::parse("pwsh"), Some(ShellHint::PowerShell));
        assert_eq!(ShellHint::parse("CMD"), Some(ShellHint::Cmd));
        assert_eq!(ShellHint::parse("fish"), None);
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

        // Quoted correctly for a path with spaces, from any cwd. The default form is
        // single-quoted (literal in both PowerShell and Git Bash; double quotes were not
        // shell-neutral once backslashes were involved).
        let cmd = format_with_entry(Some(&copied), "op-1", Some("C:/my project"), ShellHint::Default);
        assert!(cmd.starts_with("node '"), "{cmd}");
        assert!(cmd.contains("app data/cli/"), "{cmd}");
        assert!(cmd.contains("--project-path 'C:/my project'"), "{cmd}");

        // A rebuilt bundle replaces the stale copy; the workspace dist/index.js is never copied.
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(&source, "console.log('v2 longer')").unwrap();
        let again = stable_copy(&source, &stable).unwrap();
        assert_eq!(std::fs::read_to_string(again).unwrap(), "console.log('v2 longer')");
        assert!(stable_copy(&base.join("index.js"), &stable).is_none());
        let _ = std::fs::remove_dir_all(base);
    }
}
