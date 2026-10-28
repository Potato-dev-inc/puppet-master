//! Night Shift: a queue of tasks that run on this computer while the person sleeps.
//!
//! Every task runs as a mission (or one single turn) in its own separate copy of the project.
//! Puppet Master never answers a trust, permission or login question, never types into a pane and
//! never merges by itself. Keep and Throw away are the person's choices, made on the desktop.
//!
//! Stub layout for this branch: only the modules that exist are declared here. NS-1 owns this file
//! and adds the rest of the submodules when it lands.

pub mod classify;
pub mod model;
pub mod report;
