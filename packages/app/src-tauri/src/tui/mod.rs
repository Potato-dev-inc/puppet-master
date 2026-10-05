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
pub mod generic;
pub mod opencode;
pub mod shell;

use crate::pty::status::PaneStatus;
use serde_json::Value;
use std::path::PathBuf;

/// Registry order is the order `read_only_backends` reports; keep it stable.
static PROFILES: &[&dyn TuiProfile] = &[
    &claude::Claude,
    &codex::Codex,
    &opencode::OpenCode,
    &opencode::OpenCodeNative,
    &cursor::CursorAgent,
    &cursor::CursorIde,
    &shell::Cmd,
    &shell::Powershell,
    &shell::Bash,
];

static UNKNOWN: generic::Unknown = generic::Unknown;

#[cfg(test)]
pub fn profiles() -> &'static [&'static dyn TuiProfile] {
    PROFILES
}

/// Profile for an `agent_type`, case-insensitively; unregistered types get generic behaviour.
pub fn profile_for(agent_type: &str) -> &'static dyn TuiProfile {
    PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id().eq_ignore_ascii_case(agent_type))
        .unwrap_or(&UNKNOWN)
}

pub fn is_registered(agent_type: &str) -> bool {
    PROFILES
        .iter()
        .any(|profile| profile.id().eq_ignore_ascii_case(agent_type))
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
    PROFILES
        .iter()
        .filter(|profile| profile.features().read_only_headless)
        .map(|profile| profile.id())
        .collect()
}

/// Agent types whose run-spawned pane enforces `read_only`.
pub fn read_only_spawned_pane_backends() -> Vec<&'static str> {
    PROFILES
        .iter()
        .filter(|profile| profile.features().read_only_spawned_pane)
        .map(|profile| profile.id())
        .collect()
}

/// Backends that can have a warm standby worker, with the kinds each supports.
pub fn standby_backends() -> Vec<(&'static str, StandbySupport)> {
    PROFILES
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

/// Whether any profile treats `text` as an open menu/selector screen.
pub fn looks_like_menu(text: &str) -> bool {
    PROFILES.iter().any(|profile| profile.looks_like_menu(text))
}

/// Whether any profile treats this (trimmed, lowercased) line as TUI chrome.
pub fn is_chrome_line(lower: &str) -> bool {
    PROFILES.iter().any(|profile| profile.is_chrome_line(lower))
}

/// Screen text with every profile's boxed dialogs removed.
pub fn strip_dialogs(text: &str) -> String {
    PROFILES
        .iter()
        .fold(text.to_string(), |acc, profile| profile.strip_dialogs(&acc))
}

/// Reply text after every profile's redraw-artifact cleanup.
pub fn tidy_reply(text: &str) -> String {
    PROFILES
        .iter()
        .fold(text.to_string(), |acc, profile| profile.tidy_reply(&acc))
}

/// Whether any profile treats this (trimmed, lowercased) line as a tool-activity row.
pub fn is_tool_activity_line(lower: &str) -> bool {
    PROFILES
        .iter()
        .any(|profile| profile.is_tool_activity_line(lower))
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

#[cfg(test)]
mod conformance;
