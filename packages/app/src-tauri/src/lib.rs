#![recursion_limit = "512"]

mod accounts;
mod actors;
mod agent_definitions_recorder;
mod agent_adapters;
mod agent_contexts;
pub mod agent_runs;
mod agent_health;
mod app_lifecycle;
mod app_paths;
mod bridge;
mod commands;
mod context_pack;
mod event_log;
mod events;
mod mcp_hints;
mod mcp_install;
mod mcp_runtime;
pub mod mcp_sessions;
mod mcp_status;
mod mobile_pairing;
mod mobile_tunnel;
mod opencode;
pub mod operations;
#[cfg(test)]
mod operations_contract_tests;
mod pane_activity;
mod pane_preview;
mod loop_guard;
mod panic;
mod pane_prompt;
mod prompt_risk;
pub mod questions;
mod app_windows;
mod cockpit;
mod ui_mode;
mod pane_windows;
mod tray;
mod pane_wait;
mod pane_wait_notify;
mod platform;
mod project_ir;
mod project_path;
mod projections;
mod pty;
mod pwa_server;
#[cfg(test)]
mod tunnel_auth_tests;
mod session_context;
mod secrets;
mod settings_store;
mod shell_env;
mod startup_timing;
pub mod shell_exec;
mod standby;
pub mod tool_registry;
mod tui;
mod worker_launch;
pub mod worktrees;
pub mod watch_command;
pub mod env_probe;

use commands::AppState;
use std::path::PathBuf;
use tauri::path::BaseDirectory;
use tauri::{Listener, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    worker_launch::init_from_env();
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
        // A second launch opens the running app's Settings window and exits, so it never starts
        // another bridge or rewrites the bridge port file.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Panes-first: a second launch opens the Settings window; `--new-pane <agent>` opens only
            // a pop-out pane (no Settings) through the same code path as the tray's "New pane" menu.
            match tray::second_launch_action(args.as_slice()) {
                tray::SecondLaunch::Settings => cockpit::open_primary_window(app),
                tray::SecondLaunch::NewPane(agent) => {
                    let app = app.clone();
                    std::thread::spawn(move || match tray::new_pane(&app, &agent) {
                        Ok(id) => tracing::info!(%agent, pane = %id, "second launch: opened new pane"),
                        Err(e) => tracing::warn!(%agent, error = %e, "second launch: new pane failed"),
                    });
                }
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
            commands::read_pane_screen_replay,
            commands::agents_list,
            commands::pane_activities,
            commands::pane_previews,
            commands::loop_status,
            commands::loop_signals,
            commands::loop_dismiss,
            commands::loop_settings_get,
            commands::loop_settings_set,
            commands::stop_all_agents,
            commands::agents_health,
            commands::agents_health_test,
            commands::agents_definitions_list,
            commands::agents_definitions_reload,
            commands::agents_definitions_validate,
            commands::agents_definition_approve,
            commands::agents_definition_preview,
            commands::agents_check_file,
            commands::agents_create_fixtures_dir,
            commands::agents_add_generic,
            commands::agents_export_definition,
            commands::agents_import_definition,
            commands::agents_definition_revoke,
            commands::agents_definitions_consent,
            commands::agents_source_set,
            commands::agents_extra_dir,
            commands::agents_project_scan,
            commands::recorder_start,
            commands::recorder_status,
            commands::recorder_stop,
            commands::agents_drafts_list,
            commands::agents_draft_validate,
            commands::list_agent_contexts,
            commands::read_agent_context,
            commands::inspect_agent_model,
            commands::switch_agent_model,
            commands::replay_pane_timeline,
            commands::get_workspace_state,
            commands::list_tasks,
            commands::list_locks,
            commands::get_coordination_storage_info,
            commands::read_agent_inbox,
            commands::get_audit,
            commands::build_context_pack,
            commands::resize_pane,
            commands::reclaim_pane_size,
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
            commands::get_worker_launch,
            commands::exit_app,
            commands::get_opencode_key_status,
            commands::secret_storage_info,
            commands::secret_set,
            commands::secret_get_masked,
            commands::secret_delete,
            commands::secret_get_for_llm,
            commands::confirm_pane_window_close,
            commands::set_opencode_automation_settings,
            commands::set_opencode_key_settings,
            commands::set_opencode_key_profile,
            commands::capture_opencode_key_profile,
            commands::rotate_opencode_key,
            commands::accounts_list,
            commands::accounts_add,
            commands::accounts_rename,
            commands::accounts_remove,
            commands::standby_get_status,
            commands::standby_set_config,
            commands::get_ui_mode,
            commands::set_ui_mode,
            commands::open_cockpit,
            commands::open_settings,
            commands::pane_prompt,
            commands::answer_pane_prompt,
            commands::interrupt_pane,
            commands::new_pane,
            commands::pane_run_info,
            commands::pane_worktree,
            commands::worktree_list,
            commands::worktree_diff,
            commands::worktree_merge,
            commands::worktree_discard,
            commands::pane_transcript,
            commands::questions_list,
            commands::question_answer,
            commands::question_dismiss,
        ])
        .setup(|app| {
            // Register approved declarative agent definitions before anything resolves a profile.
            tui::declarative::loader::reload();
            bridge::start_definitions_watcher(app.handle().clone());
            if let Ok(resource) = app
                .path()
                .resolve("mcp-stdio.bundle.cjs", BaseDirectory::Resource)
            {
                if resource.is_file() {
                    mcp_runtime::set_bundled_mcp_script(resource);
                }
            }
            if let Ok(cli) = app
                .path()
                .resolve("puppet-master-cli.bundle.mjs", BaseDirectory::Resource)
            {
                if let Some(dir) = cli.parent() {
                    crate::watch_command::set_resource_dir(dir.to_path_buf());
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
            pane_windows::init(app.handle(), registry_for_panes.clone());
            secrets::migrate_on_startup();
            settings_store::load_saved_public_settings(app.handle());
            ui_mode::load_saved();
            // Panes-first: no main window. A standalone worker host (`--worker`) is the one
            // exception: it is a user-launched single-terminal window and needs its webview.
            if worker_launch::worker_launch().is_some() {
                app_windows::open_worker_host_window(app.handle());
            }
            if let Err(e) = tray::init(app.handle()) {
                tracing::warn!(error = %e, "tray icon failed to start");
            }
            standby::init(registry_for_panes.clone(), app.handle().clone());
            pane_activity::init(registry_for_panes.clone(), app.handle().clone());
            loop_guard::init(app.handle().clone());
            panic::init_shortcut(app.handle());
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Tray-only: closing the last window (a pane, Settings) must not end the app.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            if let tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } = &event {
                if app_windows::on_close_requested(app_handle, label) {
                    api.prevent_close();
                }
                if pane_windows::on_close_requested(app_handle, label) {
                    api.prevent_close();
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                standby::shutdown();
                bridge::port_file::release_owned(&app_paths::bridge_port_file());
                if let Some(state) = app_handle.try_state::<AppState>() {
                    pty::registry_kill_all(&state.registry);
                }
            }
        });
}
