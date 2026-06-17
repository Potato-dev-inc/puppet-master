mod bridge;
mod commands;
mod mcp_install;
mod pty;

use commands::AppState;
use std::path::PathBuf;
use tauri::Manager;

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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Puppet Master");
}
