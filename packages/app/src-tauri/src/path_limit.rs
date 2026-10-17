//! The Windows working-directory limit.
//!
//! `CreateProcess` (and ConPTY, which sits on it) cannot start a program in a current directory
//! longer than `MAX_PATH`: 259 characters plus the terminating NUL. The `\\?\` prefix does not lift
//! it for a current directory. A run that names a folder past that limit used to fail inside the
//! spawn with a bare "os error 267" (`ERROR_DIRECTORY`, "The directory name is invalid"), for both
//! a visible pane and a headless child.
//!
//! This module is the one place that knows the limit. Callers check a folder before they spawn
//! anything (`check`), and a spawn that fails anyway with the raw error is mapped back to the same
//! plain answer (`from_io_error`, `from_error_text`). The error code is `PATH_TOO_LONG`.

use crate::operations::OperationError;
use serde_json::json;
use std::path::Path;

/// The error code every caller reports.
pub const CODE: &str = "PATH_TOO_LONG";

/// Longest working directory Windows can start a program in, in UTF-16 characters (`MAX_PATH` - 1).
pub const MAX_CWD_CHARS: usize = 259;

/// A raw spawn error 267 ("directory name is invalid") or 206 ("file name is too long") is only
/// read as "the folder is too long" when the folder really is close to the limit. A short folder
/// that does not exist gives 267 too, and must keep its own error.
const NEAR_LIMIT: usize = 200;

const ERROR_FILENAME_EXCED_RANGE: i32 = 206;
const ERROR_DIRECTORY: i32 = 267;

/// A folder too long to start a program in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTooLong {
    /// The folder's length in characters, as Windows counts it.
    pub length: usize,
}

impl PathTooLong {
    /// The plain sentence for a person or an agent.
    pub fn message(&self) -> String {
        if self.length > MAX_CWD_CHARS {
            format!(
                "This folder's path is {} characters. Windows can't start programs in a folder longer than {MAX_CWD_CHARS} characters. Use a shorter folder (for example closer to the drive root).",
                self.length
            )
        } else {
            // Only the raw-error fallback gets here: the length check passed but Windows still refused.
            format!(
                "This folder's path is {} characters, too close to the {MAX_CWD_CHARS} characters Windows allows for starting programs in a folder. Use a shorter folder (for example closer to the drive root).",
                self.length
            )
        }
    }

    /// The MCP/bridge error: `PATH_TOO_LONG`, not retryable (the same folder will fail again),
    /// with the numbers in `context`.
    pub fn into_error(self, folder: &Path) -> OperationError {
        let mut error = OperationError::new(CODE, self.message(), false);
        error.context = json!({
            "length": self.length,
            "limit": MAX_CWD_CHARS,
            "path": folder.display().to_string(),
        });
        error
    }
}

/// `path` as Windows counts it for the limit: UTF-16 units, without the `\\?\` prefix (it does not
/// raise the limit for a current directory) and without a trailing separator.
pub fn cwd_length(path: &Path) -> usize {
    let text = path.to_string_lossy();
    let unc;
    let text: &str = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        unc = format!(r"\\{rest}");
        &unc
    } else if let Some(rest) = text.strip_prefix(r"\\?\").or_else(|| text.strip_prefix(r"\\.\")) {
        rest
    } else {
        &text
    };
    let trimmed = text.trim_end_matches(['\\', '/']);
    // A bare root ("C:\") keeps its separator.
    let counted = if trimmed.is_empty() || trimmed.ends_with(':') { text } else { trimmed };
    counted.encode_utf16().count()
}

/// Pure rule behind `check`: is a folder of `length` characters too long?
pub fn too_long(length: usize) -> Option<PathTooLong> {
    (length > MAX_CWD_CHARS).then_some(PathTooLong { length })
}

/// Refuse a working directory Windows could not start a program in. Other platforms have no such
/// limit, so this only ever fails on Windows. A relative path is measured as it will resolve.
pub fn check(path: &Path) -> Result<(), PathTooLong> {
    if !cfg!(windows) {
        return Ok(());
    }
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir().map(|cwd| cwd.join(path)).unwrap_or_else(|_| path.to_path_buf());
        &absolute
    };
    match too_long(cwd_length(path)) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// `check` with the error already shaped for the bridge.
pub fn check_error(path: &Path) -> Result<(), OperationError> {
    check(path).map_err(|e| e.into_error(path))
}

fn mapped(code: i32, folder: &Path) -> Option<PathTooLong> {
    if code != ERROR_DIRECTORY && code != ERROR_FILENAME_EXCED_RANGE {
        return None;
    }
    let length = cwd_length(folder);
    (length >= NEAR_LIMIT).then_some(PathTooLong { length })
}

/// A spawn that failed with a raw OS error 267 or 206 while its folder is at or near the limit.
pub fn from_io_error(error: &std::io::Error, folder: &Path) -> Option<PathTooLong> {
    mapped(error.raw_os_error()?, folder)
}

/// The same for an error that only survives as text (`portable-pty` wraps the OS error into a
/// string: "... (os error 267)").
pub fn from_error_text(text: &str, folder: &Path) -> Option<PathTooLong> {
    [ERROR_DIRECTORY, ERROR_FILENAME_EXCED_RANGE]
        .into_iter()
        .find(|code| text.contains(&format!("os error {code}")))
        .and_then(|code| mapped(code, folder))
}

/// A failed pane spawn in `folder`: `PATH_TOO_LONG` when the folder is the reason, otherwise
/// `PANE_SPAWN_FAILED` with the spawn's own text.
pub fn pane_spawn_failure(text: String, folder: Option<&Path>) -> OperationError {
    if let Some(folder) = folder {
        let found = check(folder).err().or_else(|| from_error_text(&text, folder));
        if let Some(found) = found {
            return found.into_error(folder);
        }
    }
    OperationError::new("PANE_SPAWN_FAILED", text, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder_of(len: usize) -> PathBuf {
        let mut text = String::from(r"C:\w");
        while text.len() < len {
            text.push_str(r"\segment");
        }
        text.truncate(len);
        if text.ends_with('\\') {
            text.pop();
            text.push('x');
        }
        assert_eq!(text.len(), len);
        PathBuf::from(text)
    }

    #[test]
    fn the_limit_is_259_characters_exactly() {
        assert!(too_long(259).is_none());
        assert_eq!(too_long(260), Some(PathTooLong { length: 260 }));
        assert!(too_long(1).is_none());
    }

    #[test]
    fn the_message_names_the_length_and_the_limit_in_plain_words() {
        assert_eq!(
            PathTooLong { length: 283 }.message(),
            "This folder's path is 283 characters. Windows can't start programs in a folder longer than 259 characters. Use a shorter folder (for example closer to the drive root)."
        );
    }

    #[test]
    fn length_is_counted_without_the_extended_prefix_or_a_trailing_separator() {
        assert_eq!(cwd_length(Path::new(r"C:\a\b")), 6);
        assert_eq!(cwd_length(Path::new(r"C:\a\b\")), 6);
        assert_eq!(cwd_length(Path::new(r"\\?\C:\a\b")), 6);
        assert_eq!(cwd_length(Path::new(r"\\?\UNC\srv\share")), r"\\srv\share".len());
        assert_eq!(cwd_length(Path::new(r"C:\")), 3);
        // UTF-16 units, as Windows counts: one astral character is two.
        assert_eq!(cwd_length(Path::new("C:\\\u{1F600}")), 5);
    }

    #[cfg(windows)]
    #[test]
    fn check_refuses_a_folder_past_the_limit_and_accepts_one_at_it() {
        assert!(check(&folder_of(259)).is_ok());
        let error = check(&folder_of(283)).unwrap_err();
        assert_eq!(error.length, 283);
        // The extended prefix does not buy any room.
        let prefixed = PathBuf::from(format!(r"\\?\{}", folder_of(283).display()));
        assert_eq!(check(&prefixed).unwrap_err().length, 283);
    }

    #[cfg(windows)]
    #[test]
    fn check_error_has_the_mcp_shape() {
        let folder = folder_of(283);
        let error = check_error(&folder).unwrap_err();
        assert_eq!(error.code, "PATH_TOO_LONG");
        assert!(!error.recoverable);
        assert!(error.message.starts_with("This folder's path is 283 characters."), "{}", error.message);
        assert_eq!(error.context["length"], 283);
        assert_eq!(error.context["limit"], 259);
    }

    #[test]
    fn a_raw_267_or_206_near_the_limit_becomes_the_same_answer() {
        let long = folder_of(283);
        for code in [267, 206] {
            let io = std::io::Error::from_raw_os_error(code);
            let found = from_io_error(&io, &long).expect("mapped");
            assert_eq!(found.length, 283);
            assert!(found.message().contains("283 characters"));
            let text = format!("spawn_command: The directory name is invalid. (os error {code})");
            assert_eq!(from_error_text(&text, &long).map(|e| e.length), Some(283));
        }
    }

    #[test]
    fn a_raw_267_on_a_short_folder_keeps_its_own_error() {
        let short = Path::new(r"C:\no\such\folder");
        assert!(from_io_error(&std::io::Error::from_raw_os_error(267), short).is_none());
        assert!(from_error_text("(os error 267)", short).is_none());
        // Other OS errors are never read as "too long".
        let long = folder_of(283);
        assert!(from_io_error(&std::io::Error::from_raw_os_error(5), &long).is_none());
        assert!(from_error_text("(os error 5)", &long).is_none());
    }

    #[test]
    fn a_failed_pane_spawn_in_a_long_folder_reports_path_too_long() {
        let long = folder_of(283);
        let error = pane_spawn_failure("spawn_command: (os error 267)".into(), Some(&long));
        assert_eq!(error.code, "PATH_TOO_LONG");
        let other = pane_spawn_failure("openpty: broken".into(), Some(Path::new(r"C:\ok")));
        assert_eq!(other.code, "PANE_SPAWN_FAILED");
        assert!(other.recoverable);
    }

    /// Puppet Master's own generated folder names stay short, so a long-ish project does not turn
    /// into an unusable path the moment a separate copy or a wizard folder is added under it.
    #[test]
    fn folders_puppet_master_makes_for_itself_have_short_names() {
        let wizard = format!("pm-wizard-{}", crate::tui::declarative::wizard::session::Wizard::new_id());
        assert!(wizard.len() <= 26, "{wizard}");
    }
}
