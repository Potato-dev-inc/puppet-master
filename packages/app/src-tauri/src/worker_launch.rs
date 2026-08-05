//! Standalone worker terminal launch — opens a single terminal window without the full grid UI.

use once_cell::sync::OnceCell;
use serde::Serialize;

static WORKER_LAUNCH: OnceCell<Option<WorkerLaunch>> = OnceCell::new();

#[derive(Debug, Clone, Serialize)]
pub struct WorkerLaunch {
    pub agent_type: String,
    pub cwd: Option<String>,
    pub pane_id: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub force_new: bool,
}

pub fn init_from_env() {
    let _ = WORKER_LAUNCH.set(parse_worker_launch(std::env::args().skip(1)));
}

pub fn worker_launch() -> Option<WorkerLaunch> {
    WORKER_LAUNCH
        .get()
        .and_then(|value| value.clone())
        .or_else(|| parse_worker_launch(std::env::args().skip(1)))
}

fn default_terminal_agent() -> &'static str {
    match crate::platform::current_os() {
        crate::platform::Os::Windows => "powershell",
        _ => "bash",
    }
}

fn parse_worker_launch<I>(args: I) -> Option<WorkerLaunch>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    let worker_mode = args.iter().any(|arg| arg == "--worker")
        || std::env::var("PUPPET_MASTER_WORKER")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    if !worker_mode {
        return None;
    }

    let mut agent_type = std::env::var("PUPPET_MASTER_WORKER_AGENT").ok();
    let mut cwd = std::env::var("PUPPET_MASTER_WORKER_CWD").ok();
    let mut pane_id = std::env::var("PUPPET_MASTER_WORKER_PANE_ID").ok();
    let mut cols: u16 = 120;
    let mut rows: u16 = 32;
    let mut force_new = false;

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--worker" => {}
            "--agent-type" | "--agent" => {
                index += 1;
                agent_type = args.get(index).cloned();
            }
            "--cwd" | "--project" => {
                index += 1;
                cwd = args.get(index).cloned();
            }
            "--pane-id" | "--pane" => {
                index += 1;
                pane_id = args.get(index).cloned();
            }
            "--cols" => {
                index += 1;
                if let Some(value) = args.get(index).and_then(|raw| raw.parse().ok()) {
                    cols = value;
                }
            }
            "--rows" => {
                index += 1;
                if let Some(value) = args.get(index).and_then(|raw| raw.parse().ok()) {
                    rows = value;
                }
            }
            "--force-new" => force_new = true,
            _ => {}
        }
        index += 1;
    }

    Some(WorkerLaunch {
        agent_type: agent_type.unwrap_or_else(|| default_terminal_agent().to_string()),
        cwd,
        pane_id,
        cols,
        rows,
        force_new,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_worker_launch_args() {
        let launch = parse_worker_launch([
            "--worker".to_string(),
            "--agent-type".to_string(),
            "claude".to_string(),
            "--cwd".to_string(),
            "~/work/repo".to_string(),
            "--cols".to_string(),
            "100".to_string(),
            "--rows".to_string(),
            "28".to_string(),
            "--force-new".to_string(),
        ])
        .expect("worker launch");
        assert_eq!(launch.agent_type, "claude");
        assert_eq!(launch.cwd.as_deref(), Some("~/work/repo"));
        assert_eq!(launch.cols, 100);
        assert_eq!(launch.rows, 28);
        assert!(launch.force_new);
    }

    #[test]
    fn ignores_non_worker_args() {
        assert!(parse_worker_launch(["--project".to_string(), "/tmp".to_string()]).is_none());
    }
}
