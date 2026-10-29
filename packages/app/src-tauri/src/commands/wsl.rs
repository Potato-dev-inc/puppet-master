//! Tauri commands of Linux Inside (SPEC-n33 section 3.7). Desktop only: no bridge route and no MCP
//! tool, so a phone or an agent cannot pick where an agent runs. The commands are thin: the rules
//! live in `wsl::*`. What this file adds: the checks of the arguments (a distro must be listed and
//! WSL 2, an agent must have a Linux launch), the audit line of a change, and the view the Settings
//! block shows (`DistroView`, which holds no PATH, uid or home folder).
//!
//! Every command runs its work off the UI thread (WSL calls can take seconds) and returns an
//! `OperationError` (a code and one plain sentence) when it refuses. The names and argument names
//! match `packages/app/src/lib/wsl.ts`.

use super::AppState;
use crate::audit::{self, Who};
use crate::operations::OperationError;
use crate::pty::registry_spawn_pane;
use crate::pty::SpawnPaneArgs;
use crate::tui;
use crate::wsl::model::{
    AgentPlaceRow, Distro, DistroFacts, DistroName, WslStatus, FACTS_FRESH_MS, LIST_FRESH_MS, WSL_AGENT_NOT_FOUND,
    WSL_AGENT_NOT_SUPPORTED, WSL_DISTRO_MISSING, WSL_DISTRO_NOT_V2, WSL_NOT_INSTALLED, WSL_NO_DISTRO, WSL_START_FAILED,
};
use crate::wsl::place::{self, Ask, Decision, Headless};
use crate::wsl::probe;
use crate::wsl::settings::Store;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, State};
use uuid::Uuid;

/// What `wsl_check` returns for one distro (camelCase). Holds no PATH, uid or home folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistroView {
    pub name: String,
    pub version: u8,
    pub default: bool,
    pub checked_at_ms: u64,
    pub git: Option<String>,
    pub interop: bool,
    pub networking: Option<String>,
    /// The Linux-capable agents: where each one runs, and whether it was found in this distro.
    pub agents: Vec<AgentPlaceRow>,
}

/// An agent that can run in Linux: its id, its label and the bare program the probe looks for.
struct LinuxAgent {
    id: String,
    label: String,
    program: String,
}

/// The settings file of this profile. Tests use a folder of their own (never the app data folder).
#[cfg(not(test))]
fn live_store() -> Store {
    Store::live()
}

#[cfg(test)]
fn live_store() -> Store {
    Store::at(&std::env::temp_dir().join("pm-wsl-commands-no-settings"))
}

/// Runs one blocking job on Tauri's pool. A panic or a cancelled job becomes an `INTERNAL` error.
async fn off_ui<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, OperationError> {
    tauri::async_runtime::spawn_blocking(job).await.map_err(|error| OperationError::new("INTERNAL", error.to_string(), false))
}

/// The distro list. `refresh` skips the 30 s cache. It never boots a distro.
#[tauri::command]
pub async fn wsl_status(refresh: bool) -> Result<WslStatus, OperationError> {
    off_ui(move || probe::status_cached(if refresh { 0 } else { LIST_FRESH_MS })).await
}

/// Boots one distro and probes it (up to 25 s). Only on a person's click, a pick or a spawn.
#[tauri::command]
pub async fn wsl_check(distro: String) -> Result<DistroView, OperationError> {
    off_ui(move || check_view(&distro, &live_store())).await?
}

/// The Linux-capable agents and where each one runs. Served from the settings file: it never boots a distro.
#[tauri::command]
pub async fn wsl_places_get() -> Result<Vec<AgentPlaceRow>, OperationError> {
    off_ui(|| places_view(&live_store(), live_cached)).await
}

/// Saves where one agent runs. `distro: null` sets it back to Windows and is always accepted.
#[tauri::command]
pub async fn wsl_place_set(agent_id: String, distro: Option<String>) -> Result<AgentPlaceRow, OperationError> {
    off_ui(move || {
        place_set_with(&live_store(), || probe::status_cached(LIST_FRESH_MS), live_cached, &agent_id, distro.as_deref())
    })
    .await?
}

/// The facts of one distro that a check already cached (fresh). Cache only: it never starts `wsl.exe`.
fn live_cached(distro: &str) -> Option<DistroFacts> {
    probe::cache().cached_facts(distro, FACTS_FRESH_MS)
}

/// Starts one agent in a Linux distro, whatever the saved setting says. Used from the Phase 2 UI.
#[tauri::command]
pub async fn spawn_pane_in_linux(
    args: SpawnPaneArgs,
    distro: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, OperationError> {
    let registry = state.registry.clone();
    off_ui(move || {
        let mut args = args;
        check_linux_spawn(&mut args, &distro)?;
        // The registry emits `pty://panes-changed` when the pane starts, as it does for `spawn_pane`.
        registry_spawn_pane(&registry, &app, args).map_err(|message| OperationError::new(WSL_START_FAILED, message, true))
    })
    .await?
}

/// The agents that can run in Linux: a registered profile with a Linux launch (`TuiProfile::linux_launch`).
fn linux_agents() -> Vec<LinuxAgent> {
    tui::descriptors()
        .into_iter()
        .filter_map(|descriptor| {
            let program = tui::linux_program(descriptor.id)?;
            Some(LinuxAgent { id: descriptor.id.to_string(), label: descriptor.label, program })
        })
        .collect()
}

/// The Linux-capable agent `agent_id` names (case-insensitive), or the refusal.
fn linux_agent(agent_id: &str) -> Result<LinuxAgent, OperationError> {
    linux_agents()
        .into_iter()
        .find(|agent| agent.id.eq_ignore_ascii_case(agent_id))
        .ok_or_else(|| not_supported(agent_id))
}

fn not_supported(agent_id: &str) -> OperationError {
    let label = tui::descriptor_for(agent_id).map(|descriptor| descriptor.label).unwrap_or_else(|| agent_id.to_string());
    OperationError::new(WSL_AGENT_NOT_SUPPORTED, format!("{label} can't run in Linux from Puppet Master yet."), false)
}

/// The listed distro that `distro` names (case-insensitive), when the status allows a Linux pane.
/// The same refusals as `wsl::place`, with the same codes and sentences.
fn listed_distro(distro: &str, status: &WslStatus) -> Result<Distro, OperationError> {
    let distros = match status {
        WslStatus::NotWindows => {
            return Err(OperationError::new(WSL_NOT_INSTALLED, "Linux is not available on this computer.", false))
        }
        WslStatus::NotInstalled { command } => {
            return Err(OperationError::new(
                WSL_NOT_INSTALLED,
                format!("WSL is not installed. Run \"{command}\" in PowerShell as administrator, then try again."),
                false,
            ))
        }
        WslStatus::NoDistro { command } => {
            return Err(OperationError::new(WSL_NO_DISTRO, format!("WSL has no Linux yet. Run \"{command}\", then try again."), false))
        }
        WslStatus::Broken { line } => {
            return Err(OperationError::new(WSL_START_FAILED, format!("Linux did not start: {line}"), true))
        }
        WslStatus::Ready { distros } => distros,
    };
    let found = distros
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(distro))
        .ok_or_else(|| distro_missing(distro))?;
    if found.version != 2 {
        return Err(OperationError::new(WSL_DISTRO_NOT_V2, format!("{} uses WSL 1. Agents need WSL 2.", found.name), false));
    }
    Ok(found.clone())
}

fn distro_missing(distro: &str) -> OperationError {
    OperationError::new(
        WSL_DISTRO_MISSING,
        format!("Linux \"{distro}\" was not found. Pick another one in Settings > Agents."),
        false,
    )
}

/// Whether the probe found the agent's program in the distro (an absolute path, as the probe prints it).
fn program_found(facts: &DistroFacts, program: &str) -> bool {
    matches!(facts.programs.get(program), Some(Some(abs)) if abs.starts_with('/'))
}

/// The check of one distro: list it, refuse a missing or WSL 1 one before anything boots, then probe it.
fn check_view(distro: &str, store: &Store) -> Result<DistroView, OperationError> {
    let name = DistroName::parse(distro).ok_or_else(|| distro_missing(distro))?;
    let listed = listed_distro(name.as_str(), &probe::status_cached(LIST_FRESH_MS))?;
    let programs = probe::asked_programs();
    let facts = probe::check(&listed.name, &programs)?;
    Ok(distro_view(&listed, &facts, store))
}

/// The view of one probed distro. Only the fields the Settings block shows: never PATH, uid or home.
fn distro_view(listed: &Distro, facts: &DistroFacts, store: &Store) -> DistroView {
    let agents = linux_agents()
        .into_iter()
        .map(|agent| AgentPlaceRow {
            distro: store.get(&agent.id),
            found: Some(program_found(facts, &agent.program)),
            why_not: None,
            agent_id: agent.id,
            label: agent.label,
        })
        .collect();
    DistroView {
        name: listed.name.clone(),
        version: listed.version,
        default: listed.default,
        checked_at_ms: facts.checked_at_ms,
        git: facts.git.clone(),
        interop: facts.interop,
        networking: facts.networking.clone(),
        agents,
    }
}

/// Whether the cached facts (when there are any) show the agent's program in the distro.
fn found_in(agent: &LinuxAgent, facts: Option<DistroFacts>) -> Option<bool> {
    facts.map(|facts| program_found(&facts, &agent.program))
}

/// Every Linux-capable agent, where it runs. `found` comes from the cached facts of its distro only
/// (`cached` never boots one): None when the agent runs on Windows or the distro was not checked yet.
fn places_view(store: &Store, cached: impl Fn(&str) -> Option<DistroFacts>) -> Vec<AgentPlaceRow> {
    linux_agents()
        .into_iter()
        .map(|agent| {
            let distro = store.get(&agent.id);
            let found = distro.as_deref().and_then(|name| found_in(&agent, cached(name)));
            AgentPlaceRow { distro, found, why_not: None, agent_id: agent.id, label: agent.label }
        })
        .collect()
}

/// `wsl_place_set`. `status` is read only when a distro is named (so `null` never runs `wsl.exe`), and
/// `cached` is read only then too. A distro whose cached facts lack the agent is refused
/// (`WSL_AGENT_NOT_FOUND`); an unchecked distro is accepted, and the row says `found: None`.
fn place_set_with(
    store: &Store,
    status: impl FnOnce() -> WslStatus,
    cached: impl Fn(&str) -> Option<DistroFacts>,
    agent_id: &str,
    distro: Option<&str>,
) -> Result<AgentPlaceRow, OperationError> {
    let previous = store.get(agent_id);
    let saved = match distro {
        None => None,
        Some(text) => {
            let agent = linux_agent(agent_id)?;
            let listed = listed_distro(text, &status())?;
            let name = DistroName::parse(&listed.name).ok_or_else(|| distro_missing(text))?;
            let found = found_in(&agent, cached(&listed.name));
            if found == Some(false) {
                return Err(agent_not_found(&agent.label, &listed.name));
            }
            Some((agent.id, name, found))
        }
    };
    let key = saved.as_ref().map(|(id, _, _)| id.clone()).unwrap_or_else(|| agent_id.trim().to_ascii_lowercase());
    if key.is_empty() {
        return Err(OperationError::new("INVALID_ARGUMENT", "Pick an agent first.", true));
    }
    store
        .set(&key, saved.as_ref().map(|(_, name, _)| name))
        .map_err(|error| OperationError::new("STORAGE_ERROR", error, true))?;
    let now = store.get(&key);
    audit_place(&key, previous.as_deref(), now.as_deref());
    Ok(AgentPlaceRow {
        label: tui::descriptor_for(&key).map(|descriptor| descriptor.label).unwrap_or_else(|| key.clone()),
        distro: now,
        found: saved.and_then(|(_, _, found)| found),
        why_not: None,
        agent_id: key,
    })
}

/// The refusal for a checked distro that lacks the agent. Same code and sentence as `wsl::place`.
fn agent_not_found(label: &str, distro: &str) -> OperationError {
    OperationError::new(
        WSL_AGENT_NOT_FOUND,
        format!("{label} is not installed in {distro}. Install it inside Linux, then try again."),
        false,
    )
}

/// The audit line of a change ("Runs on" setting). Never written by tests (the app's audit log).
#[cfg(not(test))]
fn audit_place(agent: &str, from: Option<&str>, to: Option<&str>) {
    if from == to {
        return;
    }
    let to_text = to.unwrap_or("Windows");
    let summary = format!("Runs on: {agent} set to {to_text}");
    audit::record(audit::Kind::Setting, Who::Desktop, &summary, json!({ "setting": "linux_place", "agent": agent, "from": from, "to": to }));
}

#[cfg(test)]
fn audit_place(_agent: &str, _from: Option<&str>, _to: Option<&str>) {}

/// The checks of `spawn_pane_in_linux` before the registry spawns anything. It names the pane (a new id
/// when none was given) and hands the distro to `place::request_once`, which the spawn reads. It
/// refuses with the same codes `place::decide` uses, so a refusal comes back before any pane exists.
fn check_linux_spawn(args: &mut SpawnPaneArgs, distro: &str) -> Result<String, OperationError> {
    let name = DistroName::parse(distro).ok_or_else(|| distro_missing(distro))?;
    let pane_id = args.pane_id.get_or_insert_with(|| Uuid::new_v4().to_string()).clone();
    let profile = tui::profile_for(&args.agent_type);
    let ask = Ask {
        agent_type: &args.agent_type,
        pane_id: &pane_id,
        account: args.account.as_deref(),
        guard_env: args.guard_env.as_deref(),
        hidden: false,
        copy: false,
        headless: Headless::No,
        kind: profile.features().kind,
        linux_program: tui::linux_program(&args.agent_type),
    };
    // A pane never runs in Linux when the agent has no Linux launch, so `decide` refuses it first.
    match place::decide(&ask, None, Some(name.as_str()))? {
        Decision::Linux { .. } => {
            place::request_once(&pane_id, &name);
            Ok(pane_id)
        }
        Decision::Windows => Err(OperationError::new(WSL_AGENT_NOT_SUPPORTED, "This pane can't run in Linux.", false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A settings file in a folder of its own (per test name), with no saved place yet.
    fn scratch_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("pm-wsl-commands-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(crate::wsl::settings::FILE_NAME));
        Store::at(&dir)
    }

    /// A distro list with the given (name, WSL version) pairs. The first one is the default.
    fn ready(distros: &[(&str, u8)]) -> WslStatus {
        WslStatus::Ready {
            distros: distros
                .iter()
                .enumerate()
                .map(|(i, (name, version))| Distro {
                    name: name.to_string(),
                    default: i == 0,
                    version: *version,
                    state: "Running".to_string(),
                })
                .collect(),
        }
    }

    fn code_of(result: Result<AgentPlaceRow, OperationError>) -> String {
        result.err().expect("the change should be refused").code
    }

    /// No distro has been checked: the cache answers nothing.
    fn no_cache(_distro: &str) -> Option<DistroFacts> {
        None
    }

    /// Facts of a checked distro named `Ubuntu`, where the given agent ids are found or not.
    fn facts_with(agents: &[(&str, bool)]) -> DistroFacts {
        let mut programs = BTreeMap::new();
        for (id, here) in agents {
            let program = tui::linux_program(id).unwrap();
            let path = (*here).then(|| format!("/usr/bin/{program}"));
            programs.insert(program, path);
        }
        DistroFacts {
            distro: "Ubuntu".into(),
            checked_at_ms: 1_000,
            uid: 1000,
            home: "/home/you".into(),
            home_windows: r"\\wsl.localhost\Ubuntu\home\you".into(),
            mount_root: "/mnt/".into(),
            path: "/usr/bin".into(),
            git: None,
            interop: true,
            networking: None,
            programs,
        }
    }

    /// A cache that holds `facts` for the distro `Ubuntu` only, and counts its reads (never a probe).
    fn one_checked<'a>(facts: DistroFacts, reads: &'a std::cell::Cell<usize>) -> impl Fn(&str) -> Option<DistroFacts> + 'a {
        move |distro: &str| {
            reads.set(reads.get() + 1);
            (distro == "Ubuntu").then(|| facts.clone())
        }
    }

    #[test]
    fn wsl_place_set_rejects_an_unknown_distro() {
        let store = scratch_store("unknown-distro");
        let result = place_set_with(&store, || ready(&[("Ubuntu", 2)]), no_cache, "claude", Some("Debian"));
        assert_eq!(code_of(result), WSL_DISTRO_MISSING);
        assert_eq!(store.get("claude"), None);
    }

    #[test]
    fn wsl_place_set_rejects_a_wsl1_distro() {
        let store = scratch_store("wsl1");
        let result = place_set_with(&store, || ready(&[("Debian", 1)]), no_cache, "claude", Some("Debian"));
        assert_eq!(code_of(result), WSL_DISTRO_NOT_V2);
        assert_eq!(store.get("claude"), None);
    }

    #[test]
    fn wsl_place_set_rejects_an_agent_without_a_linux_launch() {
        let store = scratch_store("not-capable");
        for agent in ["opencode_native", "shell", "no-such-agent"] {
            let result = place_set_with(&store, || ready(&[("Ubuntu", 2)]), no_cache, agent, Some("Ubuntu"));
            assert_eq!(code_of(result), WSL_AGENT_NOT_SUPPORTED, "{agent} must be refused");
        }
        assert_eq!(store.all().len(), 0);
    }

    #[test]
    fn wsl_place_set_accepts_null_for_any_state() {
        let store = scratch_store("null");
        place_set_with(&store, || ready(&[("Ubuntu", 2)]), no_cache, "claude", Some("Ubuntu")).unwrap();
        // WSL is off now, and null still goes through without looking at WSL or the cache.
        let status_reads = std::cell::Cell::new(0);
        let cache_reads = std::cell::Cell::new(0);
        let row = place_set_with(
            &store,
            || {
                status_reads.set(status_reads.get() + 1);
                WslStatus::NotWindows
            },
            |distro: &str| {
                cache_reads.set(cache_reads.get() + 1);
                no_cache(distro)
            },
            "claude",
            None,
        )
        .unwrap();
        assert_eq!(row.distro, None);
        assert_eq!(row.found, None);
        assert_eq!(store.get("claude"), None);
        let row = place_set_with(&store, || WslStatus::NotWindows, no_cache, "opencode_native", None).unwrap();
        assert_eq!(row.distro, None);
        assert_eq!((status_reads.get(), cache_reads.get()), (0, 0), "null never reads WSL or the cache");
    }

    #[test]
    fn wsl_place_set_refuses_an_agent_missing_from_a_checked_distro() {
        let store = scratch_store("missing-agent");
        let reads = std::cell::Cell::new(0);
        let cached = facts_with(&[("claude", true), ("codex", false)]);
        let result = place_set_with(&store, || ready(&[("Ubuntu", 2)]), one_checked(cached, &reads), "codex", Some("Ubuntu"));
        let refused = result.err().expect("codex is known to be missing in Ubuntu");
        assert_eq!(refused.code, WSL_AGENT_NOT_FOUND);
        assert_eq!(refused.message, "Codex CLI is not installed in Ubuntu. Install it inside Linux, then try again.");
        assert_eq!(store.get("codex"), None, "a refused change saves nothing");
        assert_eq!(reads.get(), 1, "the cache was read once");
    }

    #[test]
    fn wsl_place_set_saves_an_agent_found_in_a_checked_distro_with_found_true() {
        let store = scratch_store("found-true");
        let reads = std::cell::Cell::new(0);
        let cached = facts_with(&[("claude", true)]);
        let row = place_set_with(&store, || ready(&[("Ubuntu", 2)]), one_checked(cached, &reads), "claude", Some("Ubuntu")).unwrap();
        assert_eq!(row.found, Some(true));
        assert_eq!(store.get("claude").as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn wsl_place_set_accepts_an_unchecked_distro_with_found_none() {
        let store = scratch_store("unchecked");
        let row = place_set_with(&store, || ready(&[("Ubuntu", 2)]), no_cache, "codex", Some("Ubuntu")).unwrap();
        assert_eq!(row.found, None, "nothing was checked, so the row says so");
        assert_eq!(store.get("codex").as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn places_get_fills_found_from_cached_facts_only() {
        let store = scratch_store("places-found");
        store.set("claude", DistroName::parse("Ubuntu").as_ref()).unwrap();
        store.set("codex", DistroName::parse("Ubuntu").as_ref()).unwrap();
        let reads = std::cell::Cell::new(0);
        let cached = facts_with(&[("claude", true), ("codex", false)]);
        let rows = places_view(&store, one_checked(cached, &reads));
        let row = |id: &str| rows.iter().find(|row| row.agent_id == id).unwrap();
        assert_eq!(row("claude").found, Some(true), "cached facts say it is there");
        assert_eq!(row("codex").found, Some(false), "cached facts say it is missing");
        assert_eq!(row("opencode").found, None, "a Windows agent has no distro to check");
        assert_eq!(reads.get(), 2, "the cache was asked once per Linux agent set to a distro, and no more");
    }

    #[test]
    fn places_get_without_a_cache_entry_gives_found_none() {
        let store = scratch_store("places-none");
        store.set("claude", DistroName::parse("Ubuntu").as_ref()).unwrap();
        let rows = places_view(&store, no_cache);
        let claude = rows.iter().find(|row| row.agent_id == "claude").unwrap();
        assert_eq!(claude.distro.as_deref(), Some("Ubuntu"));
        assert_eq!(claude.found, None, "not checked yet");
    }

    #[test]
    fn wsl_place_set_saves_a_listed_wsl2_distro_with_its_own_spelling() {
        let store = scratch_store("saved");
        let row = place_set_with(&store, || ready(&[("Ubuntu", 2)]), no_cache, "Claude", Some("ubuntu")).unwrap();
        assert_eq!(row.agent_id, "claude");
        assert_eq!(row.distro.as_deref(), Some("Ubuntu"));
        assert_eq!(store.get("claude").as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn places_get_lists_only_linux_capable_agents() {
        let store = scratch_store("places");
        let ids: Vec<String> = places_view(&store, no_cache).into_iter().map(|row| row.agent_id).collect();
        for id in ["claude", "codex", "opencode"] {
            assert!(ids.iter().any(|x| x == id), "{id} should be listed");
        }
        assert!(!ids.iter().any(|x| x == "opencode_native"));
        assert!(!ids.iter().any(|x| x == "shell"));
    }

    #[test]
    fn distro_view_has_no_path_uid_or_home_keys() {
        let store = scratch_store("view");
        let listed = Distro { name: "Ubuntu".into(), default: true, version: 2, state: "Running".into() };
        let mut programs = BTreeMap::new();
        programs.insert(tui::linux_program("claude").unwrap(), Some("/usr/bin/claude".to_string()));
        programs.insert(tui::linux_program("codex").unwrap(), None);
        let facts = DistroFacts {
            distro: "Ubuntu".into(),
            checked_at_ms: 1_000,
            uid: 1234,
            home: "/home/secretuser".into(),
            home_windows: r"\\wsl.localhost\Ubuntu\home\secretuser".into(),
            mount_root: "/mnt/".into(),
            path: "/opt/secret-bin:/usr/bin".into(),
            git: Some("2.43.0".into()),
            interop: true,
            networking: Some("nat".into()),
            programs,
        };
        let view = serde_json::to_value(distro_view(&listed, &facts, &store)).unwrap();
        let object = view.as_object().unwrap();
        for key in ["path", "uid", "home", "homeWindows"] {
            assert!(!object.contains_key(key), "DistroView must not carry {key}");
        }
        for row in view["agents"].as_array().unwrap() {
            let row = row.as_object().unwrap();
            for key in ["path", "uid", "home", "homeWindows"] {
                assert!(!row.contains_key(key));
            }
        }
        let text = view.to_string();
        assert!(!text.contains("secretuser") && !text.contains("secret-bin") && !text.contains("1234"));
        assert_eq!(object["checkedAtMs"], json!(1_000));
        let claude = view["agents"].as_array().unwrap().iter().find(|row| row["agentId"] == "claude").unwrap();
        assert_eq!(claude["found"], json!(true));
    }

    #[test]
    fn spawn_in_linux_refuses_a_saved_login_before_any_spawn() {
        let mut args = SpawnPaneArgs {
            agent_type: "claude".into(),
            cwd: Some("C:\\work".into()),
            cols: None,
            rows: None,
            extra_args: None,
            pane_id: None,
            read_only: false,
            account: Some("work-login".into()),
            guard_env: None,
        };
        let refused = check_linux_spawn(&mut args, "Ubuntu").unwrap_err();
        assert_eq!(refused.code, crate::wsl::model::WSL_ACCOUNT_NOT_YET);
        assert!(args.pane_id.is_some(), "the pane id is named before the refusal");
    }

    #[test]
    fn spawn_in_linux_refuses_an_agent_without_a_linux_launch() {
        let mut args = SpawnPaneArgs {
            agent_type: "shell".into(),
            cwd: None,
            cols: None,
            rows: None,
            extra_args: None,
            pane_id: None,
            read_only: false,
            account: None,
            guard_env: None,
        };
        let refused = check_linux_spawn(&mut args, "Ubuntu").unwrap_err();
        assert_eq!(refused.code, WSL_AGENT_NOT_SUPPORTED);
    }

    #[test]
    fn spawn_in_linux_names_the_pane_and_asks_for_the_distro_once() {
        let mut args = SpawnPaneArgs {
            agent_type: "codex".into(),
            cwd: None,
            cols: None,
            rows: None,
            extra_args: None,
            pane_id: Some("linux-test-pane-1".into()),
            read_only: false,
            account: None,
            guard_env: None,
        };
        let pane_id = check_linux_spawn(&mut args, "Ubuntu").unwrap();
        assert_eq!(pane_id, "linux-test-pane-1");
        assert_eq!(place::take_once(&pane_id).as_deref(), Some("Ubuntu"));
        assert_eq!(place::take_once(&pane_id), None, "the request applies to one spawn only");
    }
}
