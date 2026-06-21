mod actors;
mod agent_adapters;
mod agent_contexts;
mod app_lifecycle;
mod app_paths;
mod bridge;
mod commands;
mod context_pack;
mod event_log;
mod events;
mod mcp_install;
mod mcp_runtime;
mod mcp_status;
mod mobile_pairing;
mod mobile_tunnel;
mod platform;
mod project_path;
mod projections;
mod pty;
mod pwa_server;
mod session_context;
mod settings_store;
mod shell_env;
pub mod tool_registry;

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
            commands::list_agent_contexts,
            commands::read_agent_context,
            commands::inspect_agent_model,
            commands::replay_pane_timeline,
            commands::get_workspace_state,
            commands::list_tasks,
            commands::list_locks,
            commands::get_coordination_storage_info,
            commands::read_agent_inbox,
            commands::get_audit,
            commands::build_context_pack,
            commands::resize_pane,
            commands::set_project_path,
            commands::get_project_path_cmd,
            commands::ensure_orchestrator_mcp,
            commands::install_npm_mcp_configs,
            commands::install_global_npm_mcp_configs,
            commands::uninstall_npm_mcp_configs,
            commands::uninstall_global_npm_mcp_configs,
            commands::get_mcp_status,
            commands::push_chat_event,
            commands::push_settings_event,
            commands::sync_public_settings,
            commands::create_mobile_pairing_session,
            commands::list_paired_mobile_devices,
            commands::revoke_paired_mobile_device,
            commands::get_mobile_tunnel_info,
            app_lifecycle::get_app_install_info,
            app_lifecycle::open_external_url,
            app_lifecycle::launch_uninstall,
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
            let rust_mcp_name = if cfg!(windows) {
                "bin/puppet-master-mcp.exe"
            } else {
                "bin/puppet-master-mcp"
            };
            if let Ok(resource) = app.path().resolve(rust_mcp_name, BaseDirectory::Resource) {
                if resource.is_file() {
                    mcp_runtime::set_bundled_mcp_binary(resource);
                }
            }
            if mcp_runtime::bundled_mcp_binary().is_none() {
                let binary_name = if cfg!(windows) {
                    "puppet-master-mcp.exe"
                } else {
                    "puppet-master-mcp"
                };
                let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../mcp-server/dist")
                    .join(binary_name);
                if dev.is_file() {
                    mcp_runtime::set_bundled_mcp_binary(dev);
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
            if let Some(binary) = mcp_runtime::bundled_mcp_binary() {
                tracing::info!(path = %binary.display(), "bundled Rust MCP server");
            }

            let port_file = app_paths::bridge_port_file();
            let pairing_file = app_paths::pairing_file();
            if let Err(err) = event_log::init_global_event_log(event_log::event_log_path()) {
                tracing::warn!(%err, "event log not initialized");
            }
            let initial_project_path = {
                let state = app.state::<AppState>();
                pty::registry_get_project_path(&state.registry)
            };
            event_log::set_active_project_path(Some(PathBuf::from(initial_project_path)));

            let registry = app.state::<AppState>().registry.clone();
            let bridge_url = match bridge::start_embedded_bridge(
                registry,
                app.handle().clone(),
                port_file,
                pairing_file,
            ) {
                Ok(handle) => {
                    tracing::info!(url = %handle.url, "embedded bridge started");
                    Some(handle.url)
                }
                Err(err) => {
                    tracing::warn!(%err, "embedded bridge not started — external MCP unavailable");
                    None
                }
            };

            #[cfg(debug_assertions)]
            let _ = &bridge_url;

            #[cfg(not(debug_assertions))]
            {
                if let Some(url) = bridge_url {
                    if let Err(err) = mobile_tunnel::start_mobile_tunnel(app.handle(), url) {
                        tracing::warn!(%err, "mobile tunnel not started");
                    }
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
