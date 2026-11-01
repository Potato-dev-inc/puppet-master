//! Installing an adapter or ACP-speaking build that the official registry lists (`acp::registry`).
//!
//! Two kinds, both into one folder per agent under the adapters folder (`<adapters>/reg-<id>/`), both ending with
//! the SHA-256 of the one file that will run (trust on first use), which `acp::pin` checks again before every start:
//!
//! * **npx**: `npm install` of the exact pinned `package@version` with `--ignore-scripts` (no install script of the
//!   package or of anything it pulls in runs), never global. The program that runs is the package's own `bin` script,
//!   started by the app with `node`.
//! * **binary**: the archive is downloaded (https, redirect limit, size cap), its SHA-256 is compared with the one the
//!   registry lists BEFORE anything is unpacked, then it is unpacked into a staging folder with
//!   `acp::install_archive` (no path outside the folder, no links), and the staging folder replaces the old one.
//!
//! The npm program, the network and the machine are behind `install::Runner`, `registry::Http` and `HostEnv`, so the tests
//! start nothing and download nothing. Nothing here names an agent. Registry text is data: it is never run through a
//! shell, and the only things taken from it are an exact package name and version, a download address and the
//! program's relative path.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::config::HostEnv;
use super::install::{move_dir_away, InstallError, Runner};
use super::install_archive;
use super::pin;
use super::overlay::{self, OverlayItem, OverlayStore, RegistryEntry, RegistryKind};
use super::registry::{relative_safe, Choice, Http, RegistryAgent};

/// Largest download.
pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Like `install::NPM_ARGS` but the package's optional dependencies stay in (several agents ship their native
/// program that way). Still no install scripts.
pub const NPM_ARGS: &[&str] = &["install", "--omit=dev", "--no-audit", "--no-fund", "--ignore-scripts", "--no-progress", "--loglevel=http"];

/// What an install produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// `npx` | `binary`.
    pub kind: &'static str,
    pub version: String,
    /// The file that runs, relative to the adapters folder, with `/`.
    pub rel_entry: String,
    /// SHA-256 of that file as installed.
    pub entry_sha256: String,
}

/// `reg-<id>` with anything but letters, digits, `-` and `_` turned into `_`.
pub fn folder_for(registry_id: &str) -> Result<String, InstallError> {
    let clean: String = registry_id.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if clean.is_empty() || clean.len() > 80 {
        return Err(InstallError::NotInstallable("the registry id is not usable as a folder name".into()));
    }
    Ok(format!("reg-{clean}"))
}

/// The name of the marker package a binary folder carries so `pin::check` can read its version.
pub fn marker_name(registry_id: &str) -> String {
    let flat: String = registry_id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    format!("pm-registry-{flat}")
}

fn package_json(name: &str, version: &str, dependency: Option<(&str, &str)>) -> String {
    let mut json = serde_json::json!({
        "name": name,
        "version": version,
        "private": true,
        "description": "Installed by Puppet Master from the official ACP registry. Safe to remove from the app's settings.",
    });
    if let Some((package, exact)) = dependency {
        json["dependencies"] = serde_json::json!({ package: exact });
    }
    let mut text = serde_json::to_string_pretty(&json).unwrap_or_default();
    text.push('\n');
    text
}

fn rel_of(adapters: &Path, path: &Path) -> Result<String, InstallError> {
    let rel = path.strip_prefix(adapters).map_err(|_| InstallError::Unusable("the program is outside the add-on folder".into()))?;
    Ok(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

/// Installs `choice` for the registry entry `registry_id`. `on_line` gets plain progress sentences.
pub fn install(choice: &Choice, registry_id: &str, version: &str, host: &dyn HostEnv, runner: &dyn Runner, http: &dyn Http, on_line: &mut dyn FnMut(&str)) -> Result<Installed, InstallError> {
    let folder = folder_for(registry_id)?;
    let adapters = host.adapters_dir();
    match choice {
        Choice::Npx { package, version, .. } => install_npx(&adapters, &folder, registry_id, package, version, host, runner, on_line),
        Choice::Binary { archive, kind, cmd, sha256, .. } => install_binary(&adapters, &folder, registry_id, version, archive, *kind, cmd, sha256, host, http, on_line),
        Choice::Unsupported(why) => Err(InstallError::NotInstallable(why.clone())),
    }
}

// ---- npx -----------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn install_npx(adapters: &Path, folder: &str, registry_id: &str, package: &str, version: &str, host: &dyn HostEnv, runner: &dyn Runner, on_line: &mut dyn FnMut(&str)) -> Result<Installed, InstallError> {
    let npm = runner.find_npm().ok_or(InstallError::NodeMissing)?;
    let dir = adapters.join(folder);
    move_dir_away(&dir, host)?;
    std::fs::create_dir_all(&dir).map_err(|e| InstallError::Io(format!("{}: {e}", dir.display())))?;
    let manifest = dir.join("package.json");
    crate::settings_patch::write_atomic(&manifest, package_json(&marker_name(registry_id), "0.0.0", Some((package, version))).as_bytes()).map_err(|e| InstallError::Io(format!("{}: {e}", manifest.display())))?;
    on_line(&format!("Downloading {package} {version}..."));
    let args: Vec<String> = NPM_ARGS.iter().map(|a| a.to_string()).collect();
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut seen = |line: &str| {
        let line: String = line.chars().filter(|c| !c.is_control()).take(200).collect();
        if line.trim().is_empty() {
            return;
        }
        tail.push_back(line.clone());
        if tail.len() > 8 {
            tail.pop_front();
        }
        on_line(&line);
    };
    let outcome = runner.run(&npm, &args, &dir, &mut seen);
    let tail_text = tail.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" | ");
    match outcome {
        Err(why) => return Err(InstallError::Npm { code: None, tail: why }),
        Ok(Some(0)) => {}
        Ok(code) => return Err(InstallError::Npm { code, tail: tail_text }),
    }
    on_line("Checking the download...");
    let package_dir = package.split('/').fold(dir.join("node_modules"), |p, part| p.join(part));
    let text = std::fs::read_to_string(package_dir.join("package.json")).map_err(|_| InstallError::Unusable(format!("{package} was not installed")))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|_| InstallError::Unusable("its package.json cannot be read".into()))?;
    if manifest.get("name").and_then(Value::as_str) != Some(package) || manifest.get("version").and_then(Value::as_str) != Some(version) {
        return Err(InstallError::Unusable(format!("npm installed something other than {package} {version}")));
    }
    let bin = bin_script(&manifest, package)?;
    let entry = package_dir.join(&bin);
    // The script must really be inside the package (a link out of it is refused).
    let inside = match (entry.canonicalize(), package_dir.canonicalize()) {
        (Ok(e), Ok(p)) => e.starts_with(p),
        _ => false,
    };
    if !entry.is_file() || !inside {
        return Err(InstallError::Unusable(format!("its program file {bin} is missing")));
    }
    if !runs_with_node(&entry) {
        return Err(InstallError::Unusable("its program is not a Node script, so it cannot be started here".into()));
    }
    let entry_sha256 = pin::sha256_hex(&entry).map_err(|e| InstallError::Io(e.to_string()))?;
    Ok(Installed { kind: "npx", version: version.to_string(), rel_entry: rel_of(adapters, &entry)?, entry_sha256 })
}

/// The package's program file from its `bin`: a string, or the entry named like the package, or the only one.
fn bin_script(manifest: &Value, package: &str) -> Result<String, InstallError> {
    let bin = manifest.get("bin");
    let pick = match bin {
        Some(Value::String(path)) => Some(path.clone()),
        Some(Value::Object(map)) => {
            let short = package.rsplit('/').next().unwrap_or(package);
            map.get(short).or_else(|| (map.len() == 1).then(|| map.values().next()).flatten()).and_then(Value::as_str).map(str::to_string)
        }
        _ => manifest.get("main").and_then(Value::as_str).map(str::to_string),
    };
    let path = pick.ok_or_else(|| InstallError::Unusable("it has no program to start (no single `bin`)".into()))?;
    if !relative_safe(&path) {
        return Err(InstallError::Unusable("its program path is not safe".into()));
    }
    Ok(path.trim_start_matches("./").replace('\\', "/"))
}

fn runs_with_node(entry: &Path) -> bool {
    let ext = entry.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if matches!(ext.as_str(), "js" | "mjs" | "cjs") {
        return true;
    }
    let mut head = [0u8; 64];
    let n = std::fs::File::open(entry).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let line = String::from_utf8_lossy(&head[..n]).to_ascii_lowercase();
    line.starts_with("#!") && line.lines().next().is_some_and(|l| l.contains("node"))
}

// ---- binary ----------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn install_binary(
    adapters: &Path,
    folder: &str,
    registry_id: &str,
    version: &str,
    url: &str,
    kind: super::registry::ArchiveKind,
    cmd: &str,
    sha256: &str,
    host: &dyn HostEnv,
    http: &dyn Http,
    on_line: &mut dyn FnMut(&str),
) -> Result<Installed, InstallError> {
    std::fs::create_dir_all(adapters).map_err(|e| InstallError::Io(format!("{}: {e}", adapters.display())))?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let scratch = adapters.join(format!(".dl-{nonce}"));
    std::fs::create_dir_all(&scratch).map_err(|e| InstallError::Io(format!("{}: {e}", scratch.display())))?;
    let result = (|| {
        let archive = scratch.join("download");
        on_line("Downloading...");
        http.download(url, MAX_DOWNLOAD_BYTES, DOWNLOAD_TIMEOUT, &archive).map_err(InstallError::Download)?;
        on_line("Checking the download...");
        let found = pin::sha256_hex(&archive).map_err(|e| InstallError::Io(e.to_string()))?;
        if !found.eq_ignore_ascii_case(sha256) {
            // Refused before anything is unpacked.
            return Err(InstallError::Checksum { found, expected: sha256.to_ascii_lowercase() });
        }
        on_line("Unpacking...");
        let stage = scratch.join("stage");
        install_archive::extract(kind, &archive, &stage, cmd).map_err(InstallError::Archive)?;
        let rel_cmd = install_archive::safe_entry_path(cmd).map_err(InstallError::Archive)?.ok_or_else(|| InstallError::Unusable("its program has no name".into()))?;
        let program = stage.join(&rel_cmd);
        if !program.is_file() {
            return Err(InstallError::Unusable(format!("the program {cmd} is not in the download")));
        }
        mark_executable(&program);
        let entry_sha256 = pin::sha256_hex(&program).map_err(|e| InstallError::Io(e.to_string()))?;
        // The marker `pin::check` reads the version from (replaces any package.json the archive had at its root).
        let marker = stage.join("package.json");
        crate::settings_patch::write_atomic(&marker, package_json(&marker_name(registry_id), version, None).as_bytes()).map_err(|e| InstallError::Io(format!("{}: {e}", marker.display())))?;
        Ok((stage, rel_cmd, entry_sha256))
    })();
    let (stage, rel_cmd, entry_sha256) = match result {
        Ok(ok) => ok,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(e);
        }
    };
    let finish = || -> Result<Installed, InstallError> {
        let dir = adapters.join(folder);
        move_dir_away(&dir, host)?;
        std::fs::rename(&stage, &dir).map_err(|e| InstallError::Io(format!("{}: {e}", dir.display())))?;
        let rel = rel_cmd.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        Ok(Installed { kind: "binary", version: version.to_string(), rel_entry: format!("{folder}/{rel}"), entry_sha256 })
    };
    let out = finish();
    let _ = std::fs::remove_dir_all(&scratch);
    out
}

/// Only the one program the app will run is marked executable.
fn mark_executable(program: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = program;
}

/// The whole install for one app agent: install `choice` of `entry`, build the overlay entry, check that the result
/// passes the same checks a start will (`registry_config`, then the pin against the file on disk) and only then
/// write it to `store`. Returns the entry written.
#[allow(clippy::too_many_arguments)]
pub fn install_for_agent(
    store: &OverlayStore,
    agent_id: &str,
    label: &str,
    entry: &RegistryAgent,
    choice: &Choice,
    host: &dyn HostEnv,
    runner: &dyn Runner,
    http: &dyn Http,
    on_line: &mut dyn FnMut(&str),
) -> Result<RegistryEntry, InstallError> {
    let installed = install(choice, &entry.id, &entry.version, host, runner, http, on_line)?;
    let (kind, package, archive, args) = match choice {
        Choice::Npx { package, args, .. } => (RegistryKind::Npx, Some(package.clone()), None, args.clone()),
        Choice::Binary { archive, args, .. } => (RegistryKind::Binary, None, Some(archive.clone()), args.clone()),
        Choice::Unsupported(why) => return Err(InstallError::NotInstallable(why.clone())),
    };
    let written = RegistryEntry {
        registry_id: entry.id.clone(),
        version: installed.version.clone(),
        kind,
        package,
        archive,
        installed_entry: installed.rel_entry.clone(),
        entry_sha256: installed.entry_sha256.clone(),
        args,
        env_allow: Vec::new(),
        resume_command: Vec::new(),
    };
    let cfg = overlay::registry_config(agent_id, label, &written).map_err(|why| InstallError::Unusable(why))?;
    let check = pin::check(&cfg.pins, host);
    if !matches!(check, pin::PinCheck::Ok { .. }) {
        return Err(InstallError::Unusable(check.describe()));
    }
    store.put_item(agent_id, &OverlayItem::Registry(written.clone())).map_err(InstallError::Io)?;
    on_line("Done.");
    Ok(written)
}

#[cfg(test)]
mod tests;
