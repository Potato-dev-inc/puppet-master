mod bridge;
mod commands;
mod mcp_install;
mod pty;
mod settings_store;

use commands::AppState;
use std::path::PathBuf;
use tauri::{Listener, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,puppet_master_app_lib=debug")
            }),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_panes,
            commands::spawn_pane,
            commands::kill_pane_cmd,
            commands::kill_all_panes,
            commands::write_pane_input,
            commands::read_pane_buffer,
            commands::read_pane_snapshot,
            commands::read_pane_raw_buffer,
            commands::resize_pane,
            commands::set_project_path,
            commands::get_project_path_cmd,
            commands::ensure_orchestrator_mcp,
            commands::push_chat_event,
            commands::push_settings_event,
            commands::sync_public_settings,
        ])
        .setup(|app| {
            // Resolve paths. When launched via `tauri dev` cwd is
            // `packages/app/src-tauri`, so we walk three parents to reach
            // the monorepo root.
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let workspace_root: PathBuf = cwd
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or(cwd);
            let port_file = workspace_root.join("puppet-master.bridge.port");

            let registry = app.state::<AppState>().registry.clone();
            match bridge::start_embedded_bridge(registry, app.handle().clone(), port_file) {
                Ok(handle) => {
                    tracing::info!(url = %handle.url, "embedded bridge started");
                }
                Err(err) => {
                    tracing::warn!(%err, "embedded bridge not started — external MCP unavailable");
                }
            }

            app.handle().listen("terminal-data", move |event| {
                let payload = event.payload();
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
                    let pane_id = value.get("pane_id").and_then(|v| v.as_str()).unwrap_or("");
                    let data = value.get("data").and_then(|v| v.as_array());
                    if !pane_id.is_empty() {
                        if let Some(bytes) = data {
                            let raw: Vec<u8> = bytes
                                .iter()
                                .filter_map(|v| v.as_u64().map(|n| n as u8))
                                .collect();
                            bridge::push_terminal_sse(pane_id, &raw);
                        }
                    }
                }
            });

            app.handle().listen("pty://status", |event| {
                let payload = event.payload();
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
                    let pane_id = value.get("pane_id").and_then(|v| v.as_str()).unwrap_or("");
                    let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if !pane_id.is_empty() && !status.is_empty() {
                        bridge::push_pane_status_sse(pane_id, status);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Puppet Master");
}
