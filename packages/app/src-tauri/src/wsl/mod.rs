//! Linux Inside: an agent can run inside a WSL 2 Linux distro instead of on Windows. Settings >
//! Agents picks the place per agent (`Runs on: Windows / Linux · <distro>`) and every spawn path
//! follows that choice. The UI calls it "Linux"; "WSL" appears only where the person must type a
//! WSL command.
//!
//! Rules (spec section 3.1, decided conservatively):
//! * Windows stays the default for every agent. The choice lives in `<app data>/wsl.json`, and only
//!   Tauri commands write it (never the bridge, MCP, or the phone).
//! * WSL 2 distros only. Docker Desktop and Rancher Desktop distros are hidden.
//! * The agent starts through `wsl.exe --exec`: argv elements stay apart and no shell parses agent
//!   or folder text. The launcher and the probe and stop scripts are fixed constants.
//! * Only folders on a Windows drive are mapped (`C:\x` becomes `/mnt/c/x`). Checkpoints,
//!   operations, worktrees and git facts keep running on Windows.
//! * Nothing of PM's own environment crosses into Linux except the names PM lists itself.
//! * A probe boots the distro, so it runs only on a person's click, a pick, or a spawn. The distro
//!   list is read on demand and never on a timer.
//! * Companion panes, hidden panes and guarded launches always run on Windows.
//!
//! Invariants (spec section 4): no credential file is copied or read across the boundary; PM never
//! answers a trust, permission or login screen in a Linux pane; PM never installs WSL or a distro,
//! never shuts WSL down, and never edits WSL config files; a stop only signals processes that carry
//! this pane's id and run as the same Linux user. Linux is not a sandbox: Linux programs can read
//! the Windows drives.

// The types land before their callers (the other wave tasks fill the files in parallel).
#![allow(dead_code)]

pub mod chat;
pub mod live;
pub mod model;
pub mod path;
pub mod parse;
pub mod launch;
pub mod stop;
pub mod probe;
pub mod place;
pub mod settings;
#[cfg(test)]
mod guard_tests;
