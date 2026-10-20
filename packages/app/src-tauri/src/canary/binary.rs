//! A cheap "did the program change?" fingerprint: path, size and modified time of the file an agent
//! starts from. It is compared before a run so an update that landed since the last probe is noticed
//! at once, not up to 15 minutes later. It reads no file contents (nothing is hashed).
//!
//! Limit: a launcher that stays the same file while the real program behind it is replaced (a
//! shim that calls into a package folder) keeps its fingerprint. The 15-minute probe still reads
//! `--version`, so such an update is found there.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// `path|size|mtime_ms` for the file `program` resolves to, `None` when it cannot be found.
pub fn fingerprint(program: &str) -> Option<String> {
    fingerprint_in(program, std::env::var_os("PATH").as_deref())
}

pub fn fingerprint_in(program: &str, path_var: Option<&std::ffi::OsStr>) -> Option<String> {
    let file = resolve(program, path_var)?;
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    Some(format!("{}|{}|{}", file.display(), meta.len(), modified))
}

/// The file `program` names: itself when it has a folder part, else the first match on `PATH`.
fn resolve(program: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let given = Path::new(program);
    if given.components().count() > 1 || given.is_absolute() {
        return given.is_file().then(|| given.to_path_buf());
    }
    std::env::split_paths(path_var?).map(|dir| dir.join(program)).find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-canary-bin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_same_file_has_the_same_fingerprint_and_a_changed_one_does_not() {
        let dir = temp("change");
        let file = dir.join("agent.bin");
        std::fs::write(&file, b"one").unwrap();
        let first = fingerprint(&file.to_string_lossy()).expect("found");
        assert_eq!(fingerprint(&file.to_string_lossy()).as_deref(), Some(first.as_str()));
        assert!(first.contains("agent.bin") && first.contains("|3|"), "{first}");
        std::fs::write(&file, b"longer contents").unwrap();
        let second = fingerprint(&file.to_string_lossy()).expect("found");
        assert_ne!(first, second, "a different size is a different fingerprint");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_modified_time_alone_changes_it() {
        let dir = temp("mtime");
        let file = dir.join("agent.bin");
        std::fs::write(&file, b"same").unwrap();
        let first = fingerprint(&file.to_string_lossy()).unwrap();
        let handle = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
        handle.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(120)).unwrap();
        drop(handle);
        assert_ne!(first, fingerprint(&file.to_string_lossy()).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bare_name_is_found_on_the_path_and_a_missing_one_is_none() {
        let dir = temp("path");
        std::fs::write(dir.join("thing"), b"x").unwrap();
        let path = std::env::join_paths([dir.clone()]).unwrap();
        assert!(fingerprint_in("thing", Some(path.as_os_str())).is_some());
        assert!(fingerprint_in("nothing", Some(path.as_os_str())).is_none());
        assert!(fingerprint_in("thing", None).is_none());
        assert!(fingerprint(&dir.join("missing").to_string_lossy()).is_none());
        assert!(fingerprint(&dir.to_string_lossy()).is_none(), "a folder is not a program");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
