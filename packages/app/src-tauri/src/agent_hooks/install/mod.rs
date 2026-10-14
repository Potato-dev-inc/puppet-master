//! Getting PM's hook entries to the agent (§5): a launch flag (no file outside PM's own folder) or
//! a managed entry in a project file. Which one is the agent's `[hooks].install`, never its name.

pub mod launch;
pub mod project;
pub mod stable_exe;
