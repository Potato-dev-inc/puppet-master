//! TUI profiles: everything that is specific to one agent CLI/TUI lives in its own submodule here.
//!
//! The generic machinery (pane registry, run lifecycle, wait, approvals, steering) never matches
//! on an agent-type string. It asks `profile_for(agent_type)` and calls a `TuiProfile` method, so a
//! fix for one TUI stays inside that TUI's folder. See `README.md` in this folder for how to add one.

mod profile;
mod types;

pub use profile::*;
pub use types::*;

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod declarative;
pub mod generic;
pub mod opencode;
pub mod registry;
pub mod shell;

use crate::pty::status::PaneStatus;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Unregistered `agent_type` with the ids callers may use instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAgentType {
    pub id: String,
    pub valid: Vec<String>,
}

/// Registered ids for error context: dispatchable backends first, then the rest in registry order.
pub fn registered_agent_ids() -> Vec<String> {
    let mut dispatchable = Vec::new();
    let mut rest = Vec::new();
    for profile in registry::snapshot().iter() {
        let id = profile.id();
        if profile.features().dispatchable {
            dispatchable.push(id.to_string());
        } else {
            rest.push(id.to_string());
        }
    }
    dispatchable.extend(rest);
    dispatchable
}

/// Every registered agent as `GET /agents/types` lists it, in registry order.
pub fn descriptors() -> Vec<Descriptor> {
    registry::snapshot()
        .iter()
        .map(|profile| {
            let mut descriptor = Descriptor::of(profile.as_ref());
            overlay_definition(&mut descriptor);
            descriptor
        })
        .collect()
}

/// A declarative agent: source, label, icon, install hint, modes and `definition` come from its
/// loaded definition (code profiles have no loader row and stay as `Descriptor::of` built them).
fn overlay_definition(descriptor: &mut Descriptor) {
    let Some(entry) = declarative::loader::last_entry_for(descriptor.id) else { return };
    descriptor.source = entry.source;
    if let Some(meta) = &entry.meta {
        descriptor.label = meta.label.clone();
        descriptor.icon = Some(meta.icon.clone());
        descriptor.install_hint = meta.install_hint.clone();
        descriptor.modes.visible = meta.has_tui;
    }
    descriptor.definition = Some(serde_json::json!({
        "path": entry.path.to_string_lossy(),
        "fingerprint": entry.fingerprint,
        "state": entry.state.as_str(),
    }));
}

/// Body of `GET /agents/types`: `{agents, dispatchable_ids}` (SPEC 7.5).
pub fn agent_types_listing() -> Value {
    let agents = descriptors();
    let dispatchable_ids: Vec<&str> = agents
        .iter()
        .filter(|agent| agent.dispatchable)
        .map(|agent| agent.id)
        .collect();
    serde_json::json!({ "agents": agents, "dispatchable_ids": dispatchable_ids })
}

/// Every registered profile, in registry order (a copy of the current snapshot).
#[cfg(test)]
pub fn profiles() -> Vec<Arc<dyn TuiProfile>> {
    registry::registry().ordered()
}

/// Profile for an `agent_type`, case-insensitively; unregistered types get generic behaviour.
/// Hot path (pane loops): one atomic snapshot load plus an `Arc` clone.
pub fn profile_for(agent_type: &str) -> Arc<dyn TuiProfile> {
    registry::profile_for_arc(agent_type)
}

pub fn is_registered(agent_type: &str) -> bool {
    registry::snapshot()
        .iter()
        .any(|profile| profile.id().eq_ignore_ascii_case(agent_type))
}

/// Resolve a registered profile or return the canonical id list for `UNKNOWN_AGENT_TYPE`.
pub fn require_registered(agent_type: &str) -> Result<Arc<dyn TuiProfile>, UnknownAgentType> {
    match registry::snapshot()
        .iter()
        .find(|profile| profile.id().eq_ignore_ascii_case(agent_type))
        .cloned()
    {
        Some(profile) => Ok(profile),
        None => Err(UnknownAgentType {
            id: agent_type.to_string(),
            valid: registered_agent_ids(),
        }),
    }
}

/// A registered agent backend (not a plain shell) that `delegate_work` may be asked for.
pub fn is_agent_backend(agent_type: &str) -> bool {
    is_registered(agent_type) && profile_for(agent_type).features().kind != Kind::Shell
}

pub fn is_shell(agent_type: &str) -> bool {
    profile_for(agent_type).features().kind == Kind::Shell
}

/// Agent types whose headless run enforces `read_only`.
pub fn read_only_backends() -> Vec<&'static str> {
    registry::snapshot()
        .iter()
        .filter(|profile| profile.features().read_only_headless)
        .map(|profile| profile.id())
        .collect()
}

/// Agent types whose run-spawned pane enforces `read_only`.
pub fn read_only_spawned_pane_backends() -> Vec<&'static str> {
    registry::snapshot()
        .iter()
        .filter(|profile| profile.features().read_only_spawned_pane)
        .map(|profile| profile.id())
        .collect()
}

/// Backends that can have a warm standby worker, with the kinds each supports.
pub fn standby_backends() -> Vec<(&'static str, StandbySupport)> {
    registry::snapshot()
        .iter()
        .map(|profile| (profile.id(), profile.standby_support()))
        .filter(|(_, support)| support.visible || support.headless)
        .collect()
}

/// Headless command line for `agent_type`, after the checks every backend shares.
pub fn headless_launch(agent_type: &str, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
    if launch.allow_shell && launch.read_only {
        return Err("allow_shell cannot be combined with read_only".into());
    }
    let profile = profile_for(agent_type);
    if launch.read_only && !profile.features().read_only_headless {
        return Err(format!(
            "READ_ONLY_UNSUPPORTED: read-only headless execution is unsupported for agent type: {agent_type}"
        ));
    }
    profile.headless_command(launch)
}

/// Whether this agent's profile treats `text` as an open menu/selector screen.
pub fn looks_like_menu(text: &str, agent_type: &str) -> bool {
    profile_for(agent_type).looks_like_menu(text)
}

/// Whether this agent's profile treats this (trimmed, lowercased) line as TUI chrome.
pub fn is_chrome_line(lower: &str, agent_type: &str) -> bool {
    profile_for(agent_type).is_chrome_line(lower)
}

/// Screen text with this agent's profile removing boxed dialogs (and related chrome) only.
pub fn strip_dialogs(text: &str, agent_type: &str) -> String {
    profile_for(agent_type).strip_dialogs(text)
}

/// Reply text after this agent's profile redraw-artifact cleanup only.
pub fn tidy_reply(text: &str, agent_type: &str) -> String {
    profile_for(agent_type).tidy_reply(text)
}

/// Whether this agent's profile treats this (trimmed, lowercased) line as a tool-activity row.
pub fn is_tool_activity_line(lower: &str, agent_type: &str) -> bool {
    profile_for(agent_type).is_tool_activity_line(lower)
}

/// Whether an executable called `name` is on PATH (`name.exe` on Windows).
pub(crate) fn program_on_path(name: &str) -> bool {
    #[cfg(windows)]
    {
        find_path_file(&format!("{name}.exe")).is_some()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
        })
    }
}

#[cfg(windows)]
pub(crate) fn find_path_file(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Program to spawn for a CLI called `name`. Windows needs the full path to the `.exe` because
/// ConPTY/CreateProcess does not search for it the way a shell does.
pub(crate) fn executable(name: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let file = format!("{name}.exe");
        find_path_file(&file)
            .map(|path| path.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{file} was not found on PATH"))
    }
    #[cfg(not(windows))]
    {
        Ok(name.to_string())
    }
}

/// Program path for a declarative definition (SPEC 7.3): scans `PATH` without the current or the
/// project directory and follows npm `.cmd` shims to the native binary. `executable` is untouched.
#[allow(dead_code)] // the declarative profile (T16) calls it
pub(crate) fn resolve_binary(
    binary: &declarative::schema::BinaryTable,
    project_dir: &std::path::Path,
) -> Result<PathBuf, declarative::resolve::ResolveError> {
    declarative::resolve::resolve_binary(binary, project_dir)
}

#[cfg(test)]
mod conformance;

#[cfg(test)]
mod scoping {
    use super::*;

    /// Claude's finished-turn summary row (`claude/screen.rs`); Cursor's idle placeholder
    /// (`cursor/chrome/tests.rs`) is chrome only for `cursor_agent`.
    #[test]
    fn scoping_chrome_rule_of_a_does_not_touch_b() {
        let claude_done = "\u{273b} churned for 16s \u{b7} done 23:03";
        assert!(is_chrome_line(claude_done, "claude"));
        assert!(!is_chrome_line(claude_done, "cursor_agent"));

        let cursor_placeholder = "→ plan, search, build anything";
        assert!(is_chrome_line(cursor_placeholder, "cursor_agent"));
        assert!(!is_chrome_line(cursor_placeholder, "claude"));
    }

    #[test]
    fn require_registered_lists_nine_valid_ids() {
        let _serial = registry::test_serial();
        let err = match require_registered("not-a-real-agent") {
            Err(err) => err,
            Ok(_) => panic!("expected unknown agent type"),
        };
        assert_eq!(err.id, "not-a-real-agent");
        assert_eq!(err.valid.len(), 9);
        assert_eq!(
            err.valid,
            registered_agent_ids(),
            "valid ids are dispatchable backends first"
        );
        assert!(require_registered("claude").is_ok());
    }

    #[test]
    fn scoping_strip_dialogs_only_runs_selected_profile() {
        const TRUST: &str =
            include_str!("cursor/fixtures/workspace_trust_visible_resolved.txt");
        let claude_stripped = strip_dialogs(TRUST, "claude");
        assert!(
            claude_stripped.contains("Workspace Trust Required"),
            "claude profile must not run Cursor dialog stripping"
        );
        let cursor_stripped = strip_dialogs(TRUST, "cursor_agent");
        assert!(
            !cursor_stripped.contains("Workspace Trust Required"),
            "{cursor_stripped}"
        );
    }
}

#[cfg(test)]
mod order {
    use super::*;

    /// `read_only_backends()` keeps registry order; the registry snapshot reports the same order.
    #[test]
    fn read_only_backends_order_unchanged() {
        assert_eq!(read_only_backends(), vec!["claude", "codex", "cursor_agent"]);
        let from_registry: Vec<&str> = registry::Registry::seeded()
            .ordered()
            .iter()
            .filter(|profile| profile.features().read_only_headless)
            .map(|profile| profile.id())
            .collect();
        assert_eq!(from_registry, read_only_backends());
    }
}

#[cfg(test)]
mod descriptor {
    use super::*;

    #[test]
    fn descriptor_builds_all_nine() {
        let _serial = registry::test_serial();
        let all = descriptors();
        assert_eq!(all.len(), 9);
        let ids: Vec<&str> = all.iter().map(|d| d.id).collect();
        let expected: Vec<&str> = profiles().iter().map(|p| p.id()).collect();
        assert_eq!(ids, expected);
        for d in &all {
            assert!(!d.label.is_empty(), "{} has a label", d.id);
            assert_eq!(d.source, "code");
            assert!(!d.experimental);
            assert_eq!(d.not_dispatchable_reason.is_some(), !d.dispatchable, "{}", d.id);
        }
    }

    #[test]
    fn descriptor_modes_follow_features() {
        let _serial = registry::test_serial();
        for profile in profiles() {
            let d = Descriptor::of(profile.as_ref());
            let features = profile.features();
            assert!(d.modes.visible, "{} is always visible", d.id);
            assert_eq!(d.dispatchable, features.dispatchable);
            assert_eq!(d.features.read_only_headless, features.read_only_headless);
            assert_eq!(d.features.resumable_headless, features.resumable_headless);
            if d.modes.headless {
                assert!(features.dispatchable && features.kind == Kind::Cli, "{}", d.id);
            }
        }
        let headless: Vec<&str> = descriptors()
            .into_iter()
            .filter(|d| d.modes.headless)
            .map(|d| d.id)
            .collect();
        assert_eq!(headless, vec!["claude", "codex", "cursor_agent"]);
    }
}
