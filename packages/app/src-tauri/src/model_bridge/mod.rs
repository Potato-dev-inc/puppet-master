//! Model Bridge (Labs `model_bridge_enabled`, off by default): connect a model service once (a key in the OS
//! credential store), then plug it into an app such as Codex with one click (backup, add a table, Undo).
//!
//! v1 is DIRECT mode: the app talks to the service itself, so only a service that speaks the app's wire API
//! qualifies. A service that needs translating is refused with a plain line (the translator is the next PR).
//! Generic code names no provider and no app: that is all data in `adapters/*.toml` and `recipes/*.toml`.
//!
//! - Keys: pasted once, kept ONLY in the OS credential store. Never on disk, in a log, in argv, or in the webview.
//! - Desktop only: Tauri commands in `commands/model_bridge.rs`. Not a bridge route, not an MCP tool, not a phone
//!   route (see `guard_tests`).
//! - A pane Puppet Master starts for the recipe's agent gets the key in the environment variable the recipe
//!   writes (`launch_env`). Outside Puppet Master the card shows a "set this variable" line without the key.

pub mod adapter;
pub mod recipe;

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod tests;

use crate::secrets::SecretBackend;
use adapter::{Adapter, Shape};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// The Labs switch. Set from the effective settings (`settings_store`), so a development build reads it as on.
pub const LABS_KEY: &str = "model_bridge_enabled";
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn apply_settings(settings: &Value) {
    ENABLED.store(settings.get(LABS_KEY).and_then(Value::as_bool) == Some(true), Ordering::SeqCst);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

pub const OFF: &str = "Model Bridge is off. Turn it on in Settings > General > Labs.";

/// Everything the operations need from the machine, so tests can use temp folders and a memory key store.
pub struct Deps<'a> {
    pub data_dir: PathBuf,
    pub secrets: &'a dyn SecretBackend,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub home: Option<PathBuf>,
    pub windows: bool,
}

impl Deps<'_> {
    fn adapters_dir(&self) -> PathBuf {
        self.data_dir.join("model_bridge").join("adapters")
    }
    fn state_dir(&self) -> PathBuf {
        self.data_dir.join("model_bridge").join("applied")
    }
    fn state_path(&self, recipe: &str, adapter: &str) -> PathBuf {
        self.state_dir().join(format!("{recipe}--{adapter}.json"))
    }
    fn adapters(&self) -> adapter::Loaded {
        adapter::load_all(Some(&self.adapters_dir()))
    }
    fn find_adapter(&self, id: &str) -> Result<Adapter, String> {
        self.adapters().adapters.into_iter().find(|a| a.id == id).ok_or_else(|| "That service is not in the list.".to_string())
    }
    fn config_path(&self, recipe: &recipe::Recipe) -> Result<PathBuf, String> {
        recipe::resolve_path(&recipe.config_path, self.env, self.home.as_deref())
    }
}

/// Real machine: the app data folder, the OS credential store, the process environment.
pub fn with_real<T>(f: impl FnOnce(&Deps) -> T) -> T {
    let env = |name: &str| std::env::var(name).ok();
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let deps = Deps { data_dir: crate::app_paths::app_data_dir(), secrets: crate::secrets::backend(), env: &env, home, windows: cfg!(windows) };
    f(&deps)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedView {
    pub recipe: String,
    pub env_var: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterView {
    pub id: String,
    pub display_name: String,
    pub note: Option<String>,
    pub limits_note: Option<String>,
    pub needs_review: bool,
    /// The blank adapter: the person types a name and an address.
    pub blank: bool,
    pub base_url: String,
    pub shapes: Vec<String>,
    pub key_saved: bool,
    /// Whether Codex can use it directly (a Responses route).
    pub codex_direct: bool,
    pub applied: Option<AppliedView>,
    pub env_line: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    pub enabled: bool,
    pub secure: bool,
    pub storage_label: String,
    pub adapters: Vec<AdapterView>,
    pub problems: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub title: String,
    pub config_path: String,
    pub lines: Vec<String>,
    pub env_line: String,
    pub service: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoneView {
    pub message: String,
    pub backup_path: Option<String>,
}

const CODEX: &str = "codex";

pub fn list(d: &Deps) -> ListView {
    let loaded = d.adapters();
    let recipe = recipe::find(CODEX);
    let adapters = loaded
        .adapters
        .iter()
        .map(|a| {
            let applied = d.state_path(CODEX, &a.id);
            AdapterView {
                id: a.id.clone(),
                display_name: a.display_name.clone(),
                note: a.note.clone(),
                limits_note: a.limits.note.clone(),
                needs_review: a.needs_review,
                blank: a.custom,
                base_url: a.base_url.clone(),
                shapes: a.routes.iter().map(|r| r.shape.as_str().to_string()).collect(),
                key_saved: d.secrets.get(&a.key_slot()).ok().flatten().is_some_and(|k| !k.is_empty()),
                codex_direct: !a.custom && a.route(Shape::Responses).is_some(),
                applied: recipe::load_state(&applied).map(|s| AppliedView { recipe: s.recipe_id, env_var: s.env_var }),
                env_line: recipe::env_line(&a.auth.env_var, d.windows),
            }
        })
        .collect();
    ListView {
        enabled: enabled(),
        secure: d.secrets.available(),
        storage_label: d.secrets.label().to_string(),
        adapters,
        problems: loaded.problems,
    }
}

/// A pasted key, cleaned. The error never repeats the key.
fn clean_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("Paste your key first.".into());
    }
    if key.len() > 4096 || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("That does not look like a key. Paste only the key, with no spaces.".into());
    }
    Ok(key.to_string())
}

/// Saves a key in the credential store. Returns where it is kept, for the line "Key saved in …".
pub fn save_key(d: &Deps, adapter_id: &str, key: &str) -> Result<String, String> {
    let a = d.find_adapter(adapter_id)?;
    if a.custom {
        return Err("Type the name and address first.".into());
    }
    let key = clean_key(key)?;
    if !d.secrets.available() {
        return Err("The secure key store on this computer is not available, so the key was not saved.".into());
    }
    d.secrets.set(&a.key_slot(), &key).map_err(|_| "The key could not be saved in the secure key store.".to_string())?;
    Ok(d.secrets.label().to_string())
}

/// Removes the key. If the service was added to an app, that is undone first, so nothing is left pointing at a key
/// that is gone. A service the person typed in is forgotten too.
pub fn remove_key(d: &Deps, adapter_id: &str) -> Result<(), String> {
    let a = d.find_adapter(adapter_id)?;
    if recipe::load_state(&d.state_path(CODEX, &a.id)).is_some() {
        undo(d, &a.id)?;
    }
    d.secrets.delete(&a.key_slot()).map_err(|_| "The key could not be removed from the secure key store.".to_string())?;
    if a.id.starts_with("custom-") {
        let _ = std::fs::remove_file(d.adapters_dir().join(format!("{}.toml", a.id)));
    }
    Ok(())
}

/// Saves the person's own service (a name and an address, Responses route) in the user folder. Returns its id.
pub fn add_custom(d: &Deps, name: &str, url: &str) -> Result<String, String> {
    let template = d.adapters().adapters.into_iter().find(|a| a.custom).ok_or("The blank service is missing.")?;
    let made = adapter::custom_from(&template, name, url)?;
    if d.adapters().adapters.iter().any(|a| a.id == made.id) {
        return Err("You already have a service with that name.".into());
    }
    let dir = d.adapters_dir();
    std::fs::create_dir_all(&dir).map_err(|_| "Could not save the service.".to_string())?;
    std::fs::write(dir.join(format!("{}.toml", made.id)), adapter::to_toml(&made)?).map_err(|_| "Could not save the service.".to_string())?;
    Ok(made.id)
}

fn plan_for(d: &Deps, adapter_id: &str) -> Result<(Adapter, recipe::Recipe, recipe::Plan), String> {
    let a = d.find_adapter(adapter_id)?;
    let r = recipe::find(CODEX).ok_or("The Codex recipe is missing.")?;
    let plan = recipe::plan(&r, &a, d.config_path(&r)?, None)?;
    Ok((a, r, plan))
}

pub fn preview(d: &Deps, adapter_id: &str) -> Result<PreviewView, String> {
    let (a, _, plan) = plan_for(d, adapter_id)?;
    Ok(PreviewView {
        title: "PM will add a provider to your Codex settings. A backup is kept.".into(),
        config_path: plan.config_path.to_string_lossy().into_owned(),
        lines: recipe::preview_lines(&plan),
        env_line: recipe::env_line(&plan.env_var, d.windows),
        service: a.display_name,
    })
}

/// Adds the provider to the app's settings file. Needs a saved key first.
pub fn apply(d: &Deps, adapter_id: &str) -> Result<DoneView, String> {
    let (a, _, plan) = plan_for(d, adapter_id)?;
    if d.secrets.get(&a.key_slot()).ok().flatten().map_or(true, |k| k.is_empty()) {
        return Err("Save your key first.".into());
    }
    let state = recipe::apply(&plan, &d.state_path(&plan.recipe_id, &a.id))?;
    Ok(DoneView { message: format!("Codex can now use {}.", a.display_name), backup_path: state.backup_path.map(|p| p.to_string_lossy().into_owned()) })
}

pub fn undo(d: &Deps, adapter_id: &str) -> Result<DoneView, String> {
    let path = d.state_path(CODEX, adapter_id);
    let state = recipe::load_state(&path).ok_or("There is nothing to undo.")?;
    let message = recipe::undo(&state, &path)?;
    Ok(DoneView { message, backup_path: None })
}

/// The variables to put in the environment of a pane Puppet Master starts for `agent_id`: the key, under the name
/// the settings file points at. Empty while the switch is off or nothing was added for that agent.
pub fn launch_env_with(d: &Deps, agent_id: &str) -> Vec<(String, String)> {
    if !enabled() {
        return Vec::new();
    }
    let Some(recipe) = recipe::builtin().into_iter().find(|r| r.agent_id == agent_id) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(d.state_dir()) else { return Vec::new() };
    let mut out: Vec<(String, String)> = Vec::new();
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let Some(state) = recipe::load_state(&path) else { continue };
        if state.recipe_id != recipe.id {
            continue;
        }
        let Ok(a) = d.find_adapter(&state.adapter_id) else { continue };
        if let Some(key) = d.secrets.get(&a.key_slot()).ok().flatten().filter(|k| !k.is_empty()) {
            if !out.iter().any(|(n, _)| *n == state.env_var) {
                out.push((state.env_var.clone(), key));
            }
        }
    }
    out
}

/// `launch_env_with` on the real machine. Called by the pane spawn path.
pub fn launch_env(agent_id: &str) -> Vec<(String, String)> {
    if !enabled() {
        return Vec::new();
    }
    with_real(|d| launch_env_with(d, agent_id))
}

