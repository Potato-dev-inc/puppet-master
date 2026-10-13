//! Write secret files (tunnel token, VAPID private key, push subscriptions) so they are never
//! readable by other users, not even briefly.
//!
//! Unix: the file is created with mode 0600 and new directories with 0700 (`OpenOptions::mode`,
//! so there is no window with default permissions). Windows: the app data dir lives under
//! `%APPDATA%`, which is already restricted to the user (and SYSTEM / administrators) by the
//! profile ACL, so no extra ACL is set; the docs say so.

use std::fs;
use std::io::Write;
use std::path::Path;

pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// Write `contents` to `path` atomically (temp file in the same dir, then rename), with
/// restrictive permissions from creation.
pub fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let _ = fs::remove_file(&tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|err| format!("create {}: {err}", tmp.display()))?;
    file.write_all(contents.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|err| format!("write {}: {err}", tmp.display()))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|err| format!("replace {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_atomically_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("pm-secure-{}", std::process::id()));
        let path = dir.join("nested").join("secret.json");
        write_private(&path, "one").unwrap();
        write_private(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!path.with_extension("json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = fs::remove_dir_all(dir);
    }
}
