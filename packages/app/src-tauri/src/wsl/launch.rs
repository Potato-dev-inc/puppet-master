//! Argv and environment for starting an agent inside the distro (pure, SPEC-n33 section 3.4). No process
//! starts here: `wrap` builds the `wsl.exe` launch and the PTY layer turns it into a `CommandBuilder`.
//! No shell parses anything: the agent's program and arguments go after `--exec` one element each, and the
//! only script is the fixed `LAUNCHER` constant.

use crate::tui::{LaunchEnv, LaunchSpec};
use crate::wsl::model::{DistroName, LinuxPlace};
use std::collections::BTreeSet;
use std::path::Path;

/// Sets PATH from `PM_WSL_PATH` (the login PATH the probe read), drops the marker, then runs the program
/// with its arguments. `"$@"` keeps every argument as one element.
pub const LAUNCHER: &str =
    r#"PATH="${PM_WSL_PATH:-$PATH}"; export PATH; unset PM_WSL_PATH; exec "$@""#;
/// `$0` of the launcher shell. A label only; the launcher never runs it.
pub const ARGV0: &str = "puppet-master";
/// Names that cross in both directions (a Windows program started from Linux through interop sees them too).
pub const BOTH_WAYS: &[&str] = &[
    "PUPPET_MASTER_PANE_ID",
    "PM_HOOK_TOKEN",
    "PM_HOOK_PANE",
    "PM_HOOK_BRIDGE",
];

/// The login PATH marker. Always crosses, Windows to Linux.
const PATH_ENV: &str = "PM_WSL_PATH";
/// The variable that tells WSL which names cross. PM always sets it.
const WSLENV: &str = "WSLENV";

/// Where the agent runs. `windows_cwd` is the pane's folder as Windows sees it (it stays the working
/// directory of the `wsl.exe` process).
pub struct Target<'a> {
    pub wsl_exe: &'a Path,
    pub place: &'a LinuxPlace,
    pub windows_cwd: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// The distro name fails `DistroName::parse`.
    BadDistro,
    /// A variable name that is not `^[A-Za-z_][A-Za-z0-9_]{0,63}$`, or a name PM reserves (`WSLENV`,
    /// `PM_WSL_PATH`). Carries the name, never the value.
    BadName(String),
    /// A value that starts like a Windows path. Carries the variable name, never the value.
    WindowsPathValue(String),
    /// The Linux folder or the program path is not absolute.
    NotAbsolute,
}

/// The `wsl.exe` launch for an agent: `-d <distro> --cd <linux cwd> --exec /bin/sh -c LAUNCHER puppet-master
/// <program> <args...>`. The environment is the crossing set plus `WSLENV`; nothing else of PM's env is set
/// here, and `env.remove` is dropped (nothing else crosses).
pub fn wrap(t: &Target, args: &[String], env: &LaunchEnv) -> Result<LaunchSpec, LaunchError> {
    let distro = DistroName::parse(&t.place.distro).ok_or(LaunchError::BadDistro)?;
    if !t.place.linux_cwd.starts_with('/') || !t.place.program_abs.starts_with('/') {
        return Err(LaunchError::NotAbsolute);
    }
    let crossed = crossing(env, t.place)?;
    let names: Vec<String> = crossed.set.iter().map(|(name, _)| name.clone()).collect();
    let mut spawn_env = crossed.set;
    // Always set, so an inherited WSLENV from PM's own process never applies, also when the list is short.
    spawn_env.push((WSLENV.to_string(), wslenv(&names)));

    let mut argv: Vec<String> = vec![
        "-d".into(),
        distro.as_str().to_string(),
        "--cd".into(),
        t.place.linux_cwd.clone(),
        "--exec".into(),
        "/bin/sh".into(),
        "-c".into(),
        LAUNCHER.into(),
        ARGV0.into(),
        t.place.program_abs.clone(),
    ];
    argv.extend(args.iter().cloned());

    let mut spec = LaunchSpec::new(
        t.wsl_exe.to_string_lossy().into_owned(),
        argv,
        t.windows_cwd.to_path_buf(),
        false,
    );
    spec.env = spawn_env;
    Ok(spec)
}

/// The crossing environment: every `env.set` name (validated, and no Windows-path values), plus
/// `PM_WSL_PATH` from the probe. `env.remove` is dropped.
pub fn crossing(env: &LaunchEnv, place: &LinuxPlace) -> Result<LaunchEnv, LaunchError> {
    let mut set: Vec<(String, String)> = Vec::with_capacity(env.set.len() + 1);
    for (name, value) in &env.set {
        if !valid_name(name) || name == PATH_ENV || name == WSLENV {
            return Err(LaunchError::BadName(name.clone()));
        }
        if looks_like_windows_path(value) {
            return Err(LaunchError::WindowsPathValue(name.clone()));
        }
        set.push((name.clone(), value.clone()));
    }
    set.push((PATH_ENV.to_string(), place.path.clone()));
    Ok(LaunchEnv {
        set,
        remove: Vec::new(),
    })
}

/// `"A/u:B/u:PUPPET_MASTER_PANE_ID"`: sorted, deduplicated. `BOTH_WAYS` names get no flag; the rest get
/// `/u` (Windows to Linux only). A name that is not a valid variable name is left out, never passed on.
pub fn wslenv(names: &[String]) -> String {
    let unique: BTreeSet<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| valid_name(name))
        .collect();
    unique
        .into_iter()
        .map(|name| {
            if BOTH_WAYS.contains(&name) {
                name.to_string()
            } else {
                format!("{name}/u")
            }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// `^[A-Za-z_][A-Za-z0-9_]{0,63}$`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `X:\`, `X:/` or `\\` at the start.
fn looks_like_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    drive || value.starts_with(r"\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wsl::path::Mounts;

    const WSL_EXE: &str = r"C:\Windows\System32\wsl.exe";
    const HOSTILE: &[&str] = &[
        r#"say "hi""#,
        r"trailing\",
        "two\nlines",
        "$(touch x)",
        "a;b && c | d",
        "with space",
        r"D:\work\a b\",
        "",
        "tab\there",
        "`backtick` $HOME 'single'",
    ];

    fn place() -> LinuxPlace {
        LinuxPlace {
            distro: "Ubuntu-Test".into(),
            linux_cwd: "/mnt/d/work/shop".into(),
            program_abs: "/usr/local/bin/claude".into(),
            path: "/usr/local/bin:/usr/bin:/bin".into(),
            mounts: Mounts {
                distro: "Ubuntu-Test".into(),
                mount_root: "/mnt/".into(),
                unc_root: r"\\wsl.localhost\Ubuntu-Test".into(),
            },
            home: "/home/you".into(),
            home_windows: r"\\wsl.localhost\Ubuntu-Test\home\you".into(),
        }
    }

    fn env_of(pairs: &[(&str, &str)]) -> LaunchEnv {
        LaunchEnv {
            set: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            remove: Vec::new(),
        }
    }

    fn wrap_with(
        place: &LinuxPlace,
        args: &[String],
        env: &LaunchEnv,
    ) -> Result<LaunchSpec, LaunchError> {
        let wsl = Path::new(WSL_EXE);
        let windows_cwd = Path::new(r"D:\work\shop");
        wrap(
            &Target {
                wsl_exe: wsl,
                place,
                windows_cwd,
            },
            args,
            env,
        )
    }

    fn argv_tail(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn wrap_builds_the_exact_argv() {
        let args = argv_tail(&["--model", "opus"]);
        let spec = wrap_with(
            &place(),
            &args,
            &env_of(&[("PUPPET_MASTER_PANE_ID", "pane-1")]),
        )
        .unwrap();
        assert_eq!(spec.program, WSL_EXE);
        assert_eq!(spec.cwd, Path::new(r"D:\work\shop"));
        assert_eq!(
            spec.args,
            argv_tail(&[
                "-d",
                "Ubuntu-Test",
                "--cd",
                "/mnt/d/work/shop",
                "--exec",
                "/bin/sh",
                "-c",
                LAUNCHER,
                "puppet-master",
                "/usr/local/bin/claude",
                "--model",
                "opus",
            ])
        );
        assert!(spec.env_remove.is_empty());
        assert!(!spec.read_only_enforced);
    }

    #[test]
    fn wrap_sets_the_exact_wslenv_and_the_allowed_names_only() {
        let env = env_of(&[("PUPPET_MASTER_PANE_ID", "pane-1")]);
        let spec = wrap_with(&place(), &[], &env).unwrap();
        assert_eq!(
            spec.env,
            vec![
                ("PUPPET_MASTER_PANE_ID".to_string(), "pane-1".to_string()),
                (
                    PATH_ENV.to_string(),
                    "/usr/local/bin:/usr/bin:/bin".to_string()
                ),
                (
                    WSLENV.to_string(),
                    "PM_WSL_PATH/u:PUPPET_MASTER_PANE_ID".to_string()
                ),
            ]
        );
        // Only the names PM listed (plus its own PATH marker and WSLENV) are in the spec env.
        let allowed = ["PUPPET_MASTER_PANE_ID", PATH_ENV, WSLENV];
        assert!(spec.env.iter().all(|(k, _)| allowed.contains(&k.as_str())));
        assert_eq!(
            spec.env.iter().filter(|(k, _)| k == WSLENV).count(),
            1,
            "WSLENV is set exactly once"
        );
    }

    #[test]
    fn wslenv_is_set_when_no_extra_names_are_given() {
        let spec = wrap_with(&place(), &[], &LaunchEnv::default()).unwrap();
        // The PATH marker is always in the list, so the value is never empty for a real launch.
        assert!(spec
            .env
            .contains(&(WSLENV.to_string(), "PM_WSL_PATH/u".to_string())));
        assert_eq!(
            wslenv(&[]),
            "",
            "an empty name list gives an empty WSLENV, and it is still set"
        );
    }

    #[test]
    fn wslenv_string_is_exact_sorted_and_deduplicated() {
        let names: Vec<String> = ["B", "A", "PUPPET_MASTER_PANE_ID", "A"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(wslenv(&names), "A/u:B/u:PUPPET_MASTER_PANE_ID");
        let hooks: Vec<String> = ["PM_HOOK_TOKEN", "PM_HOOK_PANE"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            wslenv(&hooks),
            "PM_HOOK_PANE:PM_HOOK_TOKEN",
            "BOTH_WAYS names carry no flag"
        );
    }

    #[test]
    fn wslenv_refuses_names_that_could_add_flags() {
        let names: Vec<String> = ["OK", "A/w", "B:C", ""]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(wslenv(&names), "OK/u");
    }

    #[test]
    fn inherited_wslenv_is_replaced_not_passed_on() {
        // The process env is never read here. The spec env carries exactly one WSLENV, which replaces any
        // WSLENV that PM's own process holds, so a name such as GITHUB_TOKEN never crosses.
        let spec = wrap_with(&place(), &[], &env_of(&[("PM_HOOK_TOKEN", "t")])).unwrap();
        let value = &spec.env.iter().find(|(k, _)| k == WSLENV).unwrap().1;
        assert!(!value.contains("GITHUB_TOKEN"));
        assert_eq!(value, "PM_HOOK_TOKEN:PM_WSL_PATH/u");
    }

    #[test]
    fn crossing_refuses_windows_path_values() {
        for bad in [r"C:\work\key", "D:/tools", r"\\server\share"] {
            let err = crossing(&env_of(&[("TOOL_HOME", bad)]), &place()).unwrap_err();
            assert_eq!(
                err,
                LaunchError::WindowsPathValue("TOOL_HOME".into()),
                "value {bad:?} must be refused"
            );
        }
        // A Linux-looking path and a plain word are fine.
        assert!(crossing(&env_of(&[("TOOL_HOME", "/home/you/tools")]), &place()).is_ok());
        assert!(crossing(&env_of(&[("TOOL_HOME", "plain")]), &place()).is_ok());
    }

    #[test]
    fn crossing_refuses_bad_and_reserved_names() {
        let too_long = "N".repeat(65);
        let limit = "N".repeat(64);
        for bad in [
            "",
            "1X",
            "A=B",
            "A;B",
            "A B",
            "A/u",
            "WSLENV",
            "PM_WSL_PATH",
            too_long.as_str(),
        ] {
            let err = crossing(&env_of(&[(bad, "x")]), &place()).unwrap_err();
            assert_eq!(
                err,
                LaunchError::BadName(bad.to_string()),
                "name {bad:?} must be refused"
            );
        }
        assert!(crossing(&env_of(&[("_OK_1", "x")]), &place()).is_ok());
        assert!(
            crossing(&env_of(&[(limit.as_str(), "x")]), &place()).is_ok(),
            "64 characters is the limit"
        );
    }

    #[test]
    fn crossing_drops_remove_and_adds_the_path_marker() {
        let env = LaunchEnv {
            set: vec![("PUPPET_MASTER_PANE_ID".into(), "pane-1".into())],
            remove: vec!["GITHUB_TOKEN".into()],
        };
        let crossed = crossing(&env, &place()).unwrap();
        assert!(crossed.remove.is_empty());
        assert_eq!(
            crossed.set,
            vec![
                ("PUPPET_MASTER_PANE_ID".to_string(), "pane-1".to_string()),
                (
                    PATH_ENV.to_string(),
                    "/usr/local/bin:/usr/bin:/bin".to_string()
                ),
            ]
        );
    }

    #[test]
    fn wrap_refuses_a_bad_distro_and_relative_paths() {
        let mut bad = place();
        for name in ["-x", "a b", "", "a/b"] {
            bad.distro = name.into();
            assert_eq!(
                wrap_with(&bad, &[], &LaunchEnv::default()).unwrap_err(),
                LaunchError::BadDistro,
                "{name:?}"
            );
        }
        let mut relative = place();
        relative.linux_cwd = "work/shop".into();
        assert_eq!(
            wrap_with(&relative, &[], &LaunchEnv::default()).unwrap_err(),
            LaunchError::NotAbsolute
        );
        let mut relative_program = place();
        relative_program.program_abs = "claude".into();
        assert_eq!(
            wrap_with(&relative_program, &[], &LaunchEnv::default()).unwrap_err(),
            LaunchError::NotAbsolute
        );
    }

    #[test]
    fn hostile_args_stay_one_element_each_after_the_program() {
        let args: Vec<String> = HOSTILE.iter().map(|s| s.to_string()).collect();
        let spec = wrap_with(&place(), &args, &LaunchEnv::default()).unwrap();
        // Everything before the program is fixed; every hostile argument is its own element, unchanged.
        assert_eq!(spec.args.len(), 10 + HOSTILE.len());
        assert_eq!(&spec.args[10..], args.as_slice());
        assert_eq!(spec.args[9], "/usr/local/bin/claude");
    }

    #[test]
    fn launcher_is_the_fixed_constant() {
        assert_eq!(
            LAUNCHER,
            r#"PATH="${PM_WSL_PATH:-$PATH}"; export PATH; unset PM_WSL_PATH; exec "$@""#
        );
        assert!(
            !LAUNCHER.contains("$1"),
            "the launcher never reads a positional other than \"$@\""
        );
    }

    /// portable-pty 0.8.1 `cmdbuilder::append_quoted` (private there): the rule the PTY layer applies to each
    /// argv element on Windows. Copied so the test can read back a command line.
    #[cfg(windows)]
    fn quote_like_portable_pty(arg: &str) -> String {
        if !arg.is_empty()
            && !arg
                .chars()
                .any(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '"'))
        {
            return arg.to_string();
        }
        let chars: Vec<char> = arg.chars().collect();
        let mut out = String::from('"');
        let mut i = 0;
        while i < chars.len() {
            let mut backslashes = 0;
            while i < chars.len() && chars[i] == '\\' {
                i += 1;
                backslashes += 1;
            }
            if i == chars.len() {
                out.extend(std::iter::repeat('\\').take(backslashes * 2));
                break;
            } else if chars[i] == '"' {
                out.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                out.push('"');
            } else {
                out.extend(std::iter::repeat('\\').take(backslashes));
                out.push(chars[i]);
            }
            i += 1;
        }
        out.push('"');
        out
    }

    #[cfg(windows)]
    #[link(name = "shell32")]
    extern "system" {
        fn CommandLineToArgvW(command_line: *const u16, num_args: *mut i32) -> *mut *mut u16;
    }

    #[cfg(windows)]
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    /// Splits a command line the way the Windows C runtime does, through the same Win32 call.
    #[cfg(windows)]
    fn command_line_to_argv(line: &str) -> Vec<String> {
        let wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
        let mut count: i32 = 0;
        // SAFETY: `wide` is nul-terminated and lives across the call; the returned array is freed below.
        unsafe {
            let argv = CommandLineToArgvW(wide.as_ptr(), &mut count);
            assert!(!argv.is_null(), "CommandLineToArgvW failed");
            let mut out = Vec::new();
            for i in 0..count as usize {
                let s = *argv.add(i);
                let mut len = 0;
                while *s.add(len) != 0 {
                    len += 1;
                }
                out.push(String::from_utf16_lossy(std::slice::from_raw_parts(s, len)));
            }
            LocalFree(argv as *mut std::ffi::c_void);
            out
        }
    }

    #[cfg(windows)]
    #[test]
    fn hostile_args_survive_the_windows_command_line() {
        let args: Vec<String> = HOSTILE.iter().map(|s| s.to_string()).collect();
        let spec = wrap_with(
            &place(),
            &args,
            &env_of(&[("PUPPET_MASTER_PANE_ID", "pane-1")]),
        )
        .unwrap();
        // The PTY layer builds its CommandBuilder from these fields (`build_launch_spec_command` is pub(super)
        // in pty::registry, so this test cannot call it). Read back the argv it holds.
        let mut cmd = portable_pty::CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let mut expected = vec![spec.program.clone()];
        expected.extend(spec.args.iter().cloned());
        assert_eq!(argv, expected);

        let line = argv
            .iter()
            .map(|a| quote_like_portable_pty(a))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            command_line_to_argv(&line),
            expected,
            "the command line splits back into the same argv"
        );
    }
}
