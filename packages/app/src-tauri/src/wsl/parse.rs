//! Parsing `wsl.exe --list --verbose` and the probe output (SPEC n33 section 3.5). Pure: no process
//! starts here (`probe.rs` runs the commands). Everything WSL prints is untrusted. A distro name, a
//! folder, or a program path is data: a name that fails `DistroName::parse` is dropped, and the probe
//! script is a fixed constant that takes its values only as positional arguments.

use crate::wsl::model::{Distro, DistroFacts, DistroName, IGNORED_DISTRO_PREFIXES};
use std::collections::BTreeMap;

/// The fixed POSIX sh probe. `$1` is the nonce, the rest are program names. It prints only lines
/// `PM1:<nonce>:<key>=<value>`. Anything else it prints (rc-file noise) is ignored by `probe`, and
/// a line with the wrong nonce is ignored too.
pub const PROBE_SCRIPT: &str = r#"nonce=$1
shift
pm() { printf 'PM1:%s:%s=%s\n' "$nonce" "$1" "$2"; }
pm uid "$(id -u 2>/dev/null)"
pm home "$HOME"
pm home_windows "$(wslpath -w "$HOME" 2>/dev/null)"
mr=$(wslpath -u 'C:\' 2>/dev/null)
pm mount_root "${mr%c/}"
g=$(git --version 2>/dev/null | sed -n 's/^git version \([0-9][0-9A-Za-z._-]*\).*$/\1/p')
pm git "$g"
if [ -e /proc/sys/fs/binfmt_misc/WSLInterop ] || [ -e /proc/sys/fs/binfmt_misc/WSLInterop-late ]; then i=1; else i=0; fi
pm interop "$i"
pm networking "$(wslinfo --networking-mode 2>/dev/null)"
login_shell=$(getent passwd "$(id -un 2>/dev/null)" 2>/dev/null | cut -d: -f7)
login_path=""
if [ -n "$login_shell" ] && [ -x "$login_shell" ]; then
  login_path=$("$login_shell" -lic 'printf "\nPMPATH:%s\n" "$PATH"' </dev/null 2>/dev/null | sed -n 's/^PMPATH://p' | tail -n 1)
fi
if [ -z "$login_path" ]; then
  login_path=$(/bin/sh -l -c 'printf "\nPMPATH:%s\n" "$PATH"' </dev/null 2>/dev/null | sed -n 's/^PMPATH://p' | tail -n 1)
fi
[ -n "$login_path" ] || login_path=$PATH
pm path "$login_path"
for prog in "$@"; do
  found=$(PATH="$login_path"; command -v "$prog" 2>/dev/null)
  pm "prog:$prog" "$found"
done
"#;

/// Why a probe result cannot be used. Nothing here is shown raw to the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// A program name fails `^[A-Za-z0-9._-]{1,64}$`. Refused before any call.
    BadProgram(String),
    /// The nonce is empty, longer than 64 characters, or has a character outside `[A-Za-z0-9_-]`.
    BadNonce,
    /// A required key was not printed, or printed nothing.
    Missing(&'static str),
    /// A required key printed a value that cannot be right (a Windows path as `home_windows`, say).
    BadValue(&'static str),
}

/// Text from `wsl.exe` bytes. UTF-16LE when the bytes start with a `FF FE` BOM or have NUL bytes at
/// odd offsets (the default output of `wsl.exe` without `WSL_UTF8=1`); otherwise UTF-8, lossy. A
/// UTF-8 BOM, every `\0`, and every `\r` are dropped.
pub fn decode(bytes: &[u8]) -> String {
    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
        utf16le(&bytes[2..])
    } else if looks_utf16le(bytes) {
        utf16le(bytes)
    } else {
        let utf8 = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8_lossy(utf8).into_owned()
    };
    text.chars().filter(|c| *c != '\0' && *c != '\r').collect()
}

fn utf16le(bytes: &[u8]) -> String {
    // An odd trailing byte (a cut-off read) is dropped by `chunks_exact`.
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

fn looks_utf16le(bytes: &[u8]) -> bool {
    bytes.iter().skip(1).step_by(2).any(|b| *b == 0)
}

/// The distros in `wsl.exe --list --verbose` text. The first non-empty line is the header (any
/// language) and is skipped. Each other line: a leading `*` marks the default; the first token is the
/// name; the last token must be the version `1` or `2`; the tokens between are the state (display
/// only, localised). Names that fail `DistroName::parse` or start with an ignored prefix (Docker
/// Desktop, Rancher Desktop; case-insensitive) are dropped.
pub fn list(text: &str) -> Vec<Distro> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .skip(1)
        .filter_map(list_line)
        .collect()
}

fn list_line(line: &str) -> Option<Distro> {
    let trimmed = line.trim_start();
    let (default, rest) = match trimmed.strip_prefix('*') {
        Some(after) => (true, after),
        None => (false, trimmed),
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }
    let version = match tokens[tokens.len() - 1] {
        "1" => 1,
        "2" => 2,
        _ => return None,
    };
    let name = DistroName::parse(tokens[0])?;
    if is_ignored(name.as_str()) {
        return None;
    }
    Some(Distro {
        name: name.as_str().to_string(),
        default,
        version,
        state: tokens[1..tokens.len() - 1].join(" "),
    })
}

fn is_ignored(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    IGNORED_DISTRO_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

/// The argv that runs `PROBE_SCRIPT`, starting at `--exec`. The caller puts `-d <distro>` in front:
/// `["-d", distro, <these>]`. The nonce and each program name are checked first, and a bad one is
/// refused before anything runs. The script text never changes with the arguments.
pub fn probe_args(nonce: &str, programs: &[String]) -> Result<Vec<String>, ProbeError> {
    if !valid_nonce(nonce) {
        return Err(ProbeError::BadNonce);
    }
    let mut args = vec![
        "--exec".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        PROBE_SCRIPT.to_string(),
        "probe".to_string(),
        nonce.to_string(),
    ];
    for program in programs {
        if !valid_program(program) {
            return Err(ProbeError::BadProgram(program.clone()));
        }
        args.push(program.clone());
    }
    Ok(args)
}

/// The facts from probe output. Only lines `PM1:<nonce>:<key>=<value>` count. `distro` stays empty
/// and `checked_at_ms` stays 0: this function only reads text, so the caller (`probe.rs`) fills them.
/// Required: `uid`, `home`, `home_windows` (a `\\wsl.localhost\` or `\\wsl$\` path), `mount_root`
/// (starts and ends with `/`), `path`. Optional, and `None` when missing or odd: `git`, `networking`;
/// `interop` is false when missing. A program is found only when its value is an absolute path.
pub fn probe(text: &str, nonce: &str, programs: &[String]) -> Result<DistroFacts, ProbeError> {
    if !valid_nonce(nonce) {
        return Err(ProbeError::BadNonce);
    }
    let prefix = format!("PM1:{nonce}:");
    let mut values: BTreeMap<&str, &str> = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if let Some((key, value)) = rest.split_once('=') {
            values.insert(key, value);
        }
    }

    let uid = required(&values, "uid")?.parse::<u32>().map_err(|_| ProbeError::BadValue("uid"))?;
    let home = required(&values, "home")?;
    if !home.starts_with('/') {
        return Err(ProbeError::BadValue("home"));
    }
    let home_windows = required(&values, "home_windows")?;
    if !is_wsl_unc(home_windows) {
        return Err(ProbeError::BadValue("home_windows"));
    }
    let mount_root = required(&values, "mount_root")?;
    if !(mount_root.starts_with('/') && mount_root.ends_with('/')) {
        return Err(ProbeError::BadValue("mount_root"));
    }
    let path = required(&values, "path")?;

    let git = values.get("git").copied().filter(|v| valid_git(v)).map(str::to_string);
    let networking = values
        .get("networking")
        .copied()
        .filter(|v| v.len() <= 20 && v.chars().all(|c| c.is_ascii_lowercase()))
        .map(str::to_string);
    let programs = programs
        .iter()
        .map(|program| {
            let key = format!("prog:{program}");
            let found = values.get(key.as_str()).copied().filter(|v| v.starts_with('/')).map(str::to_string);
            (program.clone(), found)
        })
        .collect();

    Ok(DistroFacts {
        distro: String::new(),
        checked_at_ms: 0,
        uid,
        home: home.to_string(),
        home_windows: home_windows.to_string(),
        mount_root: mount_root.to_string(),
        path: path.to_string(),
        git,
        interop: values.get("interop") == Some(&"1"),
        networking,
        programs,
    })
}

fn required<'a>(values: &BTreeMap<&str, &'a str>, key: &'static str) -> Result<&'a str, ProbeError> {
    values.get(key).copied().filter(|v| !v.is_empty()).ok_or(ProbeError::Missing(key))
}

fn is_wsl_unc(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with(r"\\wsl.localhost\") || lower.starts_with(r"\\wsl$\") || is_fake_home(value)
}

/// The end-to-end tests stand a temp Windows folder in for the distro's home (`pm-fake-wsl` prints it
/// as `home_windows`). Only in builds with feature `dev-fake-tui`, and only that exact folder.
#[cfg(feature = "dev-fake-tui")]
fn is_fake_home(value: &str) -> bool {
    std::env::var("PM_FAKE_WSL_HOME").is_ok_and(|home| !home.is_empty() && home == value)
}

#[cfg(not(feature = "dev-fake-tui"))]
fn is_fake_home(_value: &str) -> bool {
    false
}

fn valid_git(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= 64
        && nonce.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn valid_program(program: &str) -> bool {
    !program.is_empty()
        && program.len() <= 64
        && program.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
