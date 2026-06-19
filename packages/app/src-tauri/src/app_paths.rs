//! Stable application data paths (DMG-safe — not tied to process cwd).

use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "com.puppetmaster.app";
pub const BRIDGE_PORT_FILE_NAME: &str = "puppet-master.bridge.port";
pub const PAIRING_FILE_NAME: &str = "puppet-master.pairing.json";
pub const BRIDGE_PORT_FILE_ENV: &str = "PUPPET_MASTER_BRIDGE_PORT_FILE";

pub fn app_data_dir() -> PathBuf {
    let home = crate::project_path::home_dir().unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    {
        return home.join("Library").join("Application Support").join(APP_ID);
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join(APP_ID);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".local").join("share").join(APP_ID)
    }
}

pub fn ensure_app_data_dir() -> Result<PathBuf, String> {
    let dir = app_data_dir();
    fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    Ok(dir)
}

pub fn bridge_port_file() -> PathBuf {
    app_data_dir().join(BRIDGE_PORT_FILE_NAME)
}

pub fn pairing_file() -> PathBuf {
    app_data_dir().join(PAIRING_FILE_NAME)
}

pub fn bridge_port_file_env_value() -> String {
    path_for_host_config(bridge_port_file())
}

/// Host config files (Codex TOML, JSON) — forward slashes avoid TOML escape issues on Windows.
pub fn path_for_host_config(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_dir_is_absolute_and_named() {
        let dir = app_data_dir();
        assert!(dir.is_absolute());
        assert!(dir.to_string_lossy().contains(APP_ID));
    }
}
