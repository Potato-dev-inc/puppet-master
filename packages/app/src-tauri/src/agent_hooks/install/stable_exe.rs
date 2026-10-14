//! The frozen hook exe (§3.5): `<hooks dir>/v1/puppet-master-mcp(.exe)`, a copy of the shipped MCP
//! exe. Its PATH is what vendor configs name, so the path string is derived once, stored in
//! `install.json` and reused byte for byte. Probe P-C1 showed Codex hashes the hook definition, not
//! the binary, so the copy itself may be refreshed whenever the shipped exe changes.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const DIR: &str = "v1";

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "puppet-master-mcp.exe"
    } else {
        "puppet-master-mcp"
    }
}

/// The stored path string, if a previous run recorded one.
pub fn recorded(hooks_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(hooks_dir.join("install.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("exe").and_then(Value::as_str).map(str::to_string)
}

/// Make sure the frozen copy of `source` exists (refreshing it when `source` changed) and return
/// the exact path string vendor configs must use. `None` when there is no source exe or the copy
/// cannot be made: hooks are then not installed, which is today's behaviour.
pub fn ensure(source: &Path, hooks_dir: &Path) -> Option<String> {
    let dir = hooks_dir.join(DIR);
    let target = dir.join(exe_name());
    std::fs::create_dir_all(&dir).ok()?;
    if needs_copy(source, &target) {
        copy_swap(source, &target).ok()?;
    }
    if let Some(path) = recorded(hooks_dir) {
        if Path::new(&path).is_file() {
            return Some(path);
        }
    }
    let path = crate::app_paths::path_for_host_config(&target);
    let path = if cfg!(windows) { path.replace('/', "\\") } else { path };
    let record = json!({ "exe": path, "protocol": 1 });
    crate::secure_file::write_private(&hooks_dir.join("install.json"), &serde_json::to_string_pretty(&record).ok()?).ok()?;
    Some(path)
}

fn needs_copy(source: &Path, target: &Path) -> bool {
    let (Ok(src), Ok(dst)) = (std::fs::metadata(source), std::fs::metadata(target)) else {
        return true;
    };
    if src.len() != dst.len() {
        return true;
    }
    match (src.modified(), dst.modified()) {
        (Ok(s), Ok(d)) => s > d,
        _ => false,
    }
}

/// Copy beside, then swap in. A running copy (a hook in flight) is renamed aside first: Windows
/// allows renaming an exe that is running, not overwriting it.
fn copy_swap(source: &Path, target: &Path) -> std::io::Result<()> {
    let staged: PathBuf = target.with_extension("new");
    std::fs::copy(source, &staged)?;
    if target.exists() {
        let aside = target.with_extension(format!("old-{}", std::process::id()));
        let _ = std::fs::remove_file(&aside);
        if std::fs::rename(target, &aside).is_err() {
            let _ = std::fs::remove_file(&staged);
            return Ok(()); // keep the old copy; it still works
        }
        let _ = std::fs::remove_file(&aside);
    }
    std::fs::rename(&staged, target)
}
