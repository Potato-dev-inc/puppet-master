//! PTY management: pane registry, scrollback, status heuristics.

pub mod agents;
pub mod ansi;
pub mod registry;
pub mod scrollback;
pub mod status;

pub use registry::{
    get_project_path as registry_get_project_path, kill_all as registry_kill_all,
    kill_pane as registry_kill_pane, read_buffer as registry_read_buffer,
    read_raw_buffer as registry_read_raw_buffer, read_snapshot as registry_read_snapshot,
    resize as registry_resize, set_project_path as registry_set_project_path,
    spawn_pane as registry_spawn_pane, write_input as registry_write_input, PaneInfo, PaneRegistry,
    SpawnPaneArgs,
};
