//! Recipes: "plug a model service into an app", as TOML data (`recipes/*.toml`). A recipe names the app's settings
//! file and the tables to write. This generic writer then: shows a preview, BACKS UP the file, adds only
//! Puppet Master's own table(s) with a TOML-preserving editor (every other byte stays), and can undo.
//!
//! No key is ever written here. The settings file gets the NAME of the variable that holds the key.

use super::adapter::{Adapter, Shape};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, Table};

pub const CODEX_RECIPE: &str = include_str!("recipes/codex.toml");

pub const NEEDS_TRANSLATOR: &str = "This service needs the translator (coming next).";

/// One table the recipe writes: where it goes and its fields (in file order, `{field}` filled from the adapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSpec {
    pub table: String,
    pub id_prefix: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub id: String,
    pub display_name: String,
    /// The agent whose panes PM starts with the key in their environment.
    pub agent_id: String,
    pub requires_shape: Shape,
    pub config_path: String,
    pub provider: TableSpec,
    pub profile: Option<TableSpec>,
}

fn str_of(table: &Table, key: &str) -> Result<String, String> {
    table.get(key).and_then(Item::as_str).map(str::to_string).ok_or_else(|| format!("The recipe is missing \"{key}\"."))
}

fn table_spec(doc: &Table, name: &str) -> Result<Option<TableSpec>, String> {
    let Some(item) = doc.get(name) else { return Ok(None) };
    let t = item.as_table().ok_or_else(|| format!("The recipe's [{name}] is not a table."))?;
    let mut fields = Vec::new();
    for (k, v) in t.iter() {
        if k == "table" || k == "id_prefix" {
            continue;
        }
        let s = v.as_str().ok_or_else(|| format!("The recipe field \"{k}\" must be text."))?;
        fields.push((k.to_string(), s.to_string()));
    }
    Ok(Some(TableSpec { table: str_of(t, "table")?, id_prefix: t.get("id_prefix").and_then(Item::as_str).unwrap_or("").to_string(), fields }))
}

pub fn parse_recipe(text: &str) -> Result<Recipe, String> {
    let doc: Document = text.parse().map_err(|e| format!("Could not read the recipe: {e}"))?;
    let root = doc.as_table();
    let shape = match str_of(root, "requires_shape")?.as_str() {
        "responses" => Shape::Responses,
        "chat_completions" => Shape::ChatCompletions,
        "anthropic_messages" => Shape::AnthropicMessages,
        "gemini" => Shape::Gemini,
        other => return Err(format!("The recipe asks for an unknown shape \"{other}\".")),
    };
    let provider = table_spec(root, "provider")?.ok_or("The recipe has no [provider] table.")?;
    Ok(Recipe {
        id: str_of(root, "id")?,
        display_name: str_of(root, "display_name")?,
        agent_id: str_of(root, "agent_id")?,
        requires_shape: shape,
        config_path: str_of(root, "config_path")?,
        provider,
        profile: table_spec(root, "profile")?,
    })
}

/// The embedded recipes.
pub fn builtin() -> Vec<Recipe> {
    [CODEX_RECIPE].iter().filter_map(|t| parse_recipe(t).ok()).collect()
}

pub fn find(id: &str) -> Option<Recipe> {
    builtin().into_iter().find(|r| r.id == id)
}

/// Fills `{env:NAME|fallback}` (a fallback starting `~/` means the home folder). Unset or empty NAME uses the fallback.
pub fn resolve_path(template: &str, env: &dyn Fn(&str) -> Option<String>, home: Option<&Path>) -> Result<PathBuf, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{env:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 5..];
        let end = after.find('}').ok_or("The recipe's file path is not valid.")?;
        let (name, fallback) = after[..end].split_once('|').unwrap_or((&after[..end], ""));
        let value = env(name).filter(|v| !v.trim().is_empty());
        let value = match value {
            Some(v) => v,
            None => match fallback.strip_prefix("~/") {
                Some(tail) => home.ok_or("Could not find your home folder.")?.join(tail).to_string_lossy().into_owned(),
                None => fallback.to_string(),
            },
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    if out.trim().is_empty() {
        return Err("The recipe has no file path.".into());
    }
    Ok(PathBuf::from(out))
}

/// What the adapter fills in. `model` is empty when the service lists none.
#[derive(Debug, Clone)]
pub struct Vars {
    pub display_name: String,
    pub base_url: String,
    pub env_var: String,
    pub provider_id: String,
    pub model: String,
}

fn fill(template: &str, v: &Vars) -> String {
    template
        .replace("{display_name}", &v.display_name)
        .replace("{base_url}", &v.base_url)
        .replace("{env_var}", &v.env_var)
        .replace("{provider_id}", &v.provider_id)
        .replace("{model}", &v.model)
}

/// Everything the writer will do, worked out before anything is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub recipe_id: String,
    pub adapter_id: String,
    pub config_path: PathBuf,
    pub provider_id: String,
    pub env_var: String,
    pub provider_table: String,
    pub provider_fields: Vec<(String, String)>,
    pub profile_table: Option<String>,
    pub profile_fields: Vec<(String, String)>,
}

fn fields_of(spec: &TableSpec, v: &Vars) -> Vec<(String, String)> {
    spec.fields.iter().map(|(k, t)| (k.clone(), fill(t, v))).filter(|(_, val)| !val.trim().is_empty()).collect()
}

/// Works out the writes. Refuses a service whose route is not the one the app speaks.
pub fn plan(recipe: &Recipe, adapter: &Adapter, config_path: PathBuf, model: Option<&str>) -> Result<Plan, String> {
    let route = adapter.route(recipe.requires_shape).ok_or_else(|| NEEDS_TRANSLATOR.to_string())?;
    if recipe.requires_shape != Shape::Responses {
        // v1 writes the direct route only.
        return Err(NEEDS_TRANSLATOR.to_string());
    }
    let base_url = adapter.base_url_for(route).trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return Err("This service has no address yet.".into());
    }
    let provider_id = format!("{}{}", recipe.provider.id_prefix, adapter.id);
    let model = model.map(str::to_string).or_else(|| route.models.first().cloned()).unwrap_or_default();
    let vars = Vars { display_name: adapter.display_name.clone(), base_url, env_var: adapter.auth.env_var.clone(), provider_id: provider_id.clone(), model };
    Ok(Plan {
        recipe_id: recipe.id.clone(),
        adapter_id: adapter.id.clone(),
        config_path,
        provider_id,
        env_var: adapter.auth.env_var.clone(),
        provider_table: recipe.provider.table.clone(),
        provider_fields: fields_of(&recipe.provider, &vars),
        profile_table: recipe.profile.as_ref().map(|p| p.table.clone()),
        profile_fields: recipe.profile.as_ref().map(|p| fields_of(p, &vars)).unwrap_or_default(),
    })
}

/// The exact lines that will be added, as a person would read them.
pub fn preview_lines(plan: &Plan) -> Vec<String> {
    let mut lines = vec![format!("[{}.{}]", plan.provider_table, plan.provider_id)];
    lines.extend(plan.provider_fields.iter().map(|(k, v)| format!("{k} = {}", quote(v))));
    if let Some(table) = &plan.profile_table {
        lines.push(String::new());
        lines.push(format!("[{table}.{}]", plan.provider_id));
        lines.extend(plan.profile_fields.iter().map(|(k, v)| format!("{k} = {}", quote(v))));
    }
    lines
}

fn quote(v: &str) -> String {
    toml_edit::value(v).to_string().trim().to_string()
}

/// A line to copy for an app Puppet Master did not start. It never holds the key.
pub fn env_line(var: &str, windows: bool) -> String {
    if windows {
        format!("$env:{var} = \"<paste your key here>\"")
    } else {
        format!("export {var}=\"<paste your key here>\"")
    }
}

/// What is remembered so the change can be undone. No secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub recipe_id: String,
    pub adapter_id: String,
    pub config_path: PathBuf,
    pub existed_before: bool,
    pub backup_path: Option<PathBuf>,
    pub written_sha256: String,
    pub provider_table: String,
    pub profile_table: Option<String>,
    pub provider_id: String,
    pub env_var: String,
}

pub fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn read_config(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| "Your settings file is not plain text, so nothing was changed.".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Your settings file could not be read, so nothing was changed.".into()),
    }
}

const UNREADABLE: &str = "Your settings file could not be understood, so nothing was changed.";

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| "Could not make the settings folder.".to_string())?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".pm-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|_| "Could not write the settings file.".to_string())?;
    std::fs::rename(&tmp, path).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        "Could not write the settings file.".to_string()
    })
}


fn table_item(fields: &[(String, String)]) -> Table {
    let mut t = Table::new();
    for (k, v) in fields {
        t.insert(k, toml_edit::value(v.as_str()));
    }
    t
}

/// The largest table position in the document, so a new table sorts after every table already there.
fn last_position(table: &Table) -> usize {
    let mut max = table.position().unwrap_or(0);
    for (_, item) in table.iter() {
        match item {
            Item::Table(t) => max = max.max(last_position(t)),
            Item::ArrayOfTables(a) => {
                for t in a.iter() {
                    max = max.max(last_position(t));
                }
            }
            _ => {}
        }
    }
    max
}

/// Adds (or replaces) `[parent.child]` at the END of the file, after a blank line. A table that is already there
/// keeps its place and spacing. Nothing else in the document is touched.
fn set_child(doc: &mut Document, parent: &str, child: &str, fields: &[(String, String)], blank_before: bool) -> Result<(), String> {
    let end = last_position(doc.as_table()) + 1;
    let root = doc.as_table_mut();
    if !root.contains_key(parent) {
        let mut t = Table::new();
        t.set_implicit(true);
        root.insert(parent, Item::Table(t));
    }
    let p = root.get_mut(parent).and_then(Item::as_table_mut).ok_or_else(|| format!("\"{parent}\" in your settings file is not a table, so nothing was changed."))?;
    let mut made = table_item(fields);
    match p.get(child).and_then(Item::as_table) {
        Some(old) => {
            if let Some(pos) = old.position() {
                made.set_position(pos);
            }
            if let Some(prefix) = old.decor().prefix() {
                made.decor_mut().set_prefix(prefix.clone());
            }
        }
        None => {
            made.set_position(end);
            if blank_before {
                made.decor_mut().set_prefix("\n");
            }
        }
    }
    p.insert(child, Item::Table(made));
    Ok(())
}

fn remove_child(doc: &mut Document, parent: &str, child: &str) {
    if let Some(p) = doc.as_table_mut().get_mut(parent).and_then(Item::as_table_mut) {
        p.remove(child);
        if p.is_empty() && p.is_implicit() {
            doc.as_table_mut().remove(parent);
        }
    }
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn backup_name(path: &Path) -> PathBuf {
    let mut n = path.file_name().map(|f| f.to_os_string()).unwrap_or_default();
    n.push(format!(".pm-backup-{}", stamp()));
    path.with_file_name(n)
}

/// The settings with our tables removed, as plain values: used to prove nothing else changed.
fn without_ours(text: &str, parents: &[(&str, &str)]) -> Result<toml::Value, String> {
    let mut value: toml::Value = toml::from_str(text).map_err(|_| UNREADABLE.to_string())?;
    for (parent, child) in parents {
        let mut drop_parent = false;
        if let Some(t) = value.get_mut(*parent).and_then(toml::Value::as_table_mut) {
            t.remove(*child);
            drop_parent = t.is_empty();
        }
        if drop_parent {
            if let Some(root) = value.as_table_mut() {
                root.remove(*parent);
            }
        }
    }
    Ok(value)
}

fn ours<'a>(plan: &'a Plan) -> Vec<(&'a str, &'a str)> {
    let mut v = vec![(plan.provider_table.as_str(), plan.provider_id.as_str())];
    if let Some(t) = &plan.profile_table {
        v.push((t.as_str(), plan.provider_id.as_str()));
    }
    v
}

/// Backs up the file, adds our table(s), saves what is needed to undo. `state_path` holds that note.
pub fn apply(plan: &Plan, state_path: &Path) -> Result<State, String> {
    let original = read_config(&plan.config_path)?;
    let mut text = original.clone().unwrap_or_default();
    // A last line with no line break would run into the new table header.
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let mut doc: Document = text.parse().map_err(|_| UNREADABLE.to_string())?;
    set_child(&mut doc, &plan.provider_table, &plan.provider_id, &plan.provider_fields, !text.is_empty())?;
    if let Some(table) = &plan.profile_table {
        set_child(&mut doc, table, &plan.provider_id, &plan.profile_fields, !text.is_empty())?;
    }
    let new_text = doc.to_string();

    // Proof before writing: the new file reads, and without our tables it means what the old one did.
    let parents = ours(plan);
    let before = without_ours(original.as_deref().unwrap_or(""), &parents)?;
    let after = without_ours(&new_text, &parents)?;
    if before != after {
        return Err("Something unexpected came up, so nothing was changed.".into());
    }

    let backup_path = if original.is_some() {
        let mut target = backup_name(&plan.config_path);
        let mut n = 1;
        while target.exists() {
            let mut name = backup_name(&plan.config_path).into_os_string();
            name.push(format!("-{n}"));
            target = PathBuf::from(name);
            n += 1;
        }
        std::fs::copy(&plan.config_path, &target).map_err(|_| "Could not make a backup, so nothing was changed.".to_string())?;
        if std::fs::read(&target).ok().as_deref() != std::fs::read(&plan.config_path).ok().as_deref() {
            return Err("The backup did not match, so nothing was changed.".into());
        }
        Some(target)
    } else {
        None
    };

    write_atomic(&plan.config_path, new_text.as_bytes())?;
    let state = State {
        recipe_id: plan.recipe_id.clone(),
        adapter_id: plan.adapter_id.clone(),
        config_path: plan.config_path.clone(),
        existed_before: original.is_some(),
        backup_path,
        written_sha256: sha_hex(new_text.as_bytes()),
        provider_table: plan.provider_table.clone(),
        profile_table: plan.profile_table.clone(),
        provider_id: plan.provider_id.clone(),
        env_var: plan.env_var.clone(),
    };
    save_state(state_path, &state)?;
    Ok(state)
}

pub fn save_state(path: &Path, state: &State) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    write_atomic(path, &json)
}

pub fn load_state(path: &Path) -> Option<State> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Puts the settings file back. If nobody touched it since, the backup is restored exactly. If it changed, only
/// our table(s) are removed. The note is deleted either way; the backup copy is kept.
pub fn undo(state: &State, state_path: &Path) -> Result<String, String> {
    let current = read_config(&state.config_path)?;
    let untouched = current.as_deref().map(|t| sha_hex(t.as_bytes())) == Some(state.written_sha256.clone());
    let message = if untouched || current.is_none() {
        match (&state.backup_path, state.existed_before) {
            (Some(backup), true) => {
                let bytes = std::fs::read(backup).map_err(|_| "The backup copy could not be found, so nothing was changed.".to_string())?;
                write_atomic(&state.config_path, &bytes)?;
                "Your Codex settings are back exactly as they were.".to_string()
            }
            (_, false) if untouched => {
                std::fs::remove_file(&state.config_path).map_err(|_| "Could not remove the settings file.".to_string())?;
                "Your Codex settings are back as they were (there was no file before).".to_string()
            }
            _ => "Nothing to undo.".to_string(),
        }
    } else {
        let mut doc: Document = current.unwrap_or_default().parse().map_err(|_| UNREADABLE.to_string())?;
        remove_child(&mut doc, &state.provider_table, &state.provider_id);
        if let Some(t) = &state.profile_table {
            remove_child(&mut doc, t, &state.provider_id);
        }
        write_atomic(&state.config_path, doc.to_string().as_bytes())?;
        "Removed what Puppet Master added. The file had changed since, so the rest was left alone.".to_string()
    };
    let _ = std::fs::remove_file(state_path);
    Ok(message)
}
