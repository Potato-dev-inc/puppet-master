//! Tauri commands for "Add chat (ACP)" on an agent that has no `[acp]` table (`acp::overlay`). Desktop app only:
//! no bridge route, no MCP tool, so a phone or an agent cannot add a command the app will start. The person's
//! click is the approval: `acp_overlay_set` takes the exact command line and refuses unless `confirmed` is true.
//! All of them need the Labs switch (like `acp_conversation_create`).
//!
//! "Add chat" resolves an agent in this order, all with one confirm click (`acp_overlay_resolve`):
//! 1. native: the installed program's own help shows an ACP command (`acp_overlay_detect`, then `acp_overlay_set`);
//! 2. registry: the official ACP registry (`acp::registry`) lists a ready-made adapter or ACP-speaking build, which the
//!    app downloads and pins after the person confirms (`acp_overlay_install`, progress as `acp-addon-changed` events);
//! 3. terminal chat (`crate::terminal_chat`): the agent keeps running in a terminal pane and the chat reads its session
//!    file; `acp_overlay_install` with `{kind: "terminal_chat"}` only saves the agent's lane (no overlay entry, no adapter);
//! 4. bridge: the built-in bridge (`acp::bridge`), the last resort for an agent with a non-interactive mode: no
//!    download, `acp_overlay_install` with `{kind: "bridge"}` just writes `{source: "bridge"}`.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::acp::install::{self, AddonState, AddonStatus, InstallError, RealRunner};
use crate::acp::lane::{LaneStore, RunLane};
use crate::acp::overlay::{self, BridgeEntry, OverlayEntry, OverlayItem, OverlayStore};
use crate::acp::registry::{self, Choice, RegistryAgent};
use crate::acp::{self, config};

/// Event of the add-on installer (`commands::acp_install::EVENT`); the registry install reuses its shape.
const EVENT: &str = "acp-addon-changed";

fn labs_gate() -> Result<(), String> {
    if acp::chat_enabled() {
        Ok(())
    } else {
        Err("FEATURE_OFF: chat (ACP) is switched off in Labs".into())
    }
}

/// An agent that is registered and can take an overlay: a runnable agent (no shell) with no table of its own.
fn check_target(agent: &str) -> Result<String, String> {
    let id = agent.trim().to_ascii_lowercase();
    let Some(profile) = crate::tui::registry::registry().find(&id) else {
        return Err(format!("INVALID_ARGUMENT: unknown agent {agent}"));
    };
    if profile.features().kind == crate::tui::Kind::Shell {
        return Err(format!("INVALID_ARGUMENT: {agent} is a shell, not an agent"));
    }
    if profile.acp_table().is_some() {
        return Err(format!("INVALID_ARGUMENT: {agent} already has its own chat (ACP) definition; the overlay is only for agents without one"));
    }
    Ok(profile.id().to_string())
}

/// Locate the agent's program and read only its help text (`wizard::acp::detect_program`): no wizard session, no
/// subcommand probing, so it answers in about one program start.
fn detect_in(wizard: &crate::tui::declarative::wizard::Wizard, program: &str) -> Result<Value, String> {
    crate::tui::declarative::wizard::acp::detect_program(wizard, program).map_err(|e| format!("{}: {}", e.code, e.message))
}

/// How the agent's program offers the Agent Client Protocol: `{ok, command, candidates: [{command, args, evidence,
/// confidence}], stdio_hint, mentions_protocol}`, the same as the wizard's `acp_detect`. Only the wizard's fixed
/// help probes run (`--version`, `--help` and the like); nothing else starts.
#[tauri::command]
pub async fn acp_overlay_detect(agent: String) -> Result<Value, String> {
    labs_gate()?;
    let id = check_target(&agent)?;
    let program = overlay::program_of(&id).ok_or_else(|| format!("INVALID_ARGUMENT: unknown agent {agent}"))?;
    tauri::async_runtime::spawn_blocking(move || detect_in(crate::tui::declarative::wizard::Wizard::app(), &program))
        .await
        .map_err(|e| e.to_string())?
}

// ---- resolve: which ways are there to give this agent a chat? ---------------------------------------------

/// The bridge as an option, from what `acp::bridge::available_for` said: available with one plain sentence for the
/// confirm screen, or unavailable with the plain reason (no non-interactive mode).
pub(crate) fn bridge_option_from(info: Result<acp::bridge::BridgeInfo, String>) -> Value {
    match info {
        Ok(info) => json!({
            "kind": "bridge",
            "label": "Chat made by Puppet Master",
            "summary": format!(
                "Puppet Master will make a chat adapter for {} from its non-interactive mode; text only, no tool cards; {}.",
                registry::display_text(&info.label, 60),
                if info.has_memory { "it remembers the conversation" } else { "it does not remember earlier messages" }
            ),
            "bridge": { "has_memory": info.has_memory, "read_only_supported": info.read_only_supported, "note": registry::display_text(&info.note, 300) },
            "available": true,
        }),
        Err(reason) => json!({
            "kind": "bridge",
            "label": "Chat made by Puppet Master",
            "summary": "Puppet Master makes a chat adapter from the agent's non-interactive mode.",
            "available": false,
            "reason": registry::display_text(&reason, 240),
        }),
    }
}

/// The terminal-chat route as an option (`None` when the agent has no session reader: that agent keeps the bridge).
/// The UI calls it "Chat (terminal)", never "ACP".
pub(crate) fn terminal_chat_option_from(info: Result<crate::terminal_chat::TerminalChatInfo, String>) -> Option<Value> {
    let info = info.ok()?;
    let mut summary = crate::terminal_chat::lane::summary_sentence(&info);
    if !info.can_answer_prompts {
        // Without a prompt parser the questions stay in the terminal; say so instead of promising a card.
        summary = summary.replace("and you can answer its questions here.", "and you answer its questions in the terminal.");
    }
    Some(json!({
        "kind": "terminal_chat",
        "label": "Chat (terminal)",
        "summary": registry::display_text(&summary, 400),
        "terminal_chat": { "reads_file": info.reads_file, "can_resume": info.can_resume, "can_answer_prompts": info.can_answer_prompts, "note": registry::display_text(&info.note, 300) },
        "available": true,
    }))
}

fn bridge_option(agent: &str) -> Value {
    bridge_option_from(acp::bridge::available_for(agent))
}

fn native_option(program: &str, detect: Result<Value, String>) -> Value {
    let unavailable = |reason: String| {
        json!({
            "kind": "native",
            "label": format!("{program} on this computer"),
            "summary": "The program you already have starts chat itself.",
            "available": false,
            "reason": reason,
        })
    };
    let found = match detect {
        Ok(v) => v,
        Err(e) => return unavailable(format!("Could not read the program's help: {}", registry::display_text(&e, 160))),
    };
    let list = found.get("candidates").and_then(Value::as_array).cloned().unwrap_or_default();
    let best = list.iter().find(|c| c.get("confidence").and_then(Value::as_str) == Some("high")).or_else(|| list.first());
    let Some(best) = best else { return unavailable("Its help does not show a chat (ACP) command.".into()) };
    let command = best.get("command").and_then(Value::as_str).unwrap_or(program).to_string();
    let args: Vec<String> = best.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let line = std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    json!({
        "kind": "native",
        "label": format!("{program} on this computer"),
        "summary": "The program you already have starts chat itself.",
        "command_line": line,
        "confidence": best.get("confidence").and_then(Value::as_str).unwrap_or("medium"),
        "acp": { "command": command, "args": args, "env_allow": [] },
        "available": true,
    })
}

/// The version the person will get (npx: the package's, binary: the entry's).
fn choice_version(agent: &RegistryAgent, choice: &Choice) -> String {
    match choice {
        Choice::Npx { version, .. } => version.clone(),
        _ => agent.version.clone(),
    }
}

fn registry_option(agent: &RegistryAgent, choice: &Choice, origin: registry::Origin) -> Value {
    let text = |t: &str, n: usize| registry::display_text(t, n);
    let distribution = match choice {
        Choice::Npx { .. } => "npx",
        Choice::Binary { .. } => "binary",
        Choice::Unsupported(_) if agent.distribution.npx.is_some() => "npx",
        Choice::Unsupported(_) if !agent.distribution.binary.is_empty() => "binary",
        Choice::Unsupported(_) => "uvx",
    };
    let version = text(&choice_version(agent, choice), 30);
    let mut option = json!({
        "kind": "registry",
        "label": format!("{} {}", text(&agent.name, 60), version).trim().to_string(),
        "summary": text(&agent.description, 240),
        "registry": {
            "id": text(&agent.id, 80),
            "name": text(&agent.name, 60),
            "version": version,
            "authors": agent.authors.iter().take(5).map(|a| text(a, 80)).collect::<Vec<_>>(),
            "license": text(&agent.license, 60),
            "repository": text(&agent.repository, 200),
            "distribution": distribution,
            "size_hint": Value::Null,
            "list": match origin { registry::Origin::Live => "live", registry::Origin::Cache => "cache", registry::Origin::Snapshot => "snapshot" },
        },
        "available": !matches!(choice, Choice::Unsupported(_)),
    });
    if let Choice::Unsupported(why) = choice {
        option["reason"] = json!(text(why, 240));
    }
    option
}

/// The options, best first: native with high confidence, the registry, the other native guesses, terminal chat (when the
/// agent has a session reader), the bridge.
pub(crate) fn options_from(program: &str, native: Result<Value, String>, registry_hit: Option<(&RegistryAgent, &Choice, registry::Origin)>, terminal_chat: Option<Value>, bridge: Value) -> Vec<Value> {
    let native = native_option(program, native);
    let high = native["available"] == json!(true) && native["confidence"] == json!("high");
    let mut out: Vec<Value> = Vec::new();
    if high {
        out.push(native.clone());
    }
    if let Some((agent, choice, origin)) = registry_hit {
        out.push(registry_option(agent, choice, origin));
    }
    if !high {
        out.push(native);
    }
    out.extend(terminal_chat);
    out.push(bridge);
    // Available options first; each group keeps its order (the sort is stable).
    out.sort_by_key(|o| o["available"] != json!(true));
    out
}

fn resolve_in(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, label: &str, program: &str) -> Value {
    let native = detect_in(wizard, program);
    let (reg, origin) = registry::load_default();
    let platform = registry::platform_key();
    let hit = registry::resolve(&reg, program, id, label, &platform);
    let options = options_from(program, native, hit.as_ref().map(|(a, c)| (*a, c, origin)), terminal_chat_option_from(crate::terminal_chat::available_for(id)), bridge_option(id));
    json!({ "ok": true, "options": options })
}

/// Every way this agent can get a chat, best first: `{ok, options: [{kind: native|registry|terminal_chat|bridge, label, summary,
/// command_line?, acp?, registry?: {id, name, version, authors, license, repository, distribution, size_hint?}, available,
/// reason?, terminal_chat?: {reads_file, can_resume, can_answer_prompts, note}}]}`. Runs only the program's help (like `acp_overlay_detect`) and reads the registry (live, cache or the
/// snapshot in the app); nothing is installed or saved.
#[tauri::command]
pub async fn acp_overlay_resolve(agent: String) -> Result<Value, String> {
    labs_gate()?;
    let id = check_target(&agent)?;
    let program = overlay::program_of(&id).ok_or_else(|| format!("INVALID_ARGUMENT: unknown agent {agent}"))?;
    let label = crate::tui::descriptor_for(&id).map(|d| d.label).unwrap_or_else(|| id.clone());
    tauri::async_runtime::spawn_blocking(move || resolve_in(crate::tui::declarative::wizard::Wizard::app(), &id, &label, &program)).await.map_err(|e| e.to_string())
}

// ---- install from the registry --------------------------------------------------------------------------

fn status_json(id: &str, label: &str, state: AddonState, version: Option<String>, detail: String, lines: Vec<String>) -> Value {
    let status = AddonStatus { agent: id.into(), label: label.into(), state, version: version.clone(), pinned_version: version, detail, size_mb: None, lines, signin_hint: String::new() };
    serde_json::to_value(status).unwrap_or(Value::Null)
}

/// The registry entry and distribution to install for `id`, checked against what the person confirmed (`expect`:
/// the registry id and version on the screen; a registry that changed since is refused, never installed silently).
fn registry_target(reg: &registry::Registry, id: &str, label: &str, program: &str, expect: &Value) -> Result<(RegistryAgent, Choice), String> {
    let (agent, choice) = registry::resolve(reg, program, id, label, &registry::platform_key()).ok_or_else(|| format!("NOT_AVAILABLE: the registry has nothing for {label}"))?;
    if let Choice::Unsupported(why) = &choice {
        return Err(format!("NOT_AVAILABLE: {}", registry::display_text(why, 240)));
    }
    let shown = |key: &str| expect.get(key).and_then(Value::as_str).filter(|v| !v.is_empty());
    if shown("id").is_some_and(|v| v != registry::display_text(&agent.id, 80)) || shown("version").is_some_and(|v| v != registry::display_text(&choice_version(agent, &choice), 30)) {
        return Err("REGISTRY_CHANGED: the registry entry is not the one you confirmed. Look again.".into());
    }
    Ok((agent.clone(), choice))
}

/// Saves the built-in bridge as the chat of `id` (no download). Refused when the agent has no non-interactive mode.
pub(crate) fn install_bridge_in(store: &OverlayStore, id: &str, label: &str) -> Result<(), String> {
    overlay::bridge_config(id, label).map_err(|e| format!("NOT_AVAILABLE: {}", registry::display_text(&e, 240)))?;
    store.put_item(id, &OverlayItem::Bridge(BridgeEntry { raw: json!({ "source": "bridge" }) }))
}

/// Saves terminal chat as the agent's lane. No overlay entry, no adapter: the agent keeps its own terminal pane.
pub(crate) fn install_terminal_chat_in(lanes: &LaneStore, id: &str) -> Result<(), String> {
    crate::terminal_chat::available_for(id).map_err(|e| format!("NOT_AVAILABLE: {}", registry::display_text(&e, 240)))?;
    lanes.set(id, RunLane::TerminalChat)
}

/// Writes the overlay of the built-in bridge, or downloads and installs what the registry lists for `agent` and then
/// writes the overlay. `confirmed` must be true. `option` = `{kind: "bridge"}`, `{kind: "terminal_chat"}` (saves the lane only) or `{kind: "registry", id?, version?}` (`id` and `version` are what the screen showed). Runs to the end
/// (minutes for a large package): progress comes as `acp-addon-changed` events `{agent, status}` (the add-on
/// installer's shape; `status.state`: installing | installed | failed, `status.lines`). Returns the lane view.
#[tauri::command]
pub async fn acp_overlay_install(app: AppHandle, agent: String, option: Value, confirmed: bool) -> Result<Value, String> {
    labs_gate()?;
    if !confirmed {
        return Err("NOT_CONFIRMED: the person must confirm the download (confirmed: true)".into());
    }
    match option.get("kind").and_then(Value::as_str) {
        Some("registry") => {}
        Some("terminal_chat") => {
            let id = check_target(&agent)?;
            install_terminal_chat_in(&LaneStore::in_app_data(), &id)?;
            return Ok(super::acp_lane::view());
        }
        Some("bridge") => {
            let id = check_target(&agent)?;
            let label = crate::tui::descriptor_for(&id).map(|d| d.label).unwrap_or_else(|| id.clone());
            install_bridge_in(&OverlayStore::in_app_data(), &id, &label)?;
            return Ok(super::acp_lane::view());
        }
        _ => return Err("INVALID_ARGUMENT: option.kind must be \"registry\", \"terminal_chat\" or \"bridge\" (a program you already have is saved with acp_overlay_set)".into()),
    }
    let id = check_target(&agent)?;
    let program = overlay::program_of(&id).ok_or_else(|| format!("INVALID_ARGUMENT: unknown agent {agent}"))?;
    let label = crate::tui::descriptor_for(&id).map(|d| d.label).unwrap_or_else(|| id.clone());
    install::addons().begin(&id).map_err(|e| format!("ACP_ADDON_BUSY: {e}"))?;
    let (id2, label2) = (id.clone(), label.clone());
    let emitter = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let changed = || {
            if let Some((state, lines, detail)) = install::addons().live_state(&id2) {
                let _ = emitter.emit(EVENT, json!({ "agent": id2, "status": status_json(&id2, &label2, state, None, detail, lines) }));
            }
        };
        let mut job = |on_line: &mut dyn FnMut(&str)| -> Result<String, InstallError> {
            on_line("Reading the registry...");
            let (reg, _) = registry::load_default();
            let (entry, choice) = registry_target(&reg, &id2, &label2, &program, &option).map_err(InstallError::NotInstallable)?;
            let written = acp::install_registry::install_for_agent(&OverlayStore::in_app_data(), &id2, &label2, &entry, &choice, &config::RealHost, &RealRunner, &registry::RealHttp, on_line)?;
            Ok(written.version)
        };
        let result = install::addons().run_job(&id2, &changed, &mut job);
        match &result {
            Ok(version) => {
                tracing::info!(agent = %id2, "acp registry add-on installed: {version}");
                let _ = emitter.emit(EVENT, json!({ "agent": id2, "status": status_json(&id2, &label2, AddonState::Installed, Some(version.clone()), String::new(), Vec::new()) }));
            }
            Err(error) => tracing::warn!(agent = %id2, "acp registry add-on failed: {error}"),
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?;
    match outcome {
        Ok(_) => Ok(super::acp_lane::view()),
        Err(InstallError::Busy) => Err("ACP_ADDON_BUSY: The add-on is already being installed.".into()),
        Err(error) => Err(format!("ACP_ADDON_FAILED: {error}")),
    }
}

/// Validates and writes the overlay. `acp` = `{command, args, env_allow, resume_command?}` is the exact command
/// line the person read; nothing is written unless `confirmed`.
pub(crate) fn set_in(store: &OverlayStore, agent: &str, acp: &Value, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("NOT_CONFIRMED: the person must confirm the exact command line (confirmed: true)".into());
    }
    let id = check_target(agent)?;
    let entry = OverlayEntry::from_json(acp)?;
    let label = crate::tui::descriptor_for(&id).map(|d| d.label).unwrap_or_else(|| id.clone());
    overlay::build_config(&id, &label, &entry)?;
    store.put(&id, &entry)
}

/// Saves the chat setup of an agent that has none. Returns the lane view (the agent now has an `overlay: true` row).
#[tauri::command]
pub fn acp_overlay_set(agent: String, acp: Value, confirmed: bool) -> Result<Value, String> {
    labs_gate()?;
    set_in(&OverlayStore::in_app_data(), &agent, &acp, confirmed)?;
    Ok(super::acp_lane::view())
}

/// Removes the overlay entry and, when the agent's lane was terminal chat, puts the lane back to Terminal.
/// `acp_chat_remove` (commands/acp_remove.rs) does its overlay + terminal-chat step through this function. Says whether an entry was there.
pub(crate) fn remove_in(overlay: &OverlayStore, lanes: &LaneStore, agent: &str) -> Result<bool, String> {
    let had = overlay.remove(agent)?;
    if lanes.lane_for(agent) == RunLane::TerminalChat {
        lanes.set(agent, RunLane::Terminal)?;
    }
    Ok(had)
}

/// Removes the chat setup. Existing conversations stay as records (they say the agent is not available). Returns the view.
#[tauri::command]
pub fn acp_overlay_remove(agent: String) -> Result<Value, String> {
    labs_gate()?;
    if agent.trim().is_empty() {
        return Err("INVALID_ARGUMENT: agent is required".into());
    }
    remove_in(&OverlayStore::in_app_data(), &LaneStore::in_app_data(), &agent)?;
    Ok(super::acp_lane::view())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (OverlayStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pm-acp-ovcmd-{}", uuid::Uuid::new_v4()));
        (OverlayStore::at(dir.join("acp_overlay.json")), dir)
    }

    fn target() -> String {
        overlay::addable_agents(&OverlayStore::at(std::env::temp_dir().join("pm-nonexistent-overlay.json"))).remove(0).id
    }

    #[test]
    fn set_needs_the_confirmation_and_writes_only_then() {
        let (store, dir) = temp();
        let id = target();
        let acp = json!({"command": "adapter", "args": ["acp"], "env_allow": ["MY_KEY"]});
        assert!(set_in(&store, &id, &acp, false).unwrap_err().starts_with("NOT_CONFIRMED"));
        assert!(store.get(&id).is_none());
        set_in(&store, &id, &acp, true).unwrap();
        assert_eq!(store.get(&id).unwrap().args, vec!["acp".to_string()]);
        set_in(&store, &id.to_uppercase(), &json!({"command": "adapter2"}), true).unwrap();
        assert_eq!(store.get(&id).unwrap().command, "adapter2", "setting again replaces");
        assert!(store.remove(&id).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn set_refuses_unknown_shell_own_table_and_invalid() {
        let (store, dir) = temp();
        let acp = json!({"command": "adapter"});
        assert!(set_in(&store, "no_such_agent_zz", &acp, true).unwrap_err().contains("unknown agent"));
        let shell = crate::tui::registry::registry().ordered().into_iter().find(|p| p.features().kind == crate::tui::Kind::Shell);
        if let Some(shell) = shell {
            assert!(set_in(&store, shell.id(), &acp, true).unwrap_err().contains("shell"));
        }
        let own = config::builtin_agents().into_iter().next().expect("an agent with its own table");
        assert!(set_in(&store, &own.id, &acp, true).unwrap_err().contains("already has its own"));
        let id = target();
        assert!(set_in(&store, &id, &json!({"command": "bash", "args": ["-c", "x"]}), true).unwrap_err().contains("AGENT_DEF_SHELL_INTERPRETER"));
        assert!(set_in(&store, &id, &json!({"command": "adapter", "env_allow": ["PATH"]}), true).unwrap_err().contains("AGENT_DEF_DENIED_ENV"));
        assert!(set_in(&store, &id, &json!({"command": "adapter", "extra": 1}), true).is_err());
        assert!(set_in(&store, &id, &json!("adapter"), true).is_err());
        assert!(store.get(&id).is_none(), "nothing was written");
        assert!(!dir.join("acp_overlay.json").exists());
    }

    fn native(confidence: &str) -> Value {
        json!({"ok": true, "command": "tool", "candidates": [{"command": "tool", "args": ["acp"], "evidence": "acp  start", "confidence": confidence}]})
    }

    fn hit<'a>(reg: &'a registry::Registry, program: &str, id: &str) -> Option<(&'a RegistryAgent, Choice, registry::Origin)> {
        registry::resolve(reg, program, id, id, "windows-x86_64").map(|(a, c)| (a, c, registry::Origin::Snapshot))
    }

    fn bridge_info(memory: bool) -> Result<acp::bridge::BridgeInfo, String> {
        Ok(acp::bridge::BridgeInfo { agent_id: "x".into(), label: "Xagent".into(), has_memory: memory, read_only_supported: false, installed: true, note: "n".into() })
    }

    #[test]
    fn options_are_ordered_best_first_with_the_bridge_last() {
        let reg = registry::snapshot();
        // native with high confidence beats the registry
        let h = hit(&reg, "cline", "cline");
        let options = options_from("cline", Ok(native("high")), h.as_ref().map(|(a, c, o)| (*a, c, *o)), None, bridge_option_from(bridge_info(true)));
        let kinds: Vec<&str> = options.iter().map(|o| o["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["native", "registry", "bridge"]);
        assert_eq!(options[0]["command_line"], "tool acp");
        assert_eq!(options[0]["acp"], json!({"command": "tool", "args": ["acp"], "env_allow": []}));
        assert_eq!(options[1]["registry"]["distribution"], "npx");
        assert_eq!(options[1]["available"], true);
        assert_eq!(options[2]["available"], true, "the bridge is a real option, last");
        // a medium guess comes after the registry
        let options = options_from("cline", Ok(native("medium")), h.as_ref().map(|(a, c, o)| (*a, c, *o)), None, bridge_option_from(bridge_info(true)));
        let kinds: Vec<&str> = options.iter().map(|o| o["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["registry", "native", "bridge"]);
        // no help, no registry entry: nothing available, reasons given
        let options = options_from("zzz", Err("WIZARD_NOT_FOUND: zzz was not found".into()), None, None, bridge_option_from(Err("Z has no non-interactive mode, so a chat adapter cannot be made".into())));
        assert!(options.iter().all(|o| o["available"] == false));
        assert!(options[0]["reason"].as_str().unwrap().contains("Could not read"));
        let bridge = options.iter().find(|o| o["kind"] == "bridge").unwrap();
        assert_eq!(bridge["reason"], "Z has no non-interactive mode, so a chat adapter cannot be made");
        // a registry entry this build cannot install is listed as unavailable with the reason
        let h = hit(&reg, "no-such-program", "fast-agent");
        let options = options_from("no-such-program", Err("x".into()), h.as_ref().map(|(a, c, o)| (*a, c, *o)), None, bridge_option_from(bridge_info(true)));
        let r = options.iter().find(|o| o["kind"] == "registry").unwrap();
        assert_eq!((r["available"].clone(), r["registry"]["distribution"].clone()), (json!(false), json!("uvx")));
        assert!(r["reason"].as_str().unwrap().contains("uvx"));
    }

    #[test]
    fn the_bridge_option_says_in_plain_words_what_the_person_gets() {
        let with = bridge_option_from(bridge_info(true));
        let without = bridge_option_from(bridge_info(false));
        assert_eq!((with["kind"].clone(), with["available"].clone()), (json!("bridge"), json!(true)));
        let (a, b) = (with["summary"].as_str().unwrap(), without["summary"].as_str().unwrap());
        assert!(a.contains("Xagent") && a.contains("non-interactive mode") && a.contains("text only, no tool cards") && a.contains("it remembers the conversation"), "{a}");
        assert!(b.contains("it does not remember earlier messages"), "{b}");
        assert_eq!(with["bridge"]["has_memory"], true);
    }

    #[test]
    fn resolve_lists_the_bridge_available_with_a_headless_mode_and_unavailable_without() {
        let headless = crate::tui::registered_agent_ids().into_iter().find(|id| acp::bridge::available_for(id).is_ok()).expect("a headless agent");
        let o = bridge_option(&headless);
        assert_eq!((o["kind"].clone(), o["available"].clone()), (json!("bridge"), json!(true)));
        assert!(o["summary"].as_str().unwrap().contains("non-interactive mode"));
        let none = crate::tui::registered_agent_ids().into_iter().find(|id| acp::bridge::available_for(id).is_err());
        if let Some(none) = none {
            let o = bridge_option(&none);
            assert_eq!(o["available"], false);
            assert!(o["reason"].as_str().unwrap().contains("non-interactive"), "{o}");
        }
    }

    #[test]
    fn installing_the_bridge_writes_the_overlay_and_refuses_without_headless() {
        let (store, dir) = temp();
        let headless = crate::tui::registered_agent_ids().into_iter().find(|id| acp::bridge::available_for(id).is_ok() && check_target(id).is_ok()).expect("an addable headless agent");
        install_bridge_in(&store, &headless, "H").unwrap();
        assert_eq!(store.get_item(&headless).unwrap().source(), "bridge");
        let text = std::fs::read_to_string(dir.join("acp_overlay.json")).unwrap();
        assert!(text.contains("\"source\": \"bridge\""), "{text}");
        assert!(store.remove(&headless).unwrap());
        assert!(store.get_item(&headless).is_none());
        let none = crate::tui::registered_agent_ids().into_iter().find(|id| acp::bridge::available_for(id).is_err());
        if let Some(none) = none {
            assert!(install_bridge_in(&store, &none, "N").unwrap_err().starts_with("NOT_AVAILABLE"));
            assert!(store.get_item(&none).is_none());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tc_info(resume: bool, answer: bool) -> Result<crate::terminal_chat::TerminalChatInfo, String> {
        Ok(crate::terminal_chat::TerminalChatInfo { label: "Xagent".into(), reads_file: true, can_resume: resume, can_answer_prompts: answer, note: "n".into() })
    }

    #[test]
    fn terminal_chat_ranks_between_the_registry_and_the_bridge() {
        let reg = registry::snapshot();
        let h = hit(&reg, "cline", "cline");
        let tc = terminal_chat_option_from(tc_info(true, true));
        let options = options_from("cline", Ok(native("high")), h.as_ref().map(|(a, c, o)| (*a, c, *o)), tc.clone(), bridge_option_from(bridge_info(true)));
        let kinds: Vec<&str> = options.iter().map(|o| o["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["native", "registry", "terminal_chat", "bridge"]);
        let options = options_from("cline", Ok(native("medium")), h.as_ref().map(|(a, c, o)| (*a, c, *o)), tc.clone(), bridge_option_from(bridge_info(true)));
        let kinds: Vec<&str> = options.iter().map(|o| o["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["registry", "native", "terminal_chat", "bridge"]);
        // nothing else works: terminal chat is still ahead of the bridge
        let options = options_from("zzz", Err("x".into()), None, tc, bridge_option_from(bridge_info(true)));
        let kinds: Vec<&str> = options.iter().map(|o| o["kind"].as_str().unwrap()).collect();
        let pos = |k: &str| kinds.iter().position(|x| *x == k).unwrap();
        assert!(pos("terminal_chat") < pos("bridge"));
        // an agent with no session reader gets no terminal-chat row at all
        assert!(terminal_chat_option_from(Err("no file".into())).is_none());
        let options = options_from("zzz", Err("x".into()), None, None, bridge_option_from(bridge_info(true)));
        assert!(options.iter().all(|o| o["kind"] != "terminal_chat"));
    }

    #[test]
    fn the_terminal_chat_option_uses_the_exact_sentence_and_never_says_acp() {
        let full = terminal_chat_option_from(tc_info(true, true)).unwrap();
        assert_eq!(full["label"], "Chat (terminal)");
        assert_eq!(
            full["summary"],
            "Puppet Master runs Xagent in a terminal and shows it as a chat. You get its tool cards and history, and you can answer its questions here. It can continue the conversation later."
        );
        let no_resume = terminal_chat_option_from(tc_info(false, true)).unwrap();
        assert!(no_resume["summary"].as_str().unwrap().ends_with("It cannot resume old sessions."));
        let read_only = terminal_chat_option_from(tc_info(true, false)).unwrap();
        assert!(read_only["summary"].as_str().unwrap().contains("you answer its questions in the terminal"));
        for o in [&full, &no_resume, &read_only] {
            assert!(!o.to_string().to_lowercase().contains("acp"), "{o}");
            assert_eq!(o["available"], true);
        }
        assert_eq!(full["terminal_chat"]["can_answer_prompts"], true);
    }

    #[test]
    fn installing_terminal_chat_writes_the_lane_only_and_remove_resets_it() {
        let (overlay, dir) = temp();
        let lanes = LaneStore::at(dir.join("agent_lane.json"));
        install_terminal_chat_in(&lanes, "claude").unwrap();
        assert_eq!(lanes.lane_for("claude"), RunLane::TerminalChat);
        assert!(overlay.get_item("claude").is_none(), "no overlay entry");
        assert!(!dir.join("acp_overlay.json").exists(), "the overlay file is untouched");
        remove_in(&overlay, &lanes, "claude").unwrap();
        assert_eq!(lanes.lane_for("claude"), RunLane::Terminal);
        // an agent with no reader is refused and nothing is saved
        let none = crate::tui::registry::snapshot().iter().find(|p| p.session_chat().is_none()).map(|p| p.id().to_string()).expect("an agent without a reader");
        assert!(install_terminal_chat_in(&lanes, &none).unwrap_err().starts_with("NOT_AVAILABLE"));
        assert_eq!(lanes.lane_for(&none), RunLane::Terminal);
        // removing the chat of an agent on another lane leaves that lane alone
        lanes.set("codex", RunLane::Acp).unwrap();
        remove_in(&overlay, &lanes, "codex").unwrap();
        assert_eq!(lanes.lane_for("codex"), RunLane::Acp);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn registry_text_shown_to_the_person_is_plain_and_bounded() {
        let agent = RegistryAgent {
            id: "x".into(),
            name: "Evil\u{1b}[31m name".into(),
            description: "d".repeat(1000),
            authors: vec!["a\nb".into()],
            version: "1.0.0".into(),
            ..RegistryAgent::default()
        };
        let o = registry_option(&agent, &Choice::Npx { package: "p".into(), version: "1.0.0".into(), args: vec![] }, registry::Origin::Live);
        assert!(!o["registry"]["name"].as_str().unwrap().contains('\u{1b}'));
        assert!(o["summary"].as_str().unwrap().chars().count() <= 243);
        assert_eq!(o["registry"]["authors"][0], "ab");
    }

    #[test]
    fn the_install_target_must_be_what_the_person_confirmed() {
        let reg = registry::snapshot();
        let (agent, choice) = registry::resolve(&reg, "cline", "cline", "cline", &registry::platform_key()).unwrap();
        if matches!(choice, Choice::Unsupported(_)) {
            return;
        }
        let version = choice_version(agent, &choice);
        assert!(registry_target(&reg, "cline", "cline", "cline", &json!({"kind": "registry"})).is_ok());
        assert!(registry_target(&reg, "cline", "cline", "cline", &json!({"kind": "registry", "id": agent.id, "version": version})).is_ok());
        assert!(registry_target(&reg, "cline", "cline", "cline", &json!({"kind": "registry", "version": "0.0.1"})).unwrap_err().starts_with("REGISTRY_CHANGED"));
        assert!(registry_target(&reg, "none", "none", "no-such-program", &json!({})).unwrap_err().starts_with("NOT_AVAILABLE"));
    }

    #[test]
    fn the_commands_need_the_labs_switch() {
        if !acp::chat_enabled() {
            assert!(acp_overlay_set("x".into(), json!({}), true).unwrap_err().starts_with("FEATURE_OFF"));
            assert!(acp_overlay_remove("x".into()).unwrap_err().starts_with("FEATURE_OFF"));
        }
    }

    #[test]
    fn only_registered_agents_have_a_program_to_probe() {
        assert!(check_target("no_such_agent_zz").is_err());
        assert!(overlay::program_of("no_such_agent_zz").is_none());
        assert!(!overlay::program_of(&target()).unwrap().is_empty());
    }

    /// Real run against the installed program: `PM_REAL_DETECT=opencode cargo test --lib real_detect -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_detect_on_this_machine() {
        let program = std::env::var("PM_REAL_DETECT").unwrap_or_else(|_| "opencode".into());
        let wizard = crate::tui::declarative::wizard::Wizard::app();
        let started = std::time::Instant::now();
        let out = detect_in(wizard, &program);
        eprintln!("detect took {:?}: {out:#?}", started.elapsed());
        let v = out.expect("detect ran");
        assert!(!v["candidates"].as_array().unwrap().is_empty());
    }
}
