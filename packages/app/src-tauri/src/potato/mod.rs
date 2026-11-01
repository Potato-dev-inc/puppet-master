//! Potato Bot: a chat friend that lives in Puppet Master, with its own small brain. Phase 1 has one
//! home for it, the desktop app (a second one, a chat app, comes later). This folder is the Rust core
//! (piece P2 of the spec); the page is P3, the settings and helpers are P4.
//!
//! The brain is a separate Node program (`packages/potato-brain`). Puppet Master starts it, hands it
//! messages and takes its answers over the local bridge. It cannot do anything else:
//!
//! - `attachments`: files attached to a message (saved in the bot's folder, sent to the brain as text)
//! - `protocol`: the channel protocol v1 (inbox items, events, their size limits)
//! - `auth`: who may call `/potato/brain/*` (a new caller on the bridge) and what a brain can never do
//! - `rules`: `decide`, the one pure function that says what happens to a message
//! - `env`: the brain's environment, an allowlist built from nothing
//! - `keeper`: the pure state machine of one brain process (lazy start, idle stop, crash backoff)
//! - `sidecar`: the IO of the process (finding Node, starting hidden, stopping the whole tree)
//! - `store`: the files in `<app data>/potato/` (bots, settings, conversations, persona seeding)
//! - `open_work`, `memory_notes`, `notify`: the bot panel's Advanced tab (the bot's open work list, what it remembers,
//!   "notify me" per bot); commands in `commands/potato_panel.rs`
//! - `bot_settings`: a bot's own talk brain, instructions with their history, tool switches, delete and export
//!   (one file per bot; the Advanced tab's commands are in `commands/potato_bot_settings.rs`)
//! - `routines`: prompts a bot runs on a schedule (`routines.json`), the 30 s tick that sends the due ones
//! - `artifacts`: the files a bot made or received (versions by name, previews, "Save as file", download, delete)
//! - `cost`: pricing the tokens the brain reports and judging them against the daily limit
//! - `fallback`: the brains to try after the talk brain when its model is out of balance or down (the switch is in `service`)
//! - `usage`: the Usage page's numbers (30 days, per bot and in total), read only
//! - `log`: the 200-line log ring, every line cleaned
//! - `helpers`: Start / Change folder / Not now, the caps, and watching a helper until it ends
//! - `service`: the engine that ties them together; `live`: the app's one instance and its thread
//! - `sandbox`: the bot's small Linux computer (a WSL 2 distro the app owns): the one `wsl.exe` door, install,
//!   lazy start and idle stop, the control client and the screen lease (piece S1; the screen links, brain tools and
//!   settings page come later)
//! - `sandbox_calls`: the bot's computer as the brain's tools use it (`computer_*`, `browser_*`): the checks, the screen
//!   lease, the day's action cap, and the answer
//! - `sandbox_host`: the app's one sandbox (the real one or a stand-in), the idle-stop setting and its tick, the Labs
//!   switch (`potato_sandbox_enabled`); the settings page talks to it through `commands/potato_sandbox.rs`
//! - `resource_dirs`: where the bundled `potato-brain` and `potato-sandbox` folders are (installed layout or dev layout)
//! - `window_guard`: the check after every sandbox start that no remote-desktop window (WSLg's `msrdc`) is visible
//! - `wire_names`: names the brain package fixes that are also other names (the one reasoned exception to the
//!   "no agent names in generic code" guard)
//!
//! Keys and tokens live only in the OS credential store and in the brain's environment. Nothing here
//! names an agent or a chat app: a chat app's code would live only in `potato/surfaces/<app>/`.

pub mod attachments;
pub mod artifacts;
pub mod auth;
pub mod bot_settings;
pub mod cost;
pub mod env;
pub mod fallback;
pub mod helpers;
pub mod trash;
pub mod usage;
pub mod keeper;
pub mod live;
pub mod log;
pub mod memory_notes;
pub mod notify;
pub mod open_work;
pub mod wire_names;
pub mod protocol;
pub mod resource_dirs;
pub mod routines;
pub mod rules;
pub mod sandbox;
pub mod sandbox_calls;
pub mod sandbox_host;
pub mod service;
pub mod sidecar;
pub mod store;
pub mod voice;
pub mod window_guard;

#[cfg(test)]
mod attachments_tests;
mod artifacts_tests;
mod voice_tests;
#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod bot_settings_tests;
#[cfg(test)]
mod cost_tests;
#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod fallback_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod helpers_tests;
#[cfg(test)]
mod keeper_tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod memory_notes_tests;
#[cfg(test)]
mod notify_tests;
#[cfg(test)]
mod open_work_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod resource_dirs_tests;
#[cfg(test)]
mod routines_tests;
#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod sandbox_calls_tests;
#[cfg(test)]
mod usage_tests;
#[cfg(test)]
mod sandbox_host_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod real_brain_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod trash_tests;
#[cfg(test)]
mod window_guard_tests;
