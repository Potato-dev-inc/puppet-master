//! Tauri commands for user accounts (Settings > Agents > Accounts). Same store as the bridge routes.

use crate::accounts::{self, Account, Store};
use serde_json::Value;

#[tauri::command]
pub fn accounts_list() -> Value {
    accounts::overview_default()
}

#[tauri::command]
pub fn accounts_add(agent_id: String, label: String, home: Option<String>) -> Result<Account, String> {
    Store::default_location().add(&agent_id, &label, home.as_deref())
}

#[tauri::command]
pub fn accounts_rename(id: String, label: String) -> Result<Account, String> {
    Store::default_location().rename(&id, &label)
}

/// Forget an account. Its folder is only deleted with `delete_home` (and only if the app made it).
#[tauri::command]
pub fn accounts_remove(id: String, delete_home: Option<bool>) -> Result<bool, String> {
    Store::default_location().remove(&id, delete_home.unwrap_or(false))
}
