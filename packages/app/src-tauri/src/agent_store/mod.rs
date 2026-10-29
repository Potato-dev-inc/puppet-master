//! Agent Store: a catalog of terminal AI agents, one folder per agent, embedded in the app.
//!
//! The Store is data plus checks. Nothing here types into a pane, reads a credential or makes a
//! network request of its own. `entry` parses and checks the card data (`entry.toml`).
//!
//! This file is the JSON api the `agent_store_*` Tauri commands call (SPEC-n32 3.5). Each public
//! function returns the JSON the UI reads, or a `CODE: sentence` error. The `*_in` variants take the
//! catalog, the record file and the version probe as arguments, so tests never touch the app data
//! folder or run a program.

pub mod add;
// `problems`, `embedded_definition` and `detect` are used by the tests and by later tasks.
#[allow(dead_code)]
pub mod catalog;
// The catalog check runs as a test and in CI only; the app never calls it.
#[allow(dead_code)]
pub mod check;
pub mod embedded;
pub mod entry;
pub mod record;
#[allow(dead_code)]
pub mod state;
pub mod uninstall;

#[cfg(test)]
mod tests;

use std::path::{Path, MAIN_SEPARATOR};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use crate::agent_health::{self, check_probe, VERSION_TIMEOUT};
use crate::agent_install::plan::{Action, Method, Plan, Step, Tools};
use crate::agent_install::{self, plan_view};
use crate::proc_util::TtlCache;
use crate::tui::declarative::loader;
use crate::tui::declarative::parse_definition;
use crate::tui::declarative::schema::{Definition, Kind};
use crate::tui::declarative::service::tui_argv_preview;
use catalog::CatalogEntry;
use record::{AgentRecord, CheckRecord, InstallRecord, StoreRecord};
use state::Inputs;

/// How long a found program's version stays on the cards (SPEC-n32 3.4).
const VERSION_TTL: Duration = Duration::from_secs(60);

/// What the cards read from this PC. The app passes the real record file and probe; tests pass a
/// temp file and a fake probe.
pub struct Env<'a> {
    /// The Store record file (`agent-store.json`).
    pub record_path: &'a Path,
    /// The installed version of a program the Store found, or `None` (not found, or no answer).
    pub version: &'a dyn Fn(&CatalogEntry, &Inputs) -> Option<String>,
}

/// `agent_store_list`: one card per shipped entry, in catalog order.
pub fn list() -> Result<Value, String> {
    let record_path = record::default_path();
    let env = Env { record_path: &record_path, version: &live_version };
    list_in(catalog::all(), &catalog::catalog_version(), &env)
}

/// `list` over any catalog and environment.
pub fn list_in(entries: &[CatalogEntry], catalog8: &str, env: &Env<'_>) -> Result<Value, String> {
    let cards: Vec<Value> = entries.iter().map(|entry| card_for(entry, env)).collect();
    Ok(json!({ "catalog_version": catalog8, "entries": cards }))
}

/// `agent_store_detail`: the card, plus the install plan, the run command, the fixtures and the
/// uninstall summary.
pub fn detail(id: &str) -> Result<Value, String> {
    let entry = shipped(id)?;
    let tools = Tools::detect();
    let (plan, hint) = agent_install::plan_for(id, &tools)?;
    let record_path = record::default_path();
    let env = Env { record_path: &record_path, version: &live_version };
    detail_in(entry, &env, &tools, &plan, hint.as_deref())
}

/// `detail` for one entry, with its install plan already built.
pub fn detail_in(
    entry: &CatalogEntry,
    env: &Env<'_>,
    tools: &Tools,
    plan: &Plan,
    hint: Option<&str>,
) -> Result<Value, String> {
    let inputs = state::detect_at(entry, env.record_path);
    let installed = (env.version)(entry, &inputs);
    let mut card = card_value(entry, &inputs, installed);
    let install = record::load(env.record_path)
        .agents
        .remove(&entry.entry.store.id)
        .and_then(|agent| agent.install);
    let spec = catalog::install_spec_of(entry);
    let removal = uninstall::uninstall_plan(entry, spec.as_ref(), install.as_ref(), tools);
    let removal = plan_view(&removal, None);
    let extra = json!({
        "install_plan": plan_view(plan, hint),
        "run_preview": run_preview(entry),
        "fixtures": { "count": entry.fixtures.len() },
        "uninstall": {
            "available": removal["available"],
            "steps": removal["steps"],
            "reason": removal["reason"],
            "note": entry.entry.uninstall.note,
        },
        "found_path_display": inputs.found_path.as_deref().map(display_path),
    });
    if let (Value::Object(card), Value::Object(extra)) = (&mut card, extra) {
        card.extend(extra);
    }
    Ok(card)
}

/// `agent_store_add`: adds the shipped agent (`add::add`), writes the Store record and refreshes the
/// registry and the remembered rows. If the record cannot be written, the new file is taken back.
pub fn add(id: &str, confirmed: bool) -> Result<Value, String> {
    let entry = shipped(id)?;
    add::add(id, confirmed)?;
    let record_path = record::default_path();
    let saved = mark_added_in(&record_path, id, &today(), &catalog::catalog_version());
    if saved.is_err() {
        // A file the record does not know would read as the person's own, so it goes back.
        let _ = add::remove(id);
    }
    loader::reload();
    saved?;
    Ok(json!({ "ok": true, "id": id, "state": state_of(entry, &record_path) }))
}

/// `agent_store_remove`: removes the Store's own file (`add::remove`) and clears the record's added flag.
pub fn remove(id: &str) -> Result<Value, String> {
    let out = add::remove(id)?;
    let saved = update_record(&record::default_path(), |rec| {
        rec.mark_removed(id);
        Ok(())
    });
    loader::reload();
    saved?;
    Ok(out)
}

/// `agent_store_record_install`: writes how the program was installed, from a fresh plan. The method
/// and package come from the plan's step (never from the caller), and a prerequisite is refused.
pub fn record_install(id: &str, step: usize) -> Result<Value, String> {
    let entry = shipped(id)?;
    let (plan, _) = agent_install::plan_for(id, &Tools::detect())?;
    let (method, package) = step_record(&plan, step)?;
    let record_path = record::default_path();
    let version = live_version(entry, &state::detect_at(entry, &record_path)).unwrap_or_default();
    let install = InstallRecord {
        method: method.into(),
        package,
        version: version.clone(),
        date: today(),
    };
    update_record(&record_path, |rec| rec.mark_install(id, install))?;
    Ok(json!({ "ok": true, "id": id, "method": method, "version": version }))
}

/// `agent_store_record_check`: the numbers of one "Verify it works" run and the program version.
pub fn record_check(id: &str, passed: u32, total: u32, version: &str) -> Result<Value, String> {
    record_check_in(catalog::all(), &record::default_path(), id, passed, total, version, &today())
}

/// `record_check` over any catalog, record file and date.
pub fn record_check_in(
    entries: &[CatalogEntry],
    record_path: &Path,
    id: &str,
    passed: u32,
    total: u32,
    version: &str,
    date: &str,
) -> Result<Value, String> {
    if !entries.iter().any(|entry| entry.entry.store.id == id) {
        return Err(unknown_id(id));
    }
    let version = version_only(version);
    let check = CheckRecord { date: date.into(), version: version.clone(), passed, total };
    update_record(record_path, |rec| rec.mark_check(id, check))?;
    let last = json!({ "passed": passed, "total": total, "version": version, "at": date });
    Ok(json!({ "ok": true, "id": id, "last_check": last }))
}

/// `agent_store_check_source`: the shipped definition text and fixture files of one entry, for the Check run
/// (the wizard drafts from them). A built-in entry has no definition and no fixtures.
pub fn check_source(id: &str) -> Result<Value, String> {
    check_source_in(catalog::all(), id)
}

/// `check_source` over any catalog.
pub fn check_source_in(entries: &[CatalogEntry], id: &str) -> Result<Value, String> {
    let entry = entries.iter().find(|entry| entry.entry.store.id == id).ok_or_else(|| unknown_id(id))?;
    let fixtures: Vec<Value> = entry.fixtures.iter().map(|(name, text)| json!({ "name": name, "text": text })).collect();
    Ok(json!({ "definition": entry.definition_text, "fixtures": fixtures }))
}

/// Writes "the Store added this agent" to the record file.
fn mark_added_in(record_path: &Path, id: &str, date: &str, catalog8: &str) -> Result<(), String> {
    update_record(record_path, |rec| rec.mark_added(id, date, catalog8))
}

/// Reads the record, changes it, and writes it back atomically (`record::save` refuses a bad value).
fn update_record(path: &Path, change: impl FnOnce(&mut StoreRecord) -> Result<(), String>) -> Result<(), String> {
    let mut rec = record::load(path);
    change(&mut rec)?;
    record::save(path, &rec)
}

fn card_for(entry: &CatalogEntry, env: &Env<'_>) -> Value {
    let inputs = state::detect_at(entry, env.record_path);
    let installed = (env.version)(entry, &inputs);
    card_value(entry, &inputs, installed)
}

/// The installed version of a found program: `check_probe` for a registered agent, else the entry's
/// own definition (`store_probe`). Asked only when the program was found, and remembered for 60 s.
fn live_version(entry: &CatalogEntry, inputs: &Inputs) -> Option<String> {
    if !state::found(inputs) {
        return None;
    }
    let id = entry.entry.store.id.clone();
    versions().get_or_probe(id.clone(), || {
        let here = std::env::current_dir().unwrap_or_default();
        let probe = agent_health::probe_for(&id).or_else(|| agent_install::store_probe(entry, &here))?;
        let version = version_only(&check_probe(&probe, VERSION_TIMEOUT).version?);
        (!version.is_empty()).then_some(version)
    })
}

fn versions() -> &'static TtlCache<String, String> {
    static VERSIONS: OnceLock<TtlCache<String, String>> = OnceLock::new();
    VERSIONS.get_or_init(|| TtlCache::new(VERSION_TTL))
}

/// One card (SPEC-n32 3.5). The field names are the ones `packages/app/src/lib/agent-store.ts` reads.
fn card_value(entry: &CatalogEntry, inputs: &Inputs, installed: Option<String>) -> Value {
    let store = &entry.entry.store;
    let vendor = &entry.entry.vendor;
    let checked = &entry.entry.checked;
    let (card_state, reason) = state::card_state(entry, inputs);
    json!({
        "id": store.id,
        "label": entry.label(),
        "icon": icon_of(entry),
        "tagline": store.tagline,
        "tier": store.tier,
        "builtin": store.builtin,
        "categories": store.categories,
        "status": store.status,
        "replaced_by": store.replaced_by,
        "vendor": {
            "name": vendor.name,
            "homepage": vendor.homepage,
            "license": vendor.license,
            "license_kind": vendor.license_kind,
            "license_url": none_if_empty(&vendor.license_url),
            "terms_url": none_if_empty(&vendor.terms_url),
        },
        "sign_in": { "kind": entry.entry.sign_in.kind, "summary": entry.entry.sign_in.summary },
        "checked": {
            "date": checked.date,
            "version": checked.version,
            "how": checked.how,
            "windows_tested": checked.windows_tested,
        },
        "state": card_state,
        "state_reason": reason,
        "installed_version": installed,
        "modes": modes_of(entry),
        "can_install": inputs.windows && catalog::install_spec_of(entry).is_some(),
        "ambiguous_name": store.ambiguous_name,
        "last_check": last_check(inputs.record.as_ref()),
    })
}

fn last_check(record: Option<&AgentRecord>) -> Value {
    match record.and_then(|agent| agent.check.as_ref()) {
        Some(check) => json!({
            "passed": check.passed,
            "total": check.total,
            "version": check.version,
            "at": check.date,
        }),
        None => Value::Null,
    }
}

fn state_of(entry: &CatalogEntry, record_path: &Path) -> Value {
    let (card_state, _) = state::card_state(entry, &state::detect_at(entry, record_path));
    json!(card_state)
}

fn definition(entry: &CatalogEntry) -> Option<Definition> {
    entry.definition_text.as_deref().and_then(|text| parse_definition(text).ok())
}

/// "Pane" and "One task", from the definition. A built-in agent has no definition, so its registry row
/// says which modes it has.
fn modes_of(entry: &CatalogEntry) -> Vec<&'static str> {
    let (pane, one_task) = match definition(entry) {
        Some(def) => (def.tui.is_some() || def.agent.kind == Kind::Generic, def.headless.is_some()),
        None => crate::tui::descriptor_for(&entry.entry.store.id).map_or((false, false), |d| (d.modes.visible, d.modes.headless)),
    };
    let mut modes = Vec::new();
    if pane {
        modes.push("Pane");
    }
    if one_task {
        modes.push("One task");
    }
    modes
}

/// The two letters on the card: the definition's icon, else the first two letters of the id.
fn icon_of(entry: &CatalogEntry) -> String {
    match definition(entry) {
        Some(def) => def.agent.icon_or_default(),
        None => entry.entry.store.id.chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// The argv the app runs for this agent, or `null` for a built-in agent or a definition without one.
fn run_preview(entry: &CatalogEntry) -> Value {
    definition(entry).and_then(|def| tui_argv_preview(&def)).map_or(Value::Null, |argv| json!(argv))
}

fn none_if_empty(text: &str) -> Value {
    if text.is_empty() {
        Value::Null
    } else {
        json!(text)
    }
}

/// The method and package of one agent step of `plan`: the same step the install ran. A prerequisite
/// (Node.js, Git, uv) is refused, because it is not the agent and uninstall must never remove it.
fn step_record(plan: &Plan, index: usize) -> Result<(&'static str, String), String> {
    if let Some(reason) = &plan.blocked {
        return Err(format!("INSTALL_UNAVAILABLE: {reason}"));
    }
    let step = plan
        .steps
        .get(index)
        .ok_or_else(|| format!("INSTALL_NO_SUCH_STEP: there is no step {index} for {}", plan.label))?;
    if step.tool.is_some() {
        return Err("STORE_NOT_AGENT_STEP: that step installs a tool the agent needs, not the agent. Nothing is recorded.".into());
    }
    let package = package_of(step);
    if package.is_empty() && step.method != Method::Script {
        return Err("STORE_NO_PACKAGE: the step names no package, so there is nothing to remove later.".into());
    }
    Ok((method_word(step.method), package))
}

fn method_word(method: Method) -> &'static str {
    match method {
        Method::Winget => "winget",
        Method::Npm => "npm",
        Method::Uv => "uv",
        Method::Script => "script",
    }
}

/// The package a step installs: the winget id after `--id`, else the last argument (the npm name, or
/// the PyPI name after `tool install [--python v]`). A script installs no named package.
fn package_of(step: &Step) -> String {
    match (&step.action, step.method) {
        (Action::Script { .. }, _) => String::new(),
        (Action::Argv { args, .. }, Method::Winget) => {
            args.iter().skip_while(|arg| arg.as_str() != "--id").nth(1).cloned().unwrap_or_default()
        }
        (Action::Argv { args, .. }, _) => args.last().cloned().unwrap_or_default(),
    }
}

/// The version in a program's first output line: the first word that starts with a digit (a leading
/// `v` is dropped), made of letters, digits and `.-+_`. Anything else is "", so the record holds numbers only.
fn version_only(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .map(|word| word.trim_start_matches(|c: char| c == 'v' || c == 'V'))
        .find(|word| {
            word.starts_with(|c: char| c.is_ascii_digit())
                && word.len() <= 40
                && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        })
        .unwrap_or_default()
        .to_string()
}

/// A path for the screen: the home folder shown as `~` (SPEC-n32 3.1 item 12).
pub fn display_path(path: &Path) -> String {
    display_path_in(path, crate::project_path::home_dir().as_deref())
}

fn display_path_in(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{MAIN_SEPARATOR}{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

fn shipped(id: &str) -> Result<&'static CatalogEntry, String> {
    catalog::get(id).ok_or_else(|| unknown_id(id))
}

fn unknown_id(id: &str) -> String {
    format!("STORE_UNKNOWN_ID: no agent named {id} is in the Agent Store.")
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}
