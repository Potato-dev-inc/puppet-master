//! Tauri commands of the Potato Bot sandbox settings page and setup wizard. Desktop only, like the other Potato commands:
//! nothing here is reachable through the bridge, the phone or MCP. Every command answers `POTATO_SANDBOX_OFF` while the Labs
//! switch is off.
//!
//! Errors are one string, `CODE: plain sentence`. The install runs on its own thread: `potato_sandbox_install` returns at once
//! and the page follows the `potato://sandbox-install` event (payload `{ install }`); `potato://sandbox-changed` says to read
//! the status again.
//!
//! This file holds the one `Consent` for Windows' Linux feature (`potato_sandbox_linux_enable`). It is made only after the
//! manager accepted the token of a confirm dialog that `potato_sandbox_linux_ask` opened, so the feature can never be turned on
//! by an install, a start, a bot, or a call that skipped the dialog.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::potato::sandbox::detect::{self, Detected};
use crate::potato::sandbox::manager::{FeatureAsk, Manager, RecipeView, SandboxView, WizardAnswers};
use crate::potato::sandbox::wsl::{Consent, Wsl};
use crate::potato::sandbox::SandboxError;
use crate::potato::sandbox_host;

/// How many recent projects are looked at for languages.
const PROJECT_LIMIT: usize = 40;

fn text(error: SandboxError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// Runs one manager call off the UI thread (a start, a reset or a remove can take a minute).
async fn run<T: Send + 'static>(call: impl FnOnce(Arc<Manager>) -> Result<T, SandboxError> + Send + 'static) -> Result<T, String> {
    let manager = sandbox_host::manager()?;
    tauri::async_runtime::spawn_blocking(move || call(manager).map_err(text)).await.map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn potato_sandbox_status() -> Result<SandboxView, String> {
    run(|m| m.status()).await
}

/// The recipes the wizard offers (titles, descriptions and options, read from the recipe files).
#[tauri::command]
pub async fn potato_sandbox_menu() -> Result<Vec<RecipeView>, String> {
    run(|m| m.menu()).await
}

/// Languages the person's recent projects use (marker files like `package.json` or `Cargo.toml`), for the wizard's
/// pre-checked boxes. Names of folders are looked at; no file is opened.
#[tauri::command]
pub async fn potato_sandbox_detect(state: State<'_, AppState>) -> Result<Detected, String> {
    sandbox_host::manager()?;
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || detect::languages_in(&project_paths(&registry)))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))
}

/// The project folders the app already knows (the same list the bot's "Change folder" starts from).
fn project_paths(registry: &Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>) -> Vec<PathBuf> {
    let mut others = crate::operations::indexed_project_paths();
    others.push(crate::standby::default_project(registry));
    others.truncate(PROJECT_LIMIT);
    crate::worktrees::folder::recent_light(&crate::app_paths::app_data_dir(), &others, PROJECT_LIMIT)
        .into_iter()
        .filter(|p| p.state != "missing")
        .map(|p| PathBuf::from(p.path))
        .collect()
}

/// Starts set up. The answers are checked first (a bad one is an error at once); then it runs on its own thread.
#[tauri::command]
pub async fn potato_sandbox_install(answers: WizardAnswers) -> Result<(), String> {
    run(move |m| m.start_install(&answers)).await
}

#[tauri::command]
pub async fn potato_sandbox_cancel_install() -> Result<(), String> {
    run(|m| m.cancel_install()).await
}

/// The page showed the end of a set up (done, failed, stopped): forget it.
#[tauri::command]
pub async fn potato_sandbox_dismiss_install() -> Result<(), String> {
    run(|m| {
        m.dismiss_install();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn potato_sandbox_start() -> Result<(), String> {
    run(|m| m.start()).await
}

#[tauri::command]
pub async fn potato_sandbox_stop() -> Result<(), String> {
    run(|m| m.stop()).await
}

/// Back to the reset point saved at the end of set up.
#[tauri::command]
pub async fn potato_sandbox_reset() -> Result<(), String> {
    run(|m| m.reset()).await
}

/// Removes the sandbox. `typed` must be the word `remove`.
#[tauri::command]
pub async fn potato_sandbox_remove(typed: String) -> Result<(), String> {
    run(move |m| m.remove(&typed)).await
}

/// The idle-stop minutes (1 to 240). Returns the number that is used and keeps it for the next run of the app.
#[tauri::command]
pub async fn potato_sandbox_set_idle(minutes: u32) -> Result<u32, String> {
    run(move |m| {
        let used = m.set_idle_minutes(minutes);
        sandbox_host::save_idle_minutes(used);
        Ok(used)
    })
    .await
}

/// Step one of "Turn on Windows' Linux feature": the text of the confirm dialog and a token for its button.
#[tauri::command]
pub async fn potato_sandbox_linux_ask() -> Result<FeatureAsk, String> {
    run(|m| Ok(m.ask_linux_feature())).await
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureOutcome {
    /// Windows asked for a restart.
    pub restart_needed: bool,
}

/// Step two: the person pressed the dialog's button. Windows shows its own administrator prompt.
#[tauri::command]
pub async fn potato_sandbox_linux_enable(token: String) -> Result<FeatureOutcome, String> {
    let manager = sandbox_host::manager()?;
    tauri::async_runtime::spawn_blocking(move || {
        let wsl = Wsl::live().map_err(|_| "SANDBOX_NEEDS_WINDOWS: This computer cannot turn on Windows' Linux feature from here.".to_string())?;
        enable_linux_feature(&manager, &wsl, &token)
    })
    .await
    .map_err(|e| format!("INTERNAL: {e}"))?
}

/// The only place a `Consent` is made. It needs the token of an open confirm dialog; without one nothing runs.
pub fn enable_linux_feature(manager: &Manager, wsl: &Wsl, token: &str) -> Result<FeatureOutcome, String> {
    manager.take_linux_feature_ask(token).map_err(text)?;
    match wsl.enable_windows_feature(Consent::the_person_pressed_the_button()) {
        Ok(result) => Ok(FeatureOutcome { restart_needed: result.restart_needed }),
        Err(_) => Err("SANDBOX_FEATURE_FAILED: Windows' Linux feature was not turned on. Was the administrator prompt closed? Nothing was changed.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::potato::sandbox::fakes_tests::FakeGuest;
    use crate::potato::sandbox::manager::NoEvents;
    use crate::potato::sandbox::wsl::SandboxName;
    use crate::potato::sandbox::FakeSandbox;

    fn manager() -> Arc<Manager> {
        Manager::new(Arc::new(FakeSandbox::not_installed()), SandboxName::main(), None, None, Arc::new(NoEvents))
    }

    #[test]
    fn the_feature_is_not_turned_on_without_the_token_of_an_open_dialog() {
        let guest = FakeGuest::new();
        let wsl = guest.wsl();
        let manager = manager();
        // no dialog was opened
        let error = enable_linux_feature(&manager, &wsl, "anything").unwrap_err();
        assert!(error.starts_with("SANDBOX_NOT_CONFIRMED"), "{error}");
        assert!(guest.calls().is_empty(), "nothing reached wsl.exe");
        // a dialog was opened, but the call brings another token
        let ask = manager.ask_linux_feature();
        assert!(enable_linux_feature(&manager, &wsl, "not-the-token").unwrap_err().starts_with("SANDBOX_NOT_CONFIRMED"));
        assert!(guest.calls().is_empty());
        // the wrong try used the dialog up: the right token no longer works
        assert!(enable_linux_feature(&manager, &wsl, &ask.token).unwrap_err().starts_with("SANDBOX_NOT_CONFIRMED"));
        assert!(guest.calls().is_empty());
    }

    #[test]
    fn the_button_of_the_dialog_turns_it_on_once_and_asks_for_a_restart() {
        let guest = FakeGuest::new();
        let wsl = guest.wsl();
        let manager = manager();
        let ask = manager.ask_linux_feature();
        assert!(ask.body.contains("administrator") && ask.body.contains("restart"));
        let outcome = enable_linux_feature(&manager, &wsl, &ask.token).unwrap();
        assert!(outcome.restart_needed, "the fake guest answers with the restart code");
        assert_eq!(guest.st.lock().elevated_seen, 1);
        // the same token is spent
        assert!(enable_linux_feature(&manager, &wsl, &ask.token).is_err());
        assert_eq!(guest.st.lock().elevated_seen, 1);
    }

    #[test]
    fn the_commands_answer_off_while_the_switch_is_off() {
        // (nothing in a unit test turns the Labs switch on)
        let error = tauri::async_runtime::block_on(potato_sandbox_status()).unwrap_err();
        assert!(error.starts_with("POTATO_SANDBOX_OFF"), "{error}");
        let error = tauri::async_runtime::block_on(potato_sandbox_remove("remove".into())).unwrap_err();
        assert!(error.starts_with("POTATO_SANDBOX_OFF"), "{error}");
        let error = tauri::async_runtime::block_on(potato_sandbox_linux_enable("x".into())).unwrap_err();
        assert!(error.starts_with("POTATO_SANDBOX_OFF"), "{error}");
    }
}
