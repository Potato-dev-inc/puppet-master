//! Stable application data paths (DMG-safe — not tied to process cwd).

use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "com.puppetmaster.app";
/// Run a second, separate Puppet Master on the same computer: `PUPPET_MASTER_PROFILE=<name>`.
/// Unset, empty or `default` is today's app, unchanged (same paths, ports and app id).
pub const PROFILE_ENV: &str = "PUPPET_MASTER_PROFILE";
pub const BRIDGE_PORT_FILE_NAME: &str = "puppet-master.bridge.port";
pub const PAIRING_FILE_NAME: &str = "puppet-master.pairing.json";
pub const BRIDGE_PORT_FILE_ENV: &str = "PUPPET_MASTER_BRIDGE_PORT_FILE";

/// A non-default profile name: `[a-z0-9-]`, 1..=32 chars, lowercased. Anything else (unset, empty,
/// `default`, or a name with other characters) means the default profile, so a typo never lands a
/// second app in a surprising folder.
pub fn profile_from(raw: Option<&str>) -> Option<String> {
    let name = raw?.trim().to_ascii_lowercase();
    let valid = !name.is_empty()
        && name.len() <= 32
        && name != "default"
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-');
    valid.then_some(name)
}

/// The profile this process runs as (`None` = the default profile).
pub fn profile() -> Option<String> {
    static PROFILE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PROFILE
        .get_or_init(|| profile_from(std::env::var(PROFILE_ENV).ok().as_deref()))
        .clone()
}

/// The app id for a profile: today's id for the default profile, `<id>.profile-<name>` otherwise.
/// It names the app data folder, Tauri's own data (settings store, webview data), and the
/// single-instance lock, so each profile is one separate app.
pub fn app_id_for(profile: Option<&str>) -> String {
    match profile {
        None => APP_ID.to_string(),
        Some(name) => format!("{APP_ID}.profile-{name}"),
    }
}

pub fn app_id() -> String {
    app_id_for(profile().as_deref())
}

/// Bridge ports scanned for a free one. The default profile keeps today's range. Other profiles
/// use the top of it, away from the ports the MCP shim probes when it has no port file
/// (17321..=17330), so a stray shim never reaches a profile's bridge by accident.
pub fn bridge_port_range_for(profile: Option<&str>) -> (u16, u16) {
    match profile {
        None => (17321, 17399),
        Some(_) => (17360, 17399),
    }
}

/// The phone (PWA) server port: today's fixed 1420 for the default profile, a free one (0) otherwise.
pub fn pwa_port_for(profile: Option<&str>) -> u16 {
    match profile {
        None => 1420,
        Some(_) => 0,
    }
}

/// Directory of app data for `profile`, rooted at the platform's usual folder.
pub fn app_data_dir_for(profile: Option<&str>) -> PathBuf {
    app_data_root().join(app_id_for(profile))
}

/// Dev-only (`dev-fake-tui`): the end-to-end harness roots app data in its own per-process folder, so
/// test processes running side by side never share an audit log, settings or agent definitions.
#[cfg(feature = "dev-fake-tui")]
static E2E_DATA_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Dev-only (`dev-fake-tui`): root app data at `root`. Must run before anything reads a path; a
/// second call is ignored.
#[cfg(feature = "dev-fake-tui")]
pub fn set_data_root_for_e2e(root: PathBuf) {
    let _ = E2E_DATA_ROOT.set(root);
}

fn app_data_root() -> PathBuf {
    #[cfg(feature = "dev-fake-tui")]
    if let Some(root) = E2E_DATA_ROOT.get() {
        return root.clone();
    }
    let home = crate::project_path::home_dir().unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    {
        return home.join("Library").join("Application Support");
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".local").join("share")
    }
}

pub fn app_data_dir() -> PathBuf {
    app_data_dir_for(profile().as_deref())
}

#[cfg(test)]
fn legacy_app_data_dir() -> PathBuf {
    let home = crate::project_path::home_dir().unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    {
        return home
            .join("Library")
            .join("Application Support")
            .join(APP_ID);
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

/// `<app data>/agents`: the `user` definition source (SRC-2).
#[allow(dead_code)] // the loader wiring (T18) reads it
pub fn agents_dir() -> PathBuf {
    app_data_dir().join("agents")
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
    let mut text = path.as_ref().to_string_lossy().into_owned();
    // canonicalize() on Windows yields \\?\ extended paths that break Node/CLI MCP launchers.
    const WIN_EXT_PREFIX: &str = r"\\?\";
    if text.starts_with(WIN_EXT_PREFIX) {
        text = text[WIN_EXT_PREFIX.len()..].to_string();
    }
    text.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    // PLANTED FAILURE (reverted in the next commit): proves a failing lib test turns its CI shard red.
    #[test]
    fn planted_failure_proves_the_lib_shard_goes_red() {
        assert_eq!(app_id_for(None), "com.puppetmaster.app.planted", "planted: must fail");
    }

    #[test]
    fn app_data_dir_is_absolute_and_named() {
        let dir = app_data_dir();
        assert!(dir.is_absolute());
        assert!(dir.to_string_lossy().contains(APP_ID));
    }

    /// The default profile must be today's app exactly: same app id, same data folder (compared
    /// with a verbatim copy of the old implementation), same port file, same bridge ports, same
    /// phone port.
    #[test]
    fn default_profile_is_unchanged() {
        for raw in [None, Some(""), Some("  "), Some("default"), Some("DEFAULT"), Some("bad name"), Some("-x"), Some("a/b")] {
            assert_eq!(profile_from(raw), None, "{raw:?} must mean the default profile");
        }
        assert_eq!(app_id_for(None), "com.puppetmaster.app");
        assert_eq!(app_data_dir_for(None), legacy_app_data_dir());
        assert_eq!(
            app_data_dir_for(None).join(BRIDGE_PORT_FILE_NAME),
            legacy_app_data_dir().join("puppet-master.bridge.port")
        );
        assert_eq!(bridge_port_range_for(None), (17321, 17399));
        assert_eq!(pwa_port_for(None), 1420);
        // The test process runs without PUPPET_MASTER_PROFILE (cargo test never sets it).
        if std::env::var_os(PROFILE_ENV).is_none() {
            assert_eq!(profile(), None);
            assert_eq!(app_id(), "com.puppetmaster.app");
            assert_eq!(app_data_dir(), legacy_app_data_dir());
            assert_eq!(bridge_port_file(), legacy_app_data_dir().join(BRIDGE_PORT_FILE_NAME));
        }
    }

    #[test]
    fn a_named_profile_is_a_separate_app() {
        assert_eq!(profile_from(Some(" Maya ")), Some("maya".into()));
        assert_eq!(profile_from(Some("leo-2")), Some("leo-2".into()));
        let maya = profile_from(Some("maya"));
        assert_eq!(app_id_for(maya.as_deref()), "com.puppetmaster.app.profile-maya");
        assert_ne!(app_data_dir_for(maya.as_deref()), app_data_dir_for(None));
        assert_ne!(app_data_dir_for(maya.as_deref()), app_data_dir_for(Some("leo")));
        assert_eq!(app_data_dir_for(maya.as_deref()).parent(), app_data_dir_for(None).parent());
        let (lo, hi) = bridge_port_range_for(maya.as_deref());
        assert!(lo > 17330 && hi <= 17399, "outside the MCP shim's fallback probe range");
        assert_eq!(pwa_port_for(maya.as_deref()), 0, "a free port, never the default app's 1420");
    }

    #[test]
    fn path_for_host_config_strips_windows_extended_prefix() {
        let path = path_for_host_config(r"\\?\C:\Users\dev\puppet-master\mcp\index.js");
        assert_eq!(path, "C:/Users/dev/puppet-master/mcp/index.js");
        assert!(!path.contains("?/"));
    }
}
