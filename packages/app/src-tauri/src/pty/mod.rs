//! PTY management: pane registry, scrollback, status heuristics.

pub mod agent_proc;
pub mod agents;
pub mod ansi;
pub mod hidden;
pub mod keys;
pub mod launch_session;
pub mod pane_facts;
pub mod pane_parent;
pub mod proc_tree;
pub mod registry;
pub mod resize_clock;
pub mod scrollback;
pub mod status;
pub mod stream_pos;
pub mod viewer_size;
pub mod write_origin;

pub use write_origin::WriteOrigin;

/// Env var Puppet Master sets on every pane it launches: that pane's id. The MCP shim an agent starts
/// forwards it, so the bridge can tell which pane's agent made a call (a hint, see `pane_parent`).
pub const PANE_ID_ENV: &str = "PUPPET_MASTER_PANE_ID";

pub use registry::{
    get_project_path as registry_get_project_path, kill_all as registry_kill_all,
    kill_pane as registry_kill_pane,
    kill_pane_with_reason as registry_kill_pane_with_reason, read_buffer as registry_read_buffer,
    read_full_text as registry_read_full_text, read_raw_buffer as registry_read_raw_buffer, read_screen_replay_at as registry_read_screen_replay_at, read_snapshot as registry_read_snapshot, read_snapshot_full as registry_read_snapshot_full, read_snapshot_full_at as registry_read_snapshot_full_at,
    resize as registry_resize, set_project_path as registry_set_project_path,
    spawn_pane as registry_spawn_pane, write_input as registry_write_input, PaneInfo, PaneRegistry,
    SpawnPaneArgs,
};
