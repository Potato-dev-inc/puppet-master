//! CLI-update safety: version pins, strict mode, the zero-token canary and unknown-screen capture.
//!
//! Every CLI update can change the screens Puppet Master reads through a `TuiProfile`. Tonight's
//! Codex update menu is the example: a run typed over it and lost its task. This module makes a
//! version change visible and safe instead of silent:
//!
//! 1. **Tested versions.** A profile may declare the CLI versions its fixtures cover
//!    (`TuiProfile::tested_versions`, or `[profile] tested` in a TOML definition).
//! 2. **Strict mode.** Outside that range, or after a check found screens the profile does not
//!    recognise, the agent is in strict mode (`strict`): startup notices are no longer dismissed for
//!    the person, the pane shows an "Untested version" chip, and its warm standbys go cold.
//!    Strict mode never stops the person; it stops Puppet Master from guessing.
//! 3. **The canary** (`run`, `engine`). When a CLI's version changes (or the person asks), the CLI
//!    is started in a scratch folder and driven to its input box with the profile's own gate rules.
//!    It sends no prompt, so it spends no tokens. A failure puts the agent in strict mode and raises
//!    an Inbox card.
//! 4. **Unknown screens** (`unknown`, `captures`). A live pane that stays on a screen its profile
//!    cannot read has that screen saved, redacted, to a folder the person can open.
//! 5. **Results, not only failures.** Each record keeps the previous version, when it changed and
//!    the exact versions a check passed on here (`verified`). A pass clears the "Untested version"
//!    chip for that exact version only; the next update starts unverified. The Inbox and Settings
//!    say "Claude Code 2.3.0 -> 2.4.0: checked, all good." or "...: screens changed."
//! 6. **Runtime signals** (`note_signal`). A screen the profile cannot read, a run that could not
//!    type its task, a turn that went quiet without saying it finished: each may start one check per
//!    agent and version, rate limited, only with the auto-check switch on. Two failed checks of one
//!    kind in a row on one version mark the screens changed.
//! 7. **The spawn gate** (`before_run`). Before a run, the program's path, size and modified time
//!    are compared with the last probe; only a difference re-reads `--version` at once.
//!
//! Agent-agnostic: nothing here matches an agent id. What "ready" or "a gate" looks like is the
//! profile's.

pub mod binary;
pub mod captures;
pub mod engine;
mod live;
pub mod model;
pub mod run;
pub mod state;
pub mod strict;
pub mod unknown;
pub mod version;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod live_tests;

pub use live::{before_run, check, dismiss_notice, init, note_signal, open_captures, set_auto_check, status};
#[cfg(feature = "dev-fake-tui")]
pub(crate) use live::run_live;

use std::time::Duration;

pub use model::Signal;

/// Save a screen that stayed unreadable on a live pane (called by the `unknown` ticker).
pub(crate) fn save_unrecognised(agent: &str, screen: &str, unreadable_for: Duration) {
    let version = live::version_of(agent);
    let saved = captures::save(
        &captures::root(),
        agent,
        version.as_deref(),
        screen,
        unreadable_for.as_millis() as u64,
        crate::event_log::now_ms(),
    );
    if let Some(path) = saved {
        tracing::info!(agent, version = ?version, path = %path.display(), "saved a screen Puppet Master did not recognise");
    }
}

/// The unit tests share the process-wide strict set; those that change it hold this lock.
#[cfg(test)]
pub(crate) fn test_lock() -> parking_lot::MutexGuard<'static, ()> {
    static LOCK: once_cell::sync::Lazy<parking_lot::Mutex<()>> = once_cell::sync::Lazy::new(Default::default);
    LOCK.lock()
}
