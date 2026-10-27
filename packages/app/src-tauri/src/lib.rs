#![recursion_limit = "512"]

mod accounts;
mod actors;
mod agent_authority;
mod agent_run_mode;
mod audit;
mod ambient;
mod agent_definitions_recorder;
mod agent_adapters;
mod agent_contexts;
pub mod agent_runs;
mod agent_health;
mod agent_install;
pub mod agent_hooks;
mod app_lifecycle;
mod app_paths;
mod bridge;
mod budgets;
mod canary;
mod checkpoints;
mod commands;
mod context_pack;
mod event_log;
mod git_exclude;
mod handoff;
mod events;
mod mcp_hints;
mod mcp_install;
mod mcp_runtime;
pub mod mcp_sessions;
mod mcp_status;
mod missions;
mod mobile_pairing;
mod mobile_tunnel;
mod tunnel_config;
mod one_brain;
mod opencode;
pub mod operations;
#[cfg(test)]
mod operations_contract_tests;
mod pane_activity;
mod pane_preview;
mod loop_guard;
mod panic;
mod askbar;
mod attention;
mod second_opinion;
pub mod pane_outbox;
mod pane_prompt;
mod first_run;
mod plain_mode;
mod prompt_risk;
pub mod questions;
mod push;
pub mod redact;
mod flight;
mod secure_file;
mod app_windows;
mod cockpit;
mod ui_mode;
mod pane_windows;
mod tray;
mod pane_wait;
mod pane_wait_notify;
mod platform;
mod project_ir;
pub mod proc_util;
mod path_limit;
mod project_path;
mod projections;
mod pty;
#[cfg(test)]
mod replay;
#[cfg(feature = "dev-fake-tui")]
#[doc(hidden)]
pub mod fake_tui;
mod pwa_server;
#[cfg(test)]
mod tunnel_auth_tests;
#[cfg(test)]
mod device_caps_tests;
mod session_context;
mod session_chat;
mod secrets;
mod settings_store;
mod settings_patch;
mod shell_env;
mod startup_timing;
mod tamper;
pub mod shell_exec;
mod standby;
mod team;
pub mod tool_registry;
mod toolbelt;
mod agent_pack;
mod tui;
mod turn_summary;
mod worker_launch;
pub mod worktrees;
pub mod watch_command;
pub mod env_probe;
mod webview_service_worker;

use commands::AppState;
use std::path::{Path, PathBuf};
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
        // Console as before, plus the last lines kept in memory for the diagnostic file.
        .with_writer(flight::applog::Tee)
        .init();

    // `PUPPET_MASTER_PROFILE=<name>`: a separate app (own identifier, so its own settings store,
    // webview data and single-instance lock). The default profile keeps the configured identifier.
    let mut context = tauri::generate_context!();
    if app_paths::profile().is_some() {
        context.config_mut().identifier = app_paths::app_id();
    }

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
        .manage(commands::SessionChatState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_panes,
            commands::session_chat_open,
            commands::session_chat_close,
            commands::pane_message_send,
            commands::pane_message_cancel,
            commands::pane_messages_list,
            team::commands::team_state,
            team::commands::team_config_get,
            team::commands::team_config_set,
            team::commands::team_send,
            team::commands::team_draft_dismiss,
            team::commands::team_card_dismiss,
            team::commands::team_pull,
            team::commands::team_queue,
            team::commands::team_job_apply,
            team::commands::team_job_discard,
            team::commands::team_job_diff,
            team::commands::team_create,
            team::commands::team_invite,
            team::commands::team_join,
            team::commands::team_leave,
            team::commands::team_join_approve,
            team::commands::team_join_decline,
            team::commands::team_member_remove,
            team::commands::team_gaps_clear,
            team::commands::team_merge_plan,
            team::commands::team_merge_when_green,
            team::commands::team_job_start,
            team::commands::team_member_confirm_github,
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
            commands::askbar_settings_get,
            commands::askbar_settings_set,
            commands::askbar_agents,
            commands::askbar_clipboard,
            commands::askbar_hide,
            commands::askbar_resize,
            commands::askbar_ask,
            commands::askbar_continue,
            commands::askbar_notices,
            commands::askbar_notice_dismiss,
            commands::askbar_notice_open,
            commands::agents_health,
            commands::agents_health_test,
            commands::agents_install_plan,
            commands::agents_install_run,
            commands::agents_install_status,
            commands::agents_install_finish,
            commands::canary_status,
            commands::canary_check,
            commands::canary_set_auto,
            commands::canary_dismiss_notice,
            commands::canary_open_captures,
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
            commands::agents_definition_remove,
            commands::agents_definition_restore,
            commands::agents_definitions_removed,
            commands::agents_definition_purge,
            commands::agents_definitions_purge_all,
            commands::agents_definitions_consent,
            commands::agents_source_set,
            commands::agents_extra_dir,
            commands::agents_project_scan,
            commands::agent_wizard_start,
            commands::agent_wizard_probe,
            commands::agent_wizard_verify,
            commands::agent_wizard_livetest,
            commands::agent_wizard_visibletest,
            commands::agent_wizard_capture,
            commands::agent_wizard_finish,
            commands::agent_wizard_cancel,
            commands::agent_wizard_ask,
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
            commands::toolbelt_list,
            commands::toolbelt_add,
            commands::toolbelt_update,
            commands::toolbelt_remove,
            commands::toolbelt_sync_preview,
            commands::toolbelt_sync_apply,
            commands::toolbelt_import_preview,
            commands::toolbelt_import_apply,
            commands::agent_pack_preview,
            commands::agent_pack_apply,
            commands::agent_pack_undo,
            commands::push_chat_event,
            commands::push_settings_event,
            commands::sync_public_settings,
            commands::create_mobile_pairing_session,
            commands::list_paired_mobile_devices,
            commands::revoke_paired_mobile_device,
            commands::set_paired_mobile_device_caps,
            commands::bridge_local_secret,
            commands::bridge_local_secret_status,
            commands::get_mobile_tunnel_info,
            commands::profile_bridge_url,
            commands::get_tunnel_status,
            commands::set_tunnel_config,
            commands::restart_tunnel,
            commands::test_tunnel_connection,
            commands::diagnostic_preview,
            commands::diagnostic_show,
            commands::diagnostic_save,
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
            commands::pane_set_title,
            commands::ambient_get_prefs,
            commands::ambient_set_animated_tray,
            commands::memory_get,
            commands::memory_save,
            commands::memory_sync,
            commands::memory_set_project_enabled,
            commands::memory_global_enabled,
            commands::memory_set_global_enabled,
            commands::one_brain_remember,
            commands::one_brain_forget,
            commands::one_brain_set_project_enabled,
            commands::open_settings,
            settings_patch::settings_patch,
            commands::pane_prompt,
            commands::answer_pane_prompt,
            commands::attention_snapshot,
            commands::checkpoints_list,
            commands::checkpoint_diff,
            commands::checkpoint_restore,
            commands::checkpoint_undo_last,
            commands::tamper_check,
            commands::mission_list,
            commands::mission_get,
            commands::mission_detect,
            commands::mission_start,
            commands::mission_stop,
            commands::mission_pause,
            commands::mission_resume,
            commands::mission_decide,
            commands::turn_summaries,
            commands::presence_cockpit_focus,
            commands::away_digest_pending,
            commands::away_digest_dismiss,
            commands::interrupt_pane,
            commands::new_pane,
            commands::pane_run_info,
            commands::budget_for_pane,
            commands::budget_adjust,
            commands::pane_worktree,
            commands::pane_git_branch,
            commands::worktree_list,
            commands::worktree_diff,
            commands::second_opinion_agents,
            commands::second_opinion_start,
            commands::second_opinion_poll,
            commands::second_opinion_cancel,
            commands::second_opinion_send,
            commands::worktree_merge,
            commands::worktree_discard,
            commands::plain_check_folder,
            commands::plain_start,
            commands::plain_preview,
            commands::plain_open_preview,
            commands::plain_send,
            commands::first_run_state,
            commands::first_run_start_task,
            commands::first_run_task_status,
            commands::project_recent,
            commands::project_forget,
            commands::project_remember,
            commands::project_pick_folder,
            commands::project_git_state,
            commands::project_git_init_plan,
            commands::project_git_init,
            commands::project_preset_get,
            commands::project_preset_set,
            commands::project_preset_import_conductor,
            commands::worktree_pr_prepare,
            commands::worktree_pr_create,
            commands::worktree_pr_checks,
            commands::worktree_pr_send_back_preview,
            commands::worktree_pr_send_back,
            commands::pane_transcript,
            commands::questions_list,
            commands::question_answer,
            commands::question_dismiss,
            commands::agent_authority_get,
            commands::agent_authority_set,
            commands::agent_run_mode_get,
            commands::agent_run_mode_set,
            commands::audit_recent,
            commands::audit_verify,
        ])
        .setup(|app| {
            // Before the first window: an older version's desktop service worker would serve its UI.
            webview_service_worker::remove_at_startup(app.path().app_local_data_dir().ok());
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
                // A dev build has no installed resource folder: look where the binary is built or
                // staged (the package's dist, the staged `resources/bin`, next to the running exe).
                let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from));
                if let Some(dev) = mcp_runtime::dev_mcp_binary(Path::new(env!("CARGO_MANIFEST_DIR")), exe_dir.as_deref()) {
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

            push::init(app.state::<AppState>().registry.clone());

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
                            let offset = value.get("offset").and_then(|v| v.as_u64());
                            bridge::push_terminal_sse(pane_id, &raw, offset);
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
            ambient::init(app.handle());
            standby::init(registry_for_panes.clone(), app.handle().clone());
            canary::init(app.handle().clone());
            // The Agent Wizard's panes (guided capture, visible test) go through the app's pane registry. The
            // host is installed here, not referenced from the wizard module, so the Tauri window code stays out
            // of the unit-test executables.
            tui::declarative::wizard::visible::host::install(std::sync::Arc::new(
                tui::declarative::wizard::visible::registry_host::RegistryHost::new(registry_for_panes.clone(), app.handle().clone()),
            ));
            pane_activity::init(registry_for_panes.clone(), app.handle().clone());
            loop_guard::init(app.handle().clone());
            panic::init_shortcut(app.handle());
            turn_summary::start_presence();
            attention::init();
            askbar::glue::init(app.handle());
            team::init(app.handle().clone(), registry_for_panes.clone());
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
        .build(context)
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
                if let Some(chat) = app_handle.try_state::<commands::SessionChatState>() {
                    chat.shutdown();
                }
                bridge::port_file::release_owned(&app_paths::bridge_port_file());
                if let Some(state) = app_handle.try_state::<AppState>() {
                    pty::registry_kill_all(&state.registry);
                }
            }
        });
}
