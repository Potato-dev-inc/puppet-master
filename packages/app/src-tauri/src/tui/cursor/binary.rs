//! Which Cursor agent CLI a pane or headless run launches. Cursor installs `agent` as a short alias
//! of `cursor-agent` (on Windows `agent.cmd` next to `cursor-agent.cmd`/`.ps1`; both end in the
//! same PowerShell script). The `agent` alias is preferred, `cursor-agent` is the fallback.

use std::path::{Path, PathBuf};

/// The launcher found on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorCli {
    /// `agent.cmd`/`cursor-agent.cmd` (Windows) run under `cmd.exe /C`.
    CmdShim(PathBuf),
    /// `cursor-agent.ps1` (Windows) run through PowerShell.
    Ps1(PathBuf),
    /// A native/unix launcher by name (`agent` or `cursor-agent`).
    Named(&'static str),
}

/// Pure choice over a lookup: `find(name)` is a PATH search, `is_cursor_alias(path)` says whether a
/// file named `agent` really is Cursor's alias (an unrelated tool called `agent` must not win).
pub(crate) fn pick(
    windows: bool,
    find: &dyn Fn(&str) -> Option<PathBuf>,
    is_cursor_alias: &dyn Fn(&Path) -> bool,
) -> Option<CursorCli> {
    if windows {
        if let Some(path) = find("agent.cmd").filter(|p| is_cursor_alias(p)) {
            return Some(CursorCli::CmdShim(path));
        }
        if let Some(path) = find("cursor-agent.ps1") {
            return Some(CursorCli::Ps1(path));
        }
        return find("cursor-agent.cmd").map(CursorCli::CmdShim);
    }
    if find("cursor-agent").is_some() {
        return Some(CursorCli::Named("cursor-agent"));
    }
    find("agent")
        .filter(|p| is_cursor_alias(p))
        .map(|_| CursorCli::Named("agent"))
}

/// `agent.cmd` is Cursor's alias when it sits next to the Cursor shims or names `cursor-agent`.
pub(crate) fn looks_like_cursor_alias(path: &Path) -> bool {
    let sibling = |name: &str| path.with_file_name(name).is_file();
    sibling("cursor-agent.ps1")
        || sibling("cursor-agent.cmd")
        || sibling("cursor-agent")
        || std::fs::read_to_string(path).is_ok_and(|text| text.contains("cursor-agent"))
}

/// The launcher on this machine's PATH.
pub(crate) fn installed() -> Option<CursorCli> {
    #[cfg(test)]
    if let Some(stubbed) = stub::current() {
        return stubbed;
    }
    pick(
        cfg!(windows),
        &|name| crate::tui::find_path_file(name),
        &looks_like_cursor_alias,
    )
}

/// The PowerShell script both Windows shims run; headless runs call it directly (no `cmd.exe`
/// quoting of the task text). Found beside the chosen shim, else the script itself.
pub(crate) fn ps1_script(cli: &CursorCli) -> Option<PathBuf> {
    match cli {
        CursorCli::Ps1(path) => Some(path.clone()),
        CursorCli::CmdShim(path) => {
            Some(path.with_file_name("cursor-agent.ps1")).filter(|p| p.is_file())
        }
        CursorCli::Named(_) => None,
    }
}

/// Program and arguments that start `cli` as an interactive pane with `extra_args`.
pub(crate) fn visible_launch(cli: &CursorCli, extra_args: &[String]) -> (String, Vec<String>) {
    match cli {
        CursorCli::CmdShim(path) => {
            let mut args = vec!["/C".to_string(), path.to_string_lossy().into_owned()];
            args.extend(extra_args.iter().cloned());
            ("cmd.exe".into(), args)
        }
        CursorCli::Ps1(path) => {
            let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
            let mut call = format!("& {}", quote(&path.to_string_lossy()));
            for arg in extra_args {
                call.push(' ');
                call.push_str(&quote(arg));
            }
            (
                "powershell.exe".into(),
                vec!["-NoLogo".into(), "-NoExit".into(), "-Command".into(), call],
            )
        }
        CursorCli::Named(name) => ((*name).into(), extra_args.to_vec()),
    }
}

/// Test seam: lets a test give itself a Cursor CLI (or none) without touching the process-wide
/// `PATH`, so it passes whether or not Cursor is installed on the machine and cannot race the
/// parallel tests. The override is per thread; every test below runs its launch on its own thread.
#[cfg(test)]
pub(crate) mod stub {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static OVERRIDE: RefCell<Option<Option<CursorCli>>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<Option<CursorCli>> {
        OVERRIDE.with(|slot| slot.borrow().clone())
    }

    /// Restores the real lookup (and deletes the stub files) when dropped.
    pub(crate) struct StubCli {
        dir: Option<PathBuf>,
    }

    impl StubCli {
        /// The directory holding the stub launchers, `None` for [`not_installed`].
        pub(crate) fn dir(&self) -> Option<&Path> {
            self.dir.as_deref()
        }
    }

    impl Drop for StubCli {
        fn drop(&mut self) {
            OVERRIDE.with(|slot| *slot.borrow_mut() = None);
            if let Some(dir) = &self.dir {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    /// A temp dir holding stub Cursor launchers, picked by the real [`pick`] exactly as a PATH
    /// search of that directory would. What the stubs contain is irrelevant: nothing runs them.
    pub(crate) fn installed() -> StubCli {
        let dir = std::env::temp_dir().join(format!("pm-stub-cursor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("stub dir");
        let names: &[&str] = if cfg!(windows) {
            &["agent.cmd", "cursor-agent.cmd", "cursor-agent.ps1"]
        } else {
            &["cursor-agent"]
        };
        for name in names {
            std::fs::write(dir.join(name), "cursor-agent stub").expect("stub launcher");
        }
        let cli = pick(cfg!(windows), &|name| Some(dir.join(name)).filter(|p| p.is_file()), &looks_like_cursor_alias);
        assert!(cli.is_some(), "the stub launchers must be picked");
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(cli));
        StubCli { dir: Some(dir) }
    }

    /// No Cursor CLI anywhere, whatever the machine has installed.
    #[allow(dead_code)]
    pub(crate) fn not_installed() -> StubCli {
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(None));
        StubCli { dir: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finder(present: &'static [&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |name| {
            present
                .contains(&name)
                .then(|| PathBuf::from(format!(r"C:\cursor-agent\{name}")))
        }
    }

    #[test]
    fn windows_prefers_the_agent_alias_over_cursor_agent() {
        let find = finder(&["agent.cmd", "cursor-agent.cmd", "cursor-agent.ps1"]);
        let cli = pick(true, &find, &|_| true).unwrap();
        assert_eq!(cli, CursorCli::CmdShim(PathBuf::from(r"C:\cursor-agent\agent.cmd")));
    }

    #[test]
    fn windows_falls_back_to_the_ps1_then_the_cmd_shim() {
        let ps1 = pick(true, &finder(&["cursor-agent.ps1", "cursor-agent.cmd"]), &|_| true).unwrap();
        assert!(matches!(ps1, CursorCli::Ps1(_)));
        let cmd = pick(true, &finder(&["cursor-agent.cmd"]), &|_| true).unwrap();
        assert!(matches!(cmd, CursorCli::CmdShim(p) if p.ends_with("cursor-agent.cmd")));
        assert!(pick(true, &finder(&[]), &|_| true).is_none());
    }

    #[test]
    fn an_unrelated_agent_cmd_is_not_used() {
        let find = finder(&["agent.cmd", "cursor-agent.ps1"]);
        let cli = pick(true, &find, &|_| false).unwrap();
        assert!(matches!(cli, CursorCli::Ps1(_)));
        assert!(pick(true, &finder(&["agent.cmd"]), &|_| false).is_none());
    }

    #[test]
    fn unix_uses_cursor_agent_then_a_verified_agent() {
        assert_eq!(
            pick(false, &finder(&["agent", "cursor-agent"]), &|_| true),
            Some(CursorCli::Named("cursor-agent"))
        );
        assert_eq!(pick(false, &finder(&["agent"]), &|_| true), Some(CursorCli::Named("agent")));
        assert_eq!(pick(false, &finder(&["agent"]), &|_| false), None);
    }

    #[test]
    fn launch_lines_quote_extra_args_per_shim() {
        let extra = vec!["--model".to_string(), "it's".to_string()];
        let cmd = CursorCli::CmdShim(PathBuf::from(r"C:\c\agent.cmd"));
        let (program, args) = visible_launch(&cmd, &extra);
        assert_eq!(program, "cmd.exe");
        assert_eq!(args, ["/C", r"C:\c\agent.cmd", "--model", "it's"]);
        let ps1 = CursorCli::Ps1(PathBuf::from(r"C:\c\cursor-agent.ps1"));
        let (program, args) = visible_launch(&ps1, &extra);
        assert_eq!(program, "powershell.exe");
        assert_eq!(args[..3], ["-NoLogo", "-NoExit", "-Command"]);
        assert_eq!(args[3], r"& 'C:\c\cursor-agent.ps1' '--model' 'it''s'");
        assert_eq!(visible_launch(&CursorCli::Named("agent"), &extra).0, "agent");
    }

    #[test]
    fn the_headless_script_is_the_shim_sibling() {
        let dir = std::env::temp_dir().join(format!("pm-cursor-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cmd, ps1) = (dir.join("agent.cmd"), dir.join("cursor-agent.ps1"));
        std::fs::write(&cmd, "@echo off\r\npowershell -File cursor-agent.ps1 %*").unwrap();
        std::fs::write(&ps1, "").unwrap();
        assert!(looks_like_cursor_alias(&cmd));
        assert_eq!(ps1_script(&CursorCli::CmdShim(cmd.clone())), Some(ps1.clone()));
        assert_eq!(ps1_script(&CursorCli::Named("agent")), None);
        let _ = (std::fs::remove_file(&cmd), std::fs::remove_file(&ps1), std::fs::remove_dir(&dir));
    }
}
