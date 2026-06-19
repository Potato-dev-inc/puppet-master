mod app_paths;
mod bridge;
mod commands;
mod mcp_install;
mod mcp_runtime;
mod mobile_pairing;
mod platform;
mod project_path;
mod pty;
mod settings_store;
mod shell_env;

use commands::AppState;
use std::path::PathBuf;
use tauri::path::BaseDirectory;
use tauri::{Listener, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    shell_env::apply_to_process();

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
            commands::create_mobile_pairing_session,
            commands::list_paired_mobile_devices,
            commands::revoke_paired_mobile_device,
        ])
        .setup(|app| {
            if let Ok(resource) = app
                .path()
                .resolve("mcp-stdio.bundle.cjs", BaseDirectory::Resource)
            {
                if resource.is_file() {
                    mcp_runtime::set_bundled_mcp_script(resource);
                }
            }
            if mcp_runtime::bundled_mcp_script().is_none() {
                let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../mcp-server/dist/index.js");
                // Prefer the non-canonical path: Windows canonicalize() adds \\?\ and breaks MCP configs.
                if dev.is_file() {
                    mcp_runtime::set_bundled_mcp_script(dev);
                } else if let Ok(canon) = dev.canonicalize() {
                    if canon.is_file() {
                        mcp_runtime::set_bundled_mcp_script(canon);
                    }
                }
            }
            if let Some(script) = mcp_runtime::bundled_mcp_script() {
                tracing::info!(path = %script.display(), "bundled MCP server");
            }

            let port_file = app_paths::bridge_port_file();
            let pairing_file = app_paths::pairing_file();

            let registry = app.state::<AppState>().registry.clone();
            match bridge::start_embedded_bridge(
                registry,
                app.handle().clone(),
                port_file,
                pairing_file,
            ) {
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


            let registry_for_panes = app.state::<AppState>().registry.clone();
            app.handle().listen("pty://panes-changed", move |_event| {
                bridge::push_panes_sse(&registry_for_panes);
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
