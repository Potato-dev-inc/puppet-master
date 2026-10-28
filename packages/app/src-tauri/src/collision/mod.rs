//! Collision Alert (SPEC-n33): tells the person early when two agent panes are changing the same
//! work. Either the same file in two separate copies of one repo, or two agents working in one
//! folder. It only tells. It never pauses, stops, types into or merges anything.
//!
//! The person sees a plain card in the Inbox, a banner in the two chats, and a Ready-lane item on
//! the phone. Nothing is written to disk except `collision.json`. Alerts live in memory.
//!
//! Modules:
//! - `model`: the limits and the types (`Alert`, `AlertView`, ...).
//! - `settings`: `collision.json`, on by default.
//! - `repo`: finds the git repo of a folder (no process). `noise`: which changed files never count.
//! - `hunks`: parses `git diff -U0` output. `advice`: which side to finish first (fixed rules).
//! - `groups`: panes that share a repo, and the look schedule. `compare`: copies and same folder.
//! - `alerts`: one alert per pair, in-place updates, ignore. `copy` and `view`: the exact words.
//! - `probe`: the git reads (hardened). `engine`: one pure step. `live`: the watch thread and the
//!   snapshot the commands read.
//!
//! Invariants (each one has a test in the module named, or in `tests`):
//! - C1 A repo's own git config and hooks never run. Every git call goes through `worktrees::harden`.
//! - C2 Only read-only plumbing (`diff-index`, `ls-files`, `rev-parse`, one `diff` per shared file).
//!   `GIT_OPTIONAL_LOCKS=0` is always set, so the agent's own `index.lock` is never touched.
//! - C3 Bounded cost: gaps with backoff, parking after slow looks, caps, one look at a time on one
//!   thread, a 10 s kill per git process.
//! - C4 Text on screen is sanitised and redacted. Paths with control or bidi characters are dropped.
//! - C5 Paths are normalised. Stat uses `symlink_metadata`. No file content is read, only line numbers.
//! - C6 Desktop Tauri commands only. No bridge route, no MCP tool, no team, push, channel or live room.
//! - C7 No write path to any agent: no input, no key, no interrupt, no stop, no merge.
//! - C8 Ids are checked by a regex. A forged id changes nothing.
//! - C9 Noise control: quiet levels, one alert per pair, in-place updates, a fixed noise list.
//! - C10 A pane is blamed only for files changed after it was created. The start commit is recorded
//!   at the first look.
//! - C11 No personal data in strings, logs or fixtures.
//! - C12 Generic code names no agent. Labels come from the pane descriptors and headings.
//! - C13 No new bridge route. The loopback trust rules are untouched.

// The watch thread is started (CA-13), but the commands that read `live::views`, `ignore` and the
// settings come with CA-14. Remove this allow once they are wired in.
#![allow(dead_code)]

pub mod model;
pub mod settings;
pub mod repo;
pub mod noise;
pub mod hunks;
pub mod advice;
pub mod groups;
pub mod compare;
pub mod alerts;
pub mod copy;
pub mod view;
pub mod probe;
pub mod engine;
pub mod live;

#[cfg(test)]
mod tests;
