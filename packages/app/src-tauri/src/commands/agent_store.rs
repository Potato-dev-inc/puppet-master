//! Tauri commands of the Agent Store (SPEC-n32 3.5), wrapping `crate::agent_store`. Tauri commands are
//! the webview's own channel: there is no bridge route and no MCP tool for any of them, so an agent, the
//! companion or a phone can never add or install an agent (`store_commands_are_desktop_only` checks it).
//! Each runs on the blocking pool: a card's version comes from `check_probe` (a program start, up to 4 s per
//! agent) and Add, Remove and the install records read and write files.

use super::*;

/// Runs `work` on the blocking pool and flattens its result.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("STORE_INTERNAL: {e}"))?
}

/// Every shipped agent as a card: `{catalog_version, entries}`.
#[tauri::command]
pub async fn agent_store_list() -> Result<Value, String> {
    blocking(crate::agent_store::list).await
}

/// One card plus its install plan, run command, fixtures and uninstall summary.
#[tauri::command]
pub async fn agent_store_detail(id: String) -> Result<Value, String> {
    blocking(move || crate::agent_store::detail(&id)).await
}

/// Adds the shipped agent to the person's list. `confirmed_found` is the person's yes to "is this the program?".
#[tauri::command]
pub async fn agent_store_add(id: String, confirmed_found: bool) -> Result<Value, String> {
    blocking(move || crate::agent_store::add(&id, confirmed_found)).await
}

/// Removes the Store's own file for this agent (never a file the person made).
#[tauri::command]
pub async fn agent_store_remove(id: String) -> Result<Value, String> {
    blocking(move || crate::agent_store::remove(&id)).await
}

/// After an install step finished: writes how the program was installed, from a fresh plan.
#[tauri::command]
pub async fn agent_store_record_install(id: String, step_index: usize) -> Result<Value, String> {
    blocking(move || crate::agent_store::record_install(&id, step_index)).await
}

/// The removal the Store would run for a program it installed. Runs nothing.
#[tauri::command]
pub async fn agent_store_uninstall_plan(id: String) -> Result<Value, String> {
    blocking(move || crate::agent_store::uninstall::plan(&id)).await
}

/// Runs the confirmed removal step in a visible PowerShell pane. `shown_command` must equal the step's command
/// in a freshly built plan. A removal is never a script, so it never needs the high-risk confirmation.
#[tauri::command]
pub async fn agent_store_uninstall_run(
    id: String,
    step: usize,
    shown_command: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    let registry = state.registry.clone();
    blocking(move || crate::agent_store::uninstall::run(&app, &registry, &id, step, &shown_command, false)).await
}

/// The shipped definition text and fixture files of one entry, for the Check run. Reads the embedded catalog only.
#[tauri::command]
pub async fn agent_store_check_source(id: String) -> Result<Value, String> {
    blocking(move || crate::agent_store::check_source(&id)).await
}

/// Keeps the numbers of one "Check it works" run (passed, total, version) and nothing else.
#[tauri::command]
pub async fn agent_store_record_check(id: String, passed: u32, total: u32, version: String) -> Result<Value, String> {
    blocking(move || crate::agent_store::record_check(&id, passed, total, &version)).await
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for item in read.flatten() {
            let path = item.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Store actions are desktop-only: no bridge route, no MCP tool and no tool registry file may name
    /// the Store module or one of its commands.
    #[test]
    fn store_commands_are_desktop_only() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        for dir in ["bridge", "tool_registry", "mcp_shim"] {
            rust_files(&src.join(dir), &mut files);
        }
        for dir in ["bridge.rs", "tool_registry.rs", "mcp_shim.rs"] {
            if src.join(dir).is_file() {
                files.push(src.join(dir));
            }
        }
        assert!(!files.is_empty(), "no bridge, tool_registry or mcp_shim file was found: the scan is broken");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            assert!(!text.contains("agent_store"), "{} mentions agent_store", file.display());
        }
    }
}
