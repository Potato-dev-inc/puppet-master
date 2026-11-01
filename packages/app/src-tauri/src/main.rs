#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `acp-bridge --agent <id>`: speak ACP on stdio for an agent without ACP; no window, no app.
    if let Some(code) = puppet_master_app_lib::run_cli_subcommand() {
        std::process::exit(code);
    }
    puppet_master_app_lib::run();
}
