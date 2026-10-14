//! Dev-only harness for the `pm-fake-tui` end-to-end tests (`tests/fake_tui_e2e.rs`). Compiled only
//! with the `dev-fake-tui` cargo feature, so nothing here exists in the app users run.
//!
//! What it builds: a real (windowless, event-loop-only) Tauri app handle, a real `PaneRegistry`, the
//! real pane observers (`pty://status` -> `bridge::push_pane_status_sse`), and two extra agent types,
//! `fake_claude` and `fake_codex`, whose profiles are the real Claude / Codex profiles launching
//! `tools/pm-fake-tui` on a real ConPTY under a PowerShell host shell (see `profile.rs`). A test then
//! drives the real `/agents/*` routes (`agent_runs::handle_request`), the same code the MCP shim reaches
//! over HTTP, so `run_agent` -> pane -> delivery -> readiness -> status -> turn summary -> receipt is
//! the production path with only the agent replaced.
//!
//! No bridge port is bound, no port file is written, and the process runs as the app profile
//! `fake-tui-e2e` (its own app-data folder), so a running Puppet Master is never touched.

mod profile;
mod world;

pub use world::{Session, World};
