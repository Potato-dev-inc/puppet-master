//! `install = "launch_flag"` (§5.1): PM's hooks reach the agent as a command-line flag at spawn,
//! pointing at one JSON file in PM's own hooks folder. Nothing is written into any user or project
//! config (no `.claude/settings*.json`), so no other agent that imports those files sees PM's hooks.

use std::path::{Path, PathBuf};

use crate::agent_hooks::render;
use crate::tui::declarative::hooks::{HooksTable, LaunchValue};

/// The extra arguments for the agent's command line, or `None` when the table has no launch flag,
/// the user already passes that flag (PM never replaces the user's own settings), or the file
/// cannot be written.
pub fn args_for(scheme: &HooksTable, exe: &str, agent: &str, hooks_dir: &Path, user_args: &[String]) -> Option<Vec<String>> {
    let launch = scheme.launch.as_ref()?;
    if user_args.iter().any(|a| a == &launch.flag || a.starts_with(&format!("{}=", launch.flag))) {
        return None;
    }
    let document = render::document(scheme, &launch.root_key, launch.layout, exe, agent);
    let text = serde_json::to_string_pretty(&document).ok()?;
    match launch.value {
        LaunchValue::Inline => Some(vec![launch.flag.clone(), serde_json::to_string(&document).ok()?]),
        LaunchValue::File => {
            let path = settings_file(hooks_dir, agent);
            write_if_changed(&path, &text).ok()?;
            Some(vec![launch.flag.clone(), path.to_string_lossy().into_owned()])
        }
    }
}

/// `<hooks dir>/<agent>-settings.v1.json`: the same bytes for every pane (pane identity is in env).
pub fn settings_file(hooks_dir: &Path, agent: &str) -> PathBuf {
    hooks_dir.join(format!("{agent}-settings.v1.json"))
}

fn write_if_changed(path: &Path, text: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staged = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&staged, text)?;
    std::fs::rename(&staged, path)
}
