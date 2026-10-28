//! Windows and Linux folder paths for Linux Inside (SPEC-n33 section 3.3). Pure: no I/O, no process calls.
//!
//! `C:\x` is `/mnt/c/x`. The mount root comes from the probe (`wslpath`), so `wsl.conf` `root=/` gives `/c/x`.
//! A folder inside the distro (`\\wsl.localhost\<distro>\...`) cannot map to a Linux path in v1: `to_linux` says
//! `InsideDistro` and the caller refuses it. Every `..` is refused, never normalised away.
#![allow(dead_code)] // The launch and chat callers (WSL-7, WSL-10) come in later tasks.

use crate::session_chat::plain_path;

/// Where the distro is mounted and how Windows reaches its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounts {
    pub distro: String,
    /// Always ends with `/`: `/mnt/` by default, `/` with `wsl.conf` `root=/`.
    pub mount_root: String,
    /// `\\wsl.localhost\<distro>`.
    pub unc_root: String,
}

#[derive(Debug, PartialEq)]
pub enum PathError {
    Empty,
    Relative,
    NetworkShare,
    OtherDistro(String),
    InsideDistro,
    BadDrive,
    DotDot,
}

const SEPS: [char; 2] = ['\\', '/'];

/// A Windows folder as a Linux path. `C:\a b\` -> `/mnt/c/a b` (or `/c/a b` with mount root `/`).
pub fn to_linux(windows: &str, m: &Mounts) -> Result<String, PathError> {
    let plain = plain_path(windows);
    if plain.trim().is_empty() {
        return Err(PathError::Empty);
    }
    if segments(&plain).contains(&"..") {
        return Err(PathError::DotDot);
    }
    if is_unc(&plain) {
        return match distro_path(&plain) {
            Some((name, _)) if name.eq_ignore_ascii_case(&m.distro) => Err(PathError::InsideDistro),
            Some((name, _)) => Err(PathError::OtherDistro(name)),
            None => Err(PathError::NetworkShare),
        };
    }
    let (letter, parts) = drive_of(&plain)?.ok_or(PathError::Relative)?;
    let mut out = format!(
        "{}/{}",
        m.mount_root.trim_end_matches('/'),
        letter.to_ascii_lowercase()
    );
    for part in parts {
        out.push('/');
        out.push_str(part);
    }
    Ok(out)
}

/// A Linux folder as a Windows path. `/mnt/d/a` -> `D:\a`; any other absolute path -> `\\wsl.localhost\<distro>\...`.
pub fn to_windows(linux: &str, m: &Mounts) -> Result<String, PathError> {
    if linux.trim().is_empty() {
        return Err(PathError::Empty);
    }
    if !linux.starts_with('/') {
        return Err(PathError::Relative);
    }
    if segments(linux).contains(&"..") {
        return Err(PathError::DotDot);
    }
    let root = format!("{}/", m.mount_root.trim_end_matches('/'));
    if let Some(rest) = linux.strip_prefix(root.as_str()) {
        let (first, tail) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if first.len() == 1 && first.as_bytes()[0].is_ascii_alphabetic() {
            let drive = first.to_ascii_uppercase();
            let tail = segments(tail).join("\\");
            return Ok(if tail.is_empty() {
                format!("{drive}:\\")
            } else {
                format!("{drive}:\\{tail}")
            });
        }
    }
    let tail = segments(linux).join("\\");
    Ok(if tail.is_empty() {
        m.unc_root.trim_end_matches('\\').to_string()
    } else {
        format!("{}\\{tail}", m.unc_root.trim_end_matches('\\'))
    })
}

/// `\\wsl.localhost\<d>\x` or `\\wsl$\<d>\x` (any case) -> (`d` as written, `/x`). `None` for anything else,
/// including a `..` segment. Used by `to_linux` to tell a distro folder from another share.
pub fn distro_path(windows: &str) -> Option<(String, String)> {
    let plain = plain_path(windows);
    if !is_unc(&plain) {
        return None;
    }
    let parts = segments(&plain);
    let [host, distro, rest @ ..] = parts.as_slice() else {
        return None;
    };
    if !(host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$")) {
        return None;
    }
    if rest.contains(&"..") {
        return None;
    }
    Some((distro.to_string(), format!("/{}", rest.join("/"))))
}

fn is_unc(path: &str) -> bool {
    path.starts_with(r"\\") || path.starts_with("//")
}

/// The non-empty parts of a path, split on either separator.
fn segments(path: &str) -> Vec<&str> {
    path.split(SEPS).filter(|part| !part.is_empty()).collect()
}

/// `C:\rest` or `C:/rest` -> `Some(('C', ["rest", ...]))`. `None` when the first part has no colon.
/// `C:` and `C:x` (drive-relative) are `Relative`; a colon after anything but one letter (`1:\x`) is `BadDrive`.
fn drive_of(path: &str) -> Result<Option<(char, Vec<&str>)>, PathError> {
    let head_end = path.find(SEPS).unwrap_or(path.len());
    let head = &path[..head_end];
    if !head.contains(':') {
        return Ok(None);
    }
    let mut chars = head.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic);
    let colon_second = chars.next() == Some(':');
    match letter {
        Some(l) if colon_second && head_end == 2 && path.len() > 2 => {
            Ok(Some((l, segments(&path[2..]))))
        }
        Some(_) if colon_second => Err(PathError::Relative),
        _ => Err(PathError::BadDrive),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mnt() -> Mounts {
        Mounts {
            distro: "Ubuntu".into(),
            mount_root: "/mnt/".into(),
            unc_root: r"\\wsl.localhost\Ubuntu".into(),
        }
    }

    fn root() -> Mounts {
        Mounts {
            mount_root: "/".into(),
            ..mnt()
        }
    }

    #[test]
    fn drive_rows_map_under_mnt() {
        let cases = [
            (r"C:\Users\you\shop", "/mnt/c/Users/you/shop"),
            (r"\\?\D:\a b\c\", "/mnt/d/a b/c"),
            (r"D:\", "/mnt/d"),
            (r"C:\", "/mnt/c"),
            (r"C:\Users\you\shop\", "/mnt/c/Users/you/shop"),
            (r"c:\x", "/mnt/c/x"),
            (r"C:\Ünï 日本\note.txt", "/mnt/c/Ünï 日本/note.txt"),
            (r"C:\a..b", "/mnt/c/a..b"),
            (r"C:/Users/you/shop", "/mnt/c/Users/you/shop"),
        ];
        for (windows, linux) in cases {
            assert_eq!(
                to_linux(windows, &mnt()),
                Ok(linux.to_string()),
                "{windows}"
            );
        }
    }

    #[test]
    fn drive_rows_map_under_root_mount() {
        // `wsl.conf` `[automount] root=/`
        let cases = [
            (r"C:\x", "/c/x"),
            (r"D:\", "/d"),
            (r"\\?\D:\a b\c\", "/d/a b/c"),
            (r"e:\Users\you", "/e/Users/you"),
        ];
        for (windows, linux) in cases {
            assert_eq!(
                to_linux(windows, &root()),
                Ok(linux.to_string()),
                "{windows}"
            );
        }
    }

    #[test]
    fn extended_and_forward_slash_prefixes() {
        assert_eq!(
            to_linux(r"\\?\D:\a b\c\", &mnt()),
            Ok("/mnt/d/a b/c".to_string())
        );
        assert_eq!(
            to_linux("//?/D:/a b/c/", &mnt()),
            Ok("/mnt/d/a b/c".to_string())
        );
        assert_eq!(to_linux(r"\\.\D:\x", &mnt()), Ok("/mnt/d/x".to_string()));
        assert_eq!(to_linux("//?/d:/x", &mnt()), Ok("/mnt/d/x".to_string()));
    }

    #[test]
    fn refusals_to_linux() {
        let cases = [
            (
                r"\\wsl.localhost\Ubuntu\home\you\p",
                PathError::InsideDistro,
            ),
            (r"\\wsl$\ubuntu\home\you\p", PathError::InsideDistro),
            (r"\\WSL.LOCALHOST\UBUNTU\x", PathError::InsideDistro),
            (
                r"\\?\UNC\wsl.localhost\Ubuntu\home\you",
                PathError::InsideDistro,
            ),
            (r"\\wsl.localhost\Ubuntu", PathError::InsideDistro),
            (
                r"\\wsl.localhost\Debian\x",
                PathError::OtherDistro("Debian".into()),
            ),
            (r"\\server\share\x", PathError::NetworkShare),
            (r"\\?\UNC\server\share\x", PathError::NetworkShare),
            (r"//server/share/x", PathError::NetworkShare),
            (r"\\wsl.localhost\", PathError::NetworkShare),
            (r"shop\x", PathError::Relative),
            (r"\x", PathError::Relative),
            (r"/mnt/c/x", PathError::Relative),
            (r"C:", PathError::Relative),
            (r"C:x", PathError::Relative),
            (r"1:\x", PathError::BadDrive),
            (r":\x", PathError::BadDrive),
            (r"CC:\x", PathError::BadDrive),
            ("", PathError::Empty),
            ("   ", PathError::Empty),
            (r"\\?\", PathError::Empty),
            (r"C:\a\..\b", PathError::DotDot),
            (r"C:/a/../b", PathError::DotDot),
            (r"\\?\C:\a\..\b", PathError::DotDot),
            (r"\\wsl.localhost\Ubuntu\..\x", PathError::DotDot),
        ];
        for (windows, error) in cases {
            assert_eq!(to_linux(windows, &mnt()), Err(error), "{windows}");
        }
    }

    #[test]
    fn refusals_are_the_same_under_root_mount() {
        assert_eq!(
            to_linux(r"\\wsl.localhost\Ubuntu\home\you", &root()),
            Err(PathError::InsideDistro)
        );
        assert_eq!(to_linux(r"C:", &root()), Err(PathError::Relative));
        assert_eq!(to_linux(r"C:\a\..\b", &root()), Err(PathError::DotDot));
    }

    #[test]
    fn to_windows_drive_rows_under_mnt() {
        let cases = [
            ("/mnt/c/Users/you/shop", r"C:\Users\you\shop"),
            ("/mnt/d", r"D:\"),
            ("/mnt/d/", r"D:\"),
            ("/mnt/d/a b/c", r"D:\a b\c"),
            ("/mnt/c/Ünï 日本/note.txt", r"C:\Ünï 日本\note.txt"),
            ("/mnt/c/a//b", r"C:\a\b"),
        ];
        for (linux, windows) in cases {
            assert_eq!(
                to_windows(linux, &mnt()),
                Ok(windows.to_string()),
                "{linux}"
            );
        }
    }

    #[test]
    fn to_windows_drive_rows_under_root_mount() {
        assert_eq!(to_windows("/c/x", &root()), Ok(r"C:\x".to_string()));
        assert_eq!(to_windows("/d", &root()), Ok(r"D:\".to_string()));
        // Under `root=/` a folder named `home` is not a drive, so it stays in the distro.
        assert_eq!(
            to_windows("/home/you/p", &root()),
            Ok(r"\\wsl.localhost\Ubuntu\home\you\p".to_string())
        );
    }

    #[test]
    fn to_windows_distro_folders_use_the_unc_root() {
        let cases = [
            ("/home/you/p", r"\\wsl.localhost\Ubuntu\home\you\p"),
            ("/", r"\\wsl.localhost\Ubuntu"),
            ("/mnt", r"\\wsl.localhost\Ubuntu\mnt"),
            ("/mnt/cd/x", r"\\wsl.localhost\Ubuntu\mnt\cd\x"),
        ];
        for (linux, windows) in cases {
            assert_eq!(
                to_windows(linux, &mnt()),
                Ok(windows.to_string()),
                "{linux}"
            );
        }
        assert_eq!(
            to_windows("/mnt/c/x", &root()),
            Ok(r"\\wsl.localhost\Ubuntu\mnt\c\x".to_string())
        );
        let test_distro = Mounts {
            distro: "Ubuntu-Test".into(),
            unc_root: r"\\wsl.localhost\Ubuntu-Test".into(),
            ..mnt()
        };
        assert_eq!(
            to_windows("/home/you", &test_distro),
            Ok(r"\\wsl.localhost\Ubuntu-Test\home\you".to_string())
        );
    }

    #[test]
    fn to_windows_refusals() {
        assert_eq!(to_windows("", &mnt()), Err(PathError::Empty));
        assert_eq!(to_windows("  ", &mnt()), Err(PathError::Empty));
        assert_eq!(to_windows("home/you", &mnt()), Err(PathError::Relative));
        assert_eq!(to_windows("./x", &mnt()), Err(PathError::Relative));
        assert_eq!(to_windows("/mnt/c/a/../b", &mnt()), Err(PathError::DotDot));
        assert_eq!(to_windows("/a/../../etc", &root()), Err(PathError::DotDot));
    }

    #[test]
    fn round_trip_windows_drive_rows() {
        // (input, normalised: drive letter upper case, trailing separator dropped, drive root keeps `\`)
        let cases = [
            (r"C:\Users\you\shop", r"C:\Users\you\shop"),
            (r"\\?\D:\a b\c\", r"D:\a b\c"),
            (r"D:\", r"D:\"),
            (r"c:\x", r"C:\x"),
            (r"C:\Ünï 日本\a..b", r"C:\Ünï 日本\a..b"),
        ];
        for m in [mnt(), root()] {
            for (windows, normalised) in cases {
                let linux = to_linux(windows, &m).unwrap();
                assert_eq!(
                    to_windows(&linux, &m),
                    Ok(normalised.to_string()),
                    "{windows} via {linux}"
                );
            }
        }
    }

    #[test]
    fn round_trip_linux_drive_rows() {
        for linux in ["/mnt/c/Users/you/shop", "/mnt/d", "/mnt/c/a b/c"] {
            let windows = to_windows(linux, &mnt()).unwrap();
            assert_eq!(
                to_linux(&windows, &mnt()),
                Ok(linux.to_string()),
                "{linux} via {windows}"
            );
            // The same folder under `root=/` is `/c/...`.
            let root_form = linux.strip_prefix("/mnt").unwrap();
            let windows = to_windows(root_form, &root()).unwrap();
            assert_eq!(
                to_linux(&windows, &root()),
                Ok(root_form.to_string()),
                "{root_form} via {windows}"
            );
        }
    }

    #[test]
    fn distro_path_splits_host_distro_and_rest() {
        let cases = [
            (
                r"\\wsl.localhost\Ubuntu\home\you\p",
                Some(("Ubuntu", "/home/you/p")),
            ),
            (r"\\wsl$\ubuntu\home\you\p", Some(("ubuntu", "/home/you/p"))),
            (r"\\wsl.localhost\Ubuntu", Some(("Ubuntu", "/"))),
            (r"\\wsl.localhost\Ubuntu\", Some(("Ubuntu", "/"))),
            (r"\\?\UNC\wsl.localhost\Ubuntu\x", Some(("Ubuntu", "/x"))),
            (
                r"\\wsl.localhost\Ubuntu-Test\a b",
                Some(("Ubuntu-Test", "/a b")),
            ),
            (r"C:\x", None),
            (r"shop\x", None),
            (r"\\server\share\x", None),
            (r"\\wsl.localhost\", None),
            (r"\\wsl.localhost\Ubuntu\..\x", None),
            ("", None),
        ];
        for (windows, expected) in cases {
            let expected = expected.map(|(name, rest)| (name.to_string(), rest.to_string()));
            assert_eq!(distro_path(windows), expected, "{windows}");
        }
    }
}
